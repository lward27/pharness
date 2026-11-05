//! The Campus egress proxy (LUC-130).
//!
//! Campus session Pods have no route to anything but this proxy and the
//! broker, and no DNS. The proxy is the only way out, and it lets through
//! only what [`pharness_core::campus_egress`] approves:
//!
//! 1. `CONNECT <approved host>:443` only. Any other method, port, raw IP or
//!    host is refused before anything is resolved.
//! 2. The connection's source address must belong to exactly one running
//!    session Pod in a lab slot, labelled for egress by the provider, and the
//!    host must be in that Pod's approved-hosts annotation. The lookup goes to
//!    the Kubernetes API on every tunnel; if it cannot be made the tunnel is
//!    refused.
//! 3. The proxy resolves the host itself and refuses it when any answer is
//!    not a public unicast address, then dials an address it checked, so a
//!    rebinding answer is never used.
//! 4. It completes verified TLS to the upstream (web PKI roots, the approved
//!    name) before accepting the tunnel, then terminates the session's TLS
//!    with a certificate from the Campus egress CA, offering only HTTP/1.1.
//! 5. Every request inside the tunnel is checked with
//!    [`check_campus_egress_request`] and re-sent over the verified upstream
//!    connection. Redirects go back to the client; following one opens a new
//!    tunnel that is checked from step 1.

use anyhow::Context;
use http_body_util::{combinators::BoxBody, BodyExt, Full, Limited};
use hyper::body::{Bytes, Incoming};
use hyper::header::{self, HeaderMap, HeaderName};
use hyper::{Request, Response, StatusCode};
use hyper_util::rt::{TokioIo, TokioTimer};
use pharness_core::campus_egress::{
    check_campus_egress_request, parse_campus_connect_target, parse_campus_egress_hosts,
    select_campus_egress_addresses, CampusEgressDenial, CampusEgressRequest,
    CAMPUS_EGRESS_HOSTS_ANNOTATION, CAMPUS_EGRESS_LABEL, CAMPUS_EGRESS_LABEL_VALUE,
    CAMPUS_EGRESS_MAX_REQUEST_BODY_BYTES, CAMPUS_EGRESS_PORT,
};
use rustls::pki_types::pem::PemObject;
use rustls::pki_types::{CertificateDer, PrivateKeyDer, ServerName};
use rustls::server::{ClientHello, ResolvesServerCert};
use rustls::sign::CertifiedKey;
use std::collections::BTreeSet;
use std::net::{IpAddr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};
use std::time::Duration;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::Mutex;

const MAX_CONNECT_HEAD_BYTES: usize = 16 * 1024;
const CONNECT_HEAD_TIMEOUT: Duration = Duration::from_secs(10);
const RESOLVE_TIMEOUT: Duration = Duration::from_secs(5);
const DIAL_TIMEOUT: Duration = Duration::from_secs(10);
const TLS_TIMEOUT: Duration = Duration::from_secs(10);
const REQUEST_HEAD_TIMEOUT: Duration = Duration::from_secs(30);
/// A tunnel lives at most this long; a client reconnects and is checked
/// again.
const MAX_TUNNEL_LIFETIME: Duration = Duration::from_secs(30 * 60);
const CERTIFICATE_RELOAD_INTERVAL: Duration = Duration::from_secs(300);
const SERVICE_ACCOUNT_DIR: &str = "/var/run/secrets/kubernetes.io/serviceaccount";
const RECORD_LABEL: &str = "pharness.dev/campus-record";
const CONTROLLER_LABEL: &str = "pharness.dev/campus-controller";

/// Headers that belong to one hop and are never forwarded.
const HOP_BY_HOP: &[&str] = &[
    "connection",
    "proxy-connection",
    "keep-alive",
    "proxy-authorization",
    "proxy-authenticate",
    "te",
    "trailer",
    "upgrade",
];

type ProxyBody = BoxBody<Bytes, Box<dyn std::error::Error + Send + Sync>>;

struct ProxyConfig {
    bind: String,
    tls_dir: PathBuf,
    slot_namespaces: Vec<String>,
    controller: String,
}

impl ProxyConfig {
    fn from_env() -> anyhow::Result<Self> {
        let slot_namespaces = serde_json::from_str::<Vec<String>>(
            &std::env::var("PHARNESS_CAMPUS_EGRESS_SLOT_NAMESPACES_JSON")
                .context("PHARNESS_CAMPUS_EGRESS_SLOT_NAMESPACES_JSON is required")?,
        )
        .context("PHARNESS_CAMPUS_EGRESS_SLOT_NAMESPACES_JSON must be a JSON list")?;
        let valid_name = |name: &str| {
            !name.is_empty()
                && name.len() <= 63
                && name
                    .bytes()
                    .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-')
        };
        if slot_namespaces.is_empty() || !slot_namespaces.iter().all(|name| valid_name(name)) {
            anyhow::bail!("the slot namespace list is empty or invalid");
        }
        let controller = std::env::var("PHARNESS_CAMPUS_EGRESS_CONTROLLER")
            .unwrap_or_else(|_| "pharness-campus-controller".into());
        if !valid_name(&controller) {
            anyhow::bail!("the controller label value is invalid");
        }
        Ok(Self {
            bind: std::env::var("PHARNESS_CAMPUS_EGRESS_BIND")
                .unwrap_or_else(|_| "0.0.0.0:3128".into()),
            tls_dir: std::env::var("PHARNESS_CAMPUS_EGRESS_TLS_DIR")
                .unwrap_or_else(|_| "/etc/campus-egress/tls".into())
                .into(),
            slot_namespaces,
            controller,
        })
    }
}

/// The session a tunnel belongs to.
#[derive(Debug, Clone, PartialEq, Eq)]
struct CampusSession {
    namespace: String,
    pod: String,
    hosts: BTreeSet<&'static str>,
}

/// Finds the session Pod that owns a source address through the
/// Kubernetes API, with the proxy's own read-only identity.
struct SessionDirectory {
    client: reqwest::Client,
    api: String,
    token_path: PathBuf,
    namespaces: Vec<String>,
    controller: String,
}

impl SessionDirectory {
    fn in_cluster(namespaces: Vec<String>, controller: String) -> anyhow::Result<Self> {
        let host = std::env::var("KUBERNETES_SERVICE_HOST")
            .context("the proxy must run in the cluster")?;
        let port = std::env::var("KUBERNETES_SERVICE_PORT").unwrap_or_else(|_| "443".into());
        let ca = std::fs::read(Path::new(SERVICE_ACCOUNT_DIR).join("ca.crt"))
            .context("cannot read the cluster CA")?;
        let client = reqwest::Client::builder()
            .tls_built_in_root_certs(false)
            .add_root_certificate(reqwest::Certificate::from_pem(&ca)?)
            .no_proxy()
            .timeout(Duration::from_secs(5))
            .build()?;
        let host = if host.contains(':') {
            format!("[{host}]")
        } else {
            host
        };
        Ok(Self {
            client,
            api: format!("https://{host}:{port}"),
            token_path: Path::new(SERVICE_ACCOUNT_DIR).join("token"),
            namespaces,
            controller,
        })
    }

    async fn lookup(&self, source: IpAddr) -> anyhow::Result<Option<CampusSession>> {
        // Read on every lookup: the projected token is rotated in place.
        let token = tokio::fs::read_to_string(&self.token_path)
            .await
            .context("cannot read the service account token")?;
        let selector = format!(
            "{CONTROLLER_LABEL}={},{RECORD_LABEL}=workload,{CAMPUS_EGRESS_LABEL}={CAMPUS_EGRESS_LABEL_VALUE}",
            self.controller
        );
        let mut lookups = tokio::task::JoinSet::new();
        for namespace in &self.namespaces {
            let request = self
                .client
                .get(format!("{}/api/v1/namespaces/{namespace}/pods", self.api))
                .bearer_auth(token.trim())
                .query(&[
                    ("labelSelector", selector.clone()),
                    ("fieldSelector", format!("status.podIP={source}")),
                ]);
            let namespace = namespace.clone();
            lookups.spawn(async move {
                let response = request.send().await?.error_for_status()?;
                let list = response.json::<serde_json::Value>().await?;
                anyhow::Ok((namespace, list))
            });
        }
        let mut sessions = Vec::new();
        while let Some(joined) = lookups.join_next().await {
            let (namespace, list) = joined.context("session lookup task failed")??;
            sessions.extend(sessions_in(&namespace, &list, source));
        }
        match sessions.as_slice() {
            [] => Ok(None),
            [session] => Ok(Some(session.clone())),
            _ => anyhow::bail!("{} session Pods claim {source}", sessions.len()),
        }
    }
}

/// The running, egress-labelled session Pods with this address in a Pod
/// list. A Pod whose annotation does not parse admits nothing.
fn sessions_in(namespace: &str, list: &serde_json::Value, source: IpAddr) -> Vec<CampusSession> {
    let source = source.to_string();
    list.get("items")
        .and_then(serde_json::Value::as_array)
        .into_iter()
        .flatten()
        .filter(|pod| {
            pod.pointer("/status/phase")
                .and_then(serde_json::Value::as_str)
                == Some("Running")
                && pod.pointer("/metadata/deletionTimestamp").is_none()
                && pod
                    .pointer("/status/podIP")
                    .and_then(serde_json::Value::as_str)
                    == Some(source.as_str())
                && pod
                    .pointer("/metadata/labels")
                    .and_then(|labels| labels.get(CAMPUS_EGRESS_LABEL))
                    .and_then(serde_json::Value::as_str)
                    == Some(CAMPUS_EGRESS_LABEL_VALUE)
        })
        .map(|pod| CampusSession {
            namespace: namespace.to_string(),
            pod: pod
                .pointer("/metadata/name")
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default()
                .to_string(),
            hosts: pod
                .pointer("/metadata/annotations")
                .and_then(|annotations| annotations.get(CAMPUS_EGRESS_HOSTS_ANNOTATION))
                .and_then(serde_json::Value::as_str)
                .and_then(|value| parse_campus_egress_hosts(value).ok())
                .unwrap_or_default(),
        })
        .collect()
}

/// The proxy's certificate for the approved hosts, issued by the Campus
/// egress CA and renewed by cert-manager in place.
#[derive(Debug)]
struct ReloadingCertificate {
    dir: PathBuf,
    current: RwLock<Arc<CertifiedKey>>,
}

impl ReloadingCertificate {
    fn load(dir: &Path) -> anyhow::Result<Arc<CertifiedKey>> {
        let chain = CertificateDer::pem_file_iter(dir.join("tls.crt"))
            .context("cannot read tls.crt")?
            .collect::<Result<Vec<_>, _>>()
            .context("tls.crt is not PEM certificates")?;
        if chain.is_empty() {
            anyhow::bail!("tls.crt holds no certificate");
        }
        let key =
            PrivateKeyDer::from_pem_file(dir.join("tls.key")).context("cannot read tls.key")?;
        let signing_key = rustls::crypto::ring::sign::any_supported_type(&key)
            .context("tls.key is not a supported private key")?;
        Ok(Arc::new(CertifiedKey::new(chain, signing_key)))
    }

    fn new(dir: PathBuf) -> anyhow::Result<Arc<Self>> {
        let current = RwLock::new(Self::load(&dir)?);
        Ok(Arc::new(Self { dir, current }))
    }

    fn reload(&self) {
        match Self::load(&self.dir) {
            Ok(key) => {
                if let Ok(mut current) = self.current.write() {
                    *current = key;
                }
            }
            Err(error) => tracing::warn!(%error, "campus egress certificate reload failed"),
        }
    }
}

impl ResolvesServerCert for ReloadingCertificate {
    fn resolve(&self, _hello: ClientHello<'_>) -> Option<Arc<CertifiedKey>> {
        self.current.read().ok().map(|current| Arc::clone(&current))
    }
}

struct Proxy {
    directory: SessionDirectory,
    acceptor: tokio_rustls::TlsAcceptor,
    connector: tokio_rustls::TlsConnector,
}

pub async fn execute() -> anyhow::Result<()> {
    let config = ProxyConfig::from_env()?;
    let provider = Arc::new(rustls::crypto::ring::default_provider());
    let certificate = ReloadingCertificate::new(config.tls_dir.clone())?;
    let mut server = rustls::ServerConfig::builder_with_provider(provider.clone())
        .with_safe_default_protocol_versions()?
        .with_no_client_auth()
        .with_cert_resolver(certificate.clone());
    // HTTP/1.1 only: every request is then a separately checked message.
    server.alpn_protocols = vec![b"http/1.1".to_vec()];
    let mut roots = rustls::RootCertStore::empty();
    roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
    let mut client = rustls::ClientConfig::builder_with_provider(provider)
        .with_safe_default_protocol_versions()?
        .with_root_certificates(roots)
        .with_no_client_auth();
    client.alpn_protocols = vec![b"http/1.1".to_vec()];
    let proxy = Arc::new(Proxy {
        directory: SessionDirectory::in_cluster(config.slot_namespaces.clone(), config.controller)?,
        acceptor: tokio_rustls::TlsAcceptor::from(Arc::new(server)),
        connector: tokio_rustls::TlsConnector::from(Arc::new(client)),
    });
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(CERTIFICATE_RELOAD_INTERVAL).await;
            certificate.reload();
        }
    });
    let listener = TcpListener::bind(&config.bind).await?;
    tracing::info!(bind = %config.bind, slots = ?config.slot_namespaces, "campus egress proxy ready");
    loop {
        let (stream, peer) = listener.accept().await?;
        let proxy = Arc::clone(&proxy);
        tokio::spawn(async move {
            if let Err(error) = proxy.tunnel(stream, peer).await {
                tracing::info!(%peer, error = %format!("{error:#}"), "campus egress tunnel ended");
            }
        });
    }
}

/// What a `CONNECT` head asks for.
#[derive(Debug, PartialEq, Eq)]
enum ConnectHead<'a> {
    Connect {
        authority: &'a str,
    },
    /// Any other request to the proxy, such as plain-HTTP forwarding.
    Other,
    Malformed,
}

fn parse_connect_head(head: &str) -> ConnectHead<'_> {
    let Some(line) = head.split("\r\n").next() else {
        return ConnectHead::Malformed;
    };
    let mut parts = line.split(' ');
    let (Some(method), Some(target), Some(version), None) =
        (parts.next(), parts.next(), parts.next(), parts.next())
    else {
        return ConnectHead::Malformed;
    };
    if !matches!(version, "HTTP/1.0" | "HTTP/1.1") || method.is_empty() || target.is_empty() {
        return ConnectHead::Malformed;
    }
    if method == "CONNECT" {
        ConnectHead::Connect { authority: target }
    } else {
        ConnectHead::Other
    }
}

async fn read_connect_head(client: &mut TcpStream) -> anyhow::Result<Option<Vec<u8>>> {
    let mut head = Vec::with_capacity(1024);
    loop {
        if head.len() >= MAX_CONNECT_HEAD_BYTES {
            anyhow::bail!("the CONNECT head exceeds 16 KiB");
        }
        let mut chunk = [0_u8; 1024];
        let read = tokio::time::timeout(CONNECT_HEAD_TIMEOUT, client.read(&mut chunk))
            .await
            .context("the CONNECT head timed out")??;
        if read == 0 {
            // A TCP probe: nothing was asked.
            return Ok(None);
        }
        head.extend_from_slice(&chunk[..read]);
        if let Some(end) = head.windows(4).position(|window| window == b"\r\n\r\n") {
            if end + 4 != head.len() {
                // TLS must wait for our answer, so the tunnel never carries
                // bytes the client sent before it was approved.
                anyhow::bail!("the client sent data before the tunnel was established");
            }
            head.truncate(end + 4);
            return Ok(Some(head));
        }
    }
}

async fn refuse(client: &mut TcpStream, status: &str, reason: &str) -> anyhow::Result<()> {
    let body = format!("campus egress refused: {reason}\n");
    client
        .write_all(
            format!(
                "HTTP/1.1 {status}\r\ncontent-type: text/plain\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            )
            .as_bytes(),
        )
        .await?;
    Ok(())
}

impl Proxy {
    async fn tunnel(&self, mut client: TcpStream, peer: SocketAddr) -> anyhow::Result<()> {
        let Some(head) = read_connect_head(&mut client).await? else {
            return Ok(());
        };
        let head = String::from_utf8_lossy(&head).into_owned();
        let authority = match parse_connect_head(&head) {
            ConnectHead::Connect { authority } => authority,
            ConnectHead::Other => {
                refuse(
                    &mut client,
                    "403 Forbidden",
                    CampusEgressDenial::Connect.code(),
                )
                .await?;
                anyhow::bail!("refused a non-CONNECT request from {peer}");
            }
            ConnectHead::Malformed => {
                refuse(&mut client, "400 Bad Request", "malformed").await?;
                anyhow::bail!("refused a malformed request from {peer}");
            }
        };
        let host = match parse_campus_connect_target(authority) {
            Ok(host) => host,
            Err(denial) => {
                refuse(&mut client, "403 Forbidden", denial.code()).await?;
                anyhow::bail!("refused CONNECT {authority:.80} from {peer}: {denial}");
            }
        };
        let session = match self.directory.lookup(peer.ip()).await {
            Ok(Some(session)) => session,
            Ok(None) => {
                let denial = CampusEgressDenial::UnknownSession;
                refuse(&mut client, "403 Forbidden", denial.code()).await?;
                anyhow::bail!("refused CONNECT {host} from {peer}: {denial}");
            }
            Err(error) => {
                refuse(&mut client, "503 Service Unavailable", "session_lookup").await?;
                return Err(error.context(format!("cannot identify the session at {peer}")));
            }
        };
        let who = format!("{}/{}", session.namespace, session.pod);
        if !session.hosts.contains(host) {
            let denial = CampusEgressDenial::Host;
            refuse(&mut client, "403 Forbidden", denial.code()).await?;
            anyhow::bail!("refused CONNECT {host} for {who}: {denial}");
        }
        let upstream = match self.dial(host).await {
            Ok(upstream) => upstream,
            Err(DialError::Denied(denial)) => {
                refuse(&mut client, "403 Forbidden", denial.code()).await?;
                anyhow::bail!("refused CONNECT {host} for {who}: {denial}");
            }
            Err(DialError::Failed(error)) => {
                refuse(&mut client, "502 Bad Gateway", "upstream").await?;
                return Err(error.context(format!("cannot reach {host} for {who}")));
            }
        };
        let (sender, connection) =
            hyper::client::conn::http1::handshake::<_, ProxyBody>(TokioIo::new(upstream))
                .await
                .context("upstream HTTP handshake failed")?;
        tokio::spawn(async move {
            let _ = connection.await;
        });
        client
            .write_all(b"HTTP/1.1 200 Connection Established\r\n\r\n")
            .await?;
        tracing::info!(session = %who, %host, "campus egress tunnel opened");
        let tls = tokio::time::timeout(TLS_TIMEOUT, self.acceptor.accept(client))
            .await
            .context("the session TLS handshake timed out")?
            .context("the session TLS handshake failed")?;
        let sender = Arc::new(Mutex::new(sender));
        let service = hyper::service::service_fn(move |request: Request<Incoming>| {
            let sender = Arc::clone(&sender);
            let who = who.clone();
            async move { Ok::<_, std::convert::Infallible>(forward(host, &who, &sender, request).await) }
        });
        let serve = hyper::server::conn::http1::Builder::new()
            .timer(TokioTimer::new())
            .header_read_timeout(REQUEST_HEAD_TIMEOUT)
            .serve_connection(TokioIo::new(tls), service);
        match tokio::time::timeout(MAX_TUNNEL_LIFETIME, serve).await {
            Ok(result) => result.context("the tunnel's HTTP connection failed"),
            Err(_) => Ok(()),
        }
    }

    async fn dial(
        &self,
        host: &'static str,
    ) -> Result<tokio_rustls::client::TlsStream<TcpStream>, DialError> {
        let resolved = tokio::time::timeout(
            RESOLVE_TIMEOUT,
            tokio::net::lookup_host((host, CAMPUS_EGRESS_PORT)),
        )
        .await
        .map_err(|_| DialError::Denied(CampusEgressDenial::Address))?
        .map_err(|_| DialError::Denied(CampusEgressDenial::Address))?;
        let addresses = select_campus_egress_addresses(resolved).map_err(DialError::Denied)?;
        let mut last_error = None;
        for address in addresses {
            match tokio::time::timeout(DIAL_TIMEOUT, TcpStream::connect(address)).await {
                Ok(Ok(stream)) => {
                    // Defence in depth: the socket is connected to what was
                    // checked, not to whatever a later lookup would say.
                    let connected = stream
                        .peer_addr()
                        .map_err(|error| DialError::Failed(error.into()))?;
                    if connected != address {
                        return Err(DialError::Denied(CampusEgressDenial::Address));
                    }
                    let name = ServerName::try_from(host)
                        .map_err(|error| DialError::Failed(error.into()))?;
                    let tls =
                        tokio::time::timeout(TLS_TIMEOUT, self.connector.connect(name, stream))
                            .await
                            .map_err(|_| {
                                DialError::Failed(anyhow::anyhow!("upstream TLS timed out"))
                            })?
                            .map_err(|error| {
                                DialError::Failed(
                                    anyhow::Error::from(error)
                                        .context("upstream TLS verification failed"),
                                )
                            })?;
                    return Ok(tls);
                }
                Ok(Err(error)) => last_error = Some(anyhow::Error::from(error)),
                Err(_) => last_error = Some(anyhow::anyhow!("connect to {address} timed out")),
            }
        }
        Err(DialError::Failed(
            last_error.unwrap_or_else(|| anyhow::anyhow!("no address to dial")),
        ))
    }
}

enum DialError {
    Denied(CampusEgressDenial),
    Failed(anyhow::Error),
}

fn header_str(headers: &HeaderMap, name: HeaderName) -> Option<&str> {
    headers.get(name).and_then(|value| value.to_str().ok())
}

/// The policy view of one request inside a tunnel.
fn egress_request<'a>(
    tunnel_host: &'a str,
    request: &'a Request<Incoming>,
    target: &'a str,
) -> CampusEgressRequest<'a> {
    egress_request_parts(
        tunnel_host,
        request.method().as_str(),
        request.headers(),
        target,
    )
}

fn egress_request_parts<'a>(
    tunnel_host: &'a str,
    method: &'a str,
    headers: &'a HeaderMap,
    target: &'a str,
) -> CampusEgressRequest<'a> {
    let upgrade = headers.contains_key(header::UPGRADE)
        || headers
            .get_all(header::CONNECTION)
            .iter()
            .filter_map(|value| value.to_str().ok())
            .flat_map(|value| value.split(','))
            .any(|token| token.trim().eq_ignore_ascii_case("upgrade"))
        || headers.contains_key("http2-settings");
    let content_length = header_str(headers, header::CONTENT_LENGTH)
        .map(|value| value.trim().parse::<u64>().unwrap_or(u64::MAX));
    CampusEgressRequest {
        tunnel_host,
        method,
        // Several Host headers are refused rather than one picked.
        host: (headers.get_all(header::HOST).iter().count() == 1)
            .then(|| header_str(headers, header::HOST))
            .flatten(),
        target,
        content_type: header_str(headers, header::CONTENT_TYPE),
        upgrade,
        content_length,
    }
}

/// Remove hop-by-hop headers, including any named in `Connection`.
fn strip_hop_by_hop(headers: &mut HeaderMap) {
    let named = headers
        .get_all(header::CONNECTION)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .flat_map(|value| value.split(','))
        .filter_map(|token| HeaderName::from_bytes(token.trim().as_bytes()).ok())
        .collect::<Vec<_>>();
    for name in named {
        headers.remove(name);
    }
    for name in HOP_BY_HOP {
        headers.remove(*name);
    }
}

fn text_response(status: StatusCode, body: String) -> Response<ProxyBody> {
    let mut response = Response::new(
        Full::new(Bytes::from(body))
            .map_err(|never| match never {})
            .boxed(),
    );
    *response.status_mut() = status;
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        header::HeaderValue::from_static("text/plain"),
    );
    response
}

async fn forward(
    host: &'static str,
    who: &str,
    sender: &Mutex<hyper::client::conn::http1::SendRequest<ProxyBody>>,
    request: Request<Incoming>,
) -> Response<ProxyBody> {
    let target = request.uri().to_string();
    let decision = check_campus_egress_request(&egress_request(host, &request, &target));
    let shown = egress_request(host, &request, &target).to_string();
    if let Err(denial) = decision {
        tracing::info!(session = %who, request = %shown, decision = denial.code(), "campus egress request refused");
        return text_response(
            StatusCode::FORBIDDEN,
            format!("campus egress refused: {}\n", denial.code()),
        );
    }
    let (mut parts, body) = request.into_parts();
    strip_hop_by_hop(&mut parts.headers);
    parts.version = hyper::Version::HTTP_11;
    let body = Limited::new(body, CAMPUS_EGRESS_MAX_REQUEST_BODY_BYTES as usize).boxed();
    let upstream_request = Request::from_parts(parts, body);
    let mut sender = sender.lock().await;
    if sender.ready().await.is_err() {
        return text_response(
            StatusCode::BAD_GATEWAY,
            "campus egress: upstream closed\n".into(),
        );
    }
    match sender.send_request(upstream_request).await {
        Ok(response) => {
            tracing::info!(session = %who, request = %shown, status = response.status().as_u16(), "campus egress request forwarded");
            let (mut parts, body) = response.into_parts();
            strip_hop_by_hop(&mut parts.headers);
            Response::from_parts(parts, body.map_err(Into::into).boxed())
        }
        Err(error) => {
            tracing::info!(session = %who, request = %shown, %error, "campus egress upstream request failed");
            text_response(
                StatusCode::BAD_GATEWAY,
                "campus egress: upstream failed\n".into(),
            )
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn only_connect_heads_open_tunnels() {
        assert_eq!(
            parse_connect_head("CONNECT pypi.org:443 HTTP/1.1\r\nHost: pypi.org:443\r\n\r\n"),
            ConnectHead::Connect {
                authority: "pypi.org:443"
            }
        );
        for other in [
            "GET http://pypi.org/ HTTP/1.1\r\n\r\n",
            "POST http://169.254.169.254/latest HTTP/1.1\r\n\r\n",
        ] {
            assert_eq!(parse_connect_head(other), ConnectHead::Other, "{other}");
        }
        for malformed in [
            "CONNECT pypi.org:443\r\n\r\n",
            "CONNECT  pypi.org:443 HTTP/1.1\r\n\r\n",
            "CONNECT pypi.org:443 HTTP/2\r\n\r\n",
            "PRI * HTTP/2.0\r\n\r\nSM\r\n\r\n",
            "\u{16}\u{3}\u{1}garbage\r\n\r\n",
        ] {
            assert_eq!(
                parse_connect_head(malformed),
                ConnectHead::Malformed,
                "{malformed:?}"
            );
        }
    }

    #[test]
    fn sessions_are_running_egress_pods_with_that_exact_address() {
        let pod = |name: &str, ip: &str, phase: &str, hosts: &str| {
            json!({
                "metadata": {
                    "name": name,
                    "labels": { CAMPUS_EGRESS_LABEL: "proxy" },
                    "annotations": { CAMPUS_EGRESS_HOSTS_ANNOTATION: hosts },
                },
                "status": { "phase": phase, "podIP": ip },
            })
        };
        let source: IpAddr = "10.42.1.50".parse().unwrap();
        let list = json!({ "items": [
            pod("a", "10.42.1.50", "Running", "github.com,pypi.org"),
            pod("old", "10.42.1.50", "Succeeded", "pypi.org"),
            pod("other", "10.42.1.51", "Running", "pypi.org"),
        ]});
        let sessions = sessions_in("campus-lab-slot-1", &list, source);
        assert_eq!(
            sessions,
            [CampusSession {
                namespace: "campus-lab-slot-1".into(),
                pod: "a".into(),
                hosts: BTreeSet::from(["github.com", "pypi.org"]),
            }]
        );
        // A forged or widened annotation admits nothing.
        for hosts in [
            "evil.example",
            "pypi.org,evil.example",
            "",
            "pypi.org,pypi.org",
        ] {
            let list = json!({ "items": [pod("a", "10.42.1.50", "Running", hosts)] });
            assert!(
                sessions_in("s", &list, source)[0].hosts.is_empty(),
                "{hosts}"
            );
        }
        let mut terminating = pod("a", "10.42.1.50", "Running", "pypi.org");
        terminating["metadata"]["deletionTimestamp"] = "2030-01-01T00:00:00Z".into();
        assert!(sessions_in("s", &json!({ "items": [terminating] }), source).is_empty());
        let mut unlabelled = pod("a", "10.42.1.50", "Running", "pypi.org");
        unlabelled["metadata"]["labels"] = json!({});
        assert!(sessions_in("s", &json!({ "items": [unlabelled] }), source).is_empty());
    }

    #[test]
    fn requests_are_judged_on_what_upstream_would_see() {
        let mut headers = HeaderMap::new();
        headers.insert(header::HOST, "github.com".parse().unwrap());
        let request = egress_request_parts(
            "github.com",
            "GET",
            &headers,
            "/o/r/info/refs?service=git-upload-pack",
        );
        assert_eq!(check_campus_egress_request(&request), Ok(()));
        let request = egress_request_parts(
            "github.com",
            "GET",
            &headers,
            "/o/r/info/refs?service=git-receive-pack",
        );
        assert_eq!(
            check_campus_egress_request(&request),
            Err(CampusEgressDenial::GitPush)
        );

        let mut upgrade = headers.clone();
        upgrade.insert(header::CONNECTION, "keep-alive, Upgrade".parse().unwrap());
        let request = egress_request_parts("github.com", "GET", &upgrade, "/");
        assert_eq!(
            check_campus_egress_request(&request),
            Err(CampusEgressDenial::Upgrade)
        );
        let mut h2c = headers.clone();
        h2c.insert("http2-settings", "AAMAAABkAAQAAP__".parse().unwrap());
        let request = egress_request_parts("github.com", "GET", &h2c, "/");
        assert_eq!(
            check_campus_egress_request(&request),
            Err(CampusEgressDenial::Upgrade)
        );

        let mut two_hosts = headers.clone();
        two_hosts.append(header::HOST, "evil.example".parse().unwrap());
        let request = egress_request_parts("github.com", "GET", &two_hosts, "/");
        assert_eq!(
            check_campus_egress_request(&request),
            Err(CampusEgressDenial::HostMismatch)
        );
        let mut other_host = HeaderMap::new();
        other_host.insert(header::HOST, "api.github.com".parse().unwrap());
        let request = egress_request_parts("github.com", "GET", &other_host, "/");
        assert_eq!(
            check_campus_egress_request(&request),
            Err(CampusEgressDenial::HostMismatch)
        );

        let mut huge = headers.clone();
        huge.insert(header::CONTENT_LENGTH, "99999999999".parse().unwrap());
        huge.insert(
            header::CONTENT_TYPE,
            "application/x-git-upload-pack-request".parse().unwrap(),
        );
        let request = egress_request_parts("github.com", "POST", &huge, "/o/r/git-upload-pack");
        assert_eq!(
            check_campus_egress_request(&request),
            Err(CampusEgressDenial::BodyTooLarge)
        );
        let mut garbage = huge.clone();
        garbage.insert(header::CONTENT_LENGTH, "12, 12".parse().unwrap());
        let request = egress_request_parts("github.com", "POST", &garbage, "/o/r/git-upload-pack");
        assert_eq!(
            check_campus_egress_request(&request),
            Err(CampusEgressDenial::BodyTooLarge)
        );

        let request = egress_request_parts("github.com", "GET", &headers, "http://evil.example/");
        assert_eq!(
            check_campus_egress_request(&request),
            Err(CampusEgressDenial::Target)
        );
    }

    #[test]
    fn hop_by_hop_headers_and_proxy_credentials_are_not_forwarded() {
        let mut headers = HeaderMap::new();
        headers.insert(
            header::CONNECTION,
            "keep-alive, x-secret-hop".parse().unwrap(),
        );
        headers.insert("x-secret-hop", "1".parse().unwrap());
        headers.insert("proxy-authorization", "Basic Zm9v".parse().unwrap());
        headers.insert(header::USER_AGENT, "pip/24".parse().unwrap());
        strip_hop_by_hop(&mut headers);
        assert_eq!(headers.len(), 1);
        assert!(headers.contains_key(header::USER_AGENT));
    }
}
