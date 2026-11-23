//! The Campus model proxy: a loopback endpoint inside a lab sandbox that
//! forwards OpenAI-compatible model requests to the Campus broker.
//!
//! Any coding harness (OpenCode, Aider, Goose, ...) points at
//! `http://127.0.0.1:4791/v1` with any API key. The proxy replaces the key
//! with the session's broker token, read from its file on every request, so
//! token rotation is invisible to the harness and no harness needs to know
//! how Campus credentials work. It holds no credential of its own: anything
//! in the sandbox can already read the token file, and the broker decides
//! what the token may do.
//!
//! Only `/v1/chat/completions` and `/v1/models` are forwarded. It speaks
//! plain HTTP/1.1 to the broker's in-cluster address, one request per
//! connection, and streams responses through unbuffered.

use std::io::{self, BufRead, BufReader, Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream, ToSocketAddrs};
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

const DEFAULT_LISTEN: &str = "127.0.0.1:4791";
const DEFAULT_TOKEN_FILE: &str = "/var/run/campus-broker/token";
const MAX_HEAD_BYTES: usize = 64 * 1024;
const MAX_BODY_BYTES: usize = 8 * 1024 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_secs(10);
const CLIENT_READ_TIMEOUT: Duration = Duration::from_secs(60);
/// The longest silence allowed in a streamed model response.
const UPSTREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(600);

/// Request headers never forwarded: hop-by-hop headers, and the ones the
/// proxy sets itself.
const DROPPED_REQUEST_HEADERS: &[&str] = &[
    "authorization",
    "connection",
    "content-length",
    "host",
    "keep-alive",
    "proxy-authorization",
    "proxy-connection",
    "te",
    "trailer",
    "transfer-encoding",
    "upgrade",
];

#[derive(Debug, Clone, PartialEq, Eq)]
struct Upstream {
    host: String,
    port: u16,
    /// Always ends with `/`.
    base_path: String,
}

#[derive(Debug, Clone)]
struct Config {
    upstream: Upstream,
    token_file: PathBuf,
}

#[derive(Debug)]
struct Request {
    method: String,
    target: String,
    headers: Vec<(String, String)>,
    body: Vec<u8>,
}

#[derive(Debug)]
struct Refusal {
    status: u16,
    reason: &'static str,
    message: &'static str,
}

impl Refusal {
    const fn new(status: u16, reason: &'static str, message: &'static str) -> Self {
        Self {
            status,
            reason,
            message,
        }
    }
}

fn parse_upstream(raw: &str) -> Result<Upstream, String> {
    let rest = raw
        .trim()
        .strip_prefix("http://")
        .ok_or_else(|| "CAMPUS_BROKER_URL must be an http:// URL".to_string())?;
    let (authority, path) = match rest.find('/') {
        Some(index) => (&rest[..index], &rest[index..]),
        None => (rest, "/"),
    };
    if authority.is_empty() || authority.contains('@') {
        return Err("CAMPUS_BROKER_URL needs a host and no credentials".into());
    }
    let (host, port) = match authority.rsplit_once(':') {
        Some((host, port)) if !host.is_empty() => (
            host.to_string(),
            port.parse::<u16>()
                .map_err(|_| "CAMPUS_BROKER_URL has an invalid port".to_string())?,
        ),
        Some(_) => return Err("CAMPUS_BROKER_URL needs a host".into()),
        None => (authority.to_string(), 80),
    };
    let mut base_path = path.split(['?', '#']).next().unwrap_or("/").to_string();
    if !base_path.ends_with('/') {
        base_path.push('/');
    }
    Ok(Upstream {
        host,
        port,
        base_path,
    })
}

/// The broker path for a harness request target, or `None` when the proxy
/// does not forward it.
fn broker_route(target: &str) -> Option<String> {
    let (path, query) = match target.split_once('?') {
        Some((path, query)) => (path, Some(query)),
        None => (target, None),
    };
    let route = match path.trim_end_matches('/') {
        "/v1/chat/completions" | "/chat/completions" => "v1/inference/chat/completions",
        "/v1/models" | "/models" => "v1/inference/models",
        _ => return None,
    };
    Some(match query {
        Some(query) => format!("{route}?{query}"),
        None => route.to_string(),
    })
}

fn read_request(reader: &mut impl BufRead) -> Result<Option<Request>, Refusal> {
    let bad = || Refusal::new(400, "Bad Request", "the request is not valid HTTP/1.1");
    let mut head_bytes = 0;
    let mut lines = Vec::new();
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line).map_err(|_| bad())?;
        if read == 0 {
            if lines.is_empty() {
                return Ok(None);
            }
            return Err(bad());
        }
        head_bytes += read;
        if head_bytes > MAX_HEAD_BYTES {
            return Err(Refusal::new(
                431,
                "Request Header Fields Too Large",
                "the request headers are too large",
            ));
        }
        let line = line.trim_end_matches(['\r', '\n']).to_string();
        if line.is_empty() {
            if lines.is_empty() {
                continue;
            }
            break;
        }
        lines.push(line);
    }
    let mut request_line = lines[0].split(' ');
    let (Some(method), Some(target), Some(version), None) = (
        request_line.next(),
        request_line.next(),
        request_line.next(),
        request_line.next(),
    ) else {
        return Err(bad());
    };
    if !version.starts_with("HTTP/1.") || !target.starts_with('/') {
        return Err(bad());
    }
    let mut headers = Vec::new();
    let mut content_length = 0usize;
    for line in &lines[1..] {
        let (name, value) = line.split_once(':').ok_or_else(bad)?;
        let name = name.trim().to_string();
        let value = value.trim().to_string();
        let lower = name.to_ascii_lowercase();
        if lower == "transfer-encoding" {
            return Err(Refusal::new(
                411,
                "Length Required",
                "send the request body with a Content-Length",
            ));
        }
        if lower == "content-length" {
            content_length = value.parse().map_err(|_| bad())?;
        }
        headers.push((name, value));
    }
    if content_length > MAX_BODY_BYTES {
        return Err(Refusal::new(
            413,
            "Payload Too Large",
            "the request body is too large",
        ));
    }
    let mut body = vec![0; content_length];
    reader.read_exact(&mut body).map_err(|_| bad())?;
    Ok(Some(Request {
        method: method.to_string(),
        target: target.to_string(),
        headers,
        body,
    }))
}

fn upstream_head(request: &Request, upstream: &Upstream, route: &str, token: &str) -> String {
    let mut head = format!(
        "{} {}{} HTTP/1.1\r\nHost: {}:{}\r\nAuthorization: Bearer {}\r\nConnection: close\r\nContent-Length: {}\r\n",
        request.method,
        upstream.base_path,
        route,
        upstream.host,
        upstream.port,
        token,
        request.body.len(),
    );
    for (name, value) in &request.headers {
        if !DROPPED_REQUEST_HEADERS.contains(&name.to_ascii_lowercase().as_str()) {
            head.push_str(&format!("{name}: {value}\r\n"));
        }
    }
    head.push_str("\r\n");
    head
}

fn write_refusal(client: &mut impl Write, refusal: &Refusal) -> io::Result<()> {
    let body = format!(
        "{{\"error\":{{\"message\":\"{}\",\"type\":\"campus_model_proxy\"}}}}",
        refusal.message
    );
    write!(
        client,
        "HTTP/1.1 {} {}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        refusal.status,
        refusal.reason,
        body.len(),
        body
    )?;
    client.flush()
}

fn read_token(config: &Config) -> Result<String, Refusal> {
    let token = std::fs::read_to_string(&config.token_file).map_err(|_| {
        Refusal::new(
            503,
            "Service Unavailable",
            "this lab has no model access token right now",
        )
    })?;
    let token = token.trim();
    if token.is_empty() || !token.chars().all(|c| c.is_ascii_graphic()) {
        return Err(Refusal::new(
            503,
            "Service Unavailable",
            "this lab has no model access token right now",
        ));
    }
    Ok(token.to_string())
}

fn connect(upstream: &Upstream) -> io::Result<TcpStream> {
    let mut last = io::Error::new(io::ErrorKind::NotFound, "the broker has no address");
    for address in (upstream.host.as_str(), upstream.port).to_socket_addrs()? {
        match TcpStream::connect_timeout(&address, CONNECT_TIMEOUT) {
            Ok(stream) => return Ok(stream),
            Err(error) => last = error,
        }
    }
    Err(last)
}

/// Copies the broker's response to the client, replacing its connection
/// headers with `Connection: close`.
fn relay_response(upstream: TcpStream, client: &mut TcpStream) -> io::Result<()> {
    let mut reader = BufReader::new(upstream);
    let mut head = String::new();
    let mut head_bytes = 0;
    let mut first = true;
    loop {
        let mut line = String::new();
        let read = reader.read_line(&mut line)?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "the broker closed the connection early",
            ));
        }
        head_bytes += read;
        if head_bytes > MAX_HEAD_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the broker response head is too large",
            ));
        }
        let trimmed = line.trim_end_matches(['\r', '\n']);
        if trimmed.is_empty() {
            break;
        }
        if !first {
            let name = trimmed
                .split_once(':')
                .map(|(name, _)| name.trim().to_ascii_lowercase())
                .unwrap_or_default();
            if name == "connection" || name == "keep-alive" {
                continue;
            }
        }
        first = false;
        head.push_str(trimmed);
        head.push_str("\r\n");
    }
    head.push_str("Connection: close\r\n\r\n");
    client.write_all(head.as_bytes())?;
    client.flush()?;
    // Unbuffered on the client side, so streamed events reach the harness
    // as soon as the broker sends them.
    let mut buffer = [0u8; 16 * 1024];
    loop {
        let read = reader.read(&mut buffer)?;
        if read == 0 {
            return Ok(());
        }
        client.write_all(&buffer[..read])?;
        client.flush()?;
    }
}

fn handle(mut client: TcpStream, config: &Config) {
    let _ = client.set_read_timeout(Some(CLIENT_READ_TIMEOUT));
    let request = {
        let mut reader = BufReader::new(&client);
        read_request(&mut reader)
    };
    let result = match request {
        Ok(None) => Ok(()),
        Err(refusal) => write_refusal(&mut client, &refusal),
        Ok(Some(request)) => serve(&mut client, &request, config),
    };
    if let Err(error) = result {
        eprintln!("campus-model-proxy: {error}");
    }
    let _ = client.shutdown(Shutdown::Both);
}

fn serve(client: &mut TcpStream, request: &Request, config: &Config) -> io::Result<()> {
    let Some(route) = broker_route(&request.target) else {
        return write_refusal(
            client,
            &Refusal::new(
                404,
                "Not Found",
                "only /v1/chat/completions and /v1/models are available",
            ),
        );
    };
    let token = match read_token(config) {
        Ok(token) => token,
        Err(refusal) => return write_refusal(client, &refusal),
    };
    let mut upstream = match connect(&config.upstream) {
        Ok(upstream) => upstream,
        Err(error) => {
            eprintln!("campus-model-proxy: broker unreachable: {error}");
            return write_refusal(
                client,
                &Refusal::new(502, "Bad Gateway", "the Campus broker is unreachable"),
            );
        }
    };
    upstream.set_read_timeout(Some(UPSTREAM_IDLE_TIMEOUT))?;
    upstream.write_all(upstream_head(request, &config.upstream, &route, &token).as_bytes())?;
    upstream.write_all(&request.body)?;
    upstream.flush()?;
    relay_response(upstream, client)
}

fn config_from_env() -> Result<(String, Config), String> {
    let upstream = parse_upstream(
        &std::env::var("CAMPUS_BROKER_URL").map_err(|_| "CAMPUS_BROKER_URL is not set")?,
    )?;
    let listen =
        std::env::var("CAMPUS_MODEL_PROXY_LISTEN").unwrap_or_else(|_| DEFAULT_LISTEN.into());
    let token_file = std::env::var("CAMPUS_BROKER_TOKEN_FILE")
        .unwrap_or_else(|_| DEFAULT_TOKEN_FILE.into())
        .into();
    Ok((
        listen,
        Config {
            upstream,
            token_file,
        },
    ))
}

fn serve_forever(listener: TcpListener, config: Arc<Config>) {
    for client in listener.incoming() {
        match client {
            Ok(client) => {
                let config = config.clone();
                std::thread::spawn(move || handle(client, &config));
            }
            Err(error) => eprintln!("campus-model-proxy: accept failed: {error}"),
        }
    }
}

fn main() {
    let (listen, config) = match config_from_env() {
        Ok(config) => config,
        Err(error) => {
            eprintln!("campus-model-proxy: {error}");
            std::process::exit(2);
        }
    };
    let listener = match TcpListener::bind(&listen) {
        Ok(listener) => listener,
        Err(error) => {
            eprintln!("campus-model-proxy: cannot listen on {listen}: {error}");
            std::process::exit(1);
        }
    };
    serve_forever(listener, Arc::new(config));
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc;

    #[test]
    fn parses_the_broker_url() {
        assert_eq!(
            parse_upstream("http://10.43.130.31:4790/").unwrap(),
            Upstream {
                host: "10.43.130.31".into(),
                port: 4790,
                base_path: "/".into()
            }
        );
        assert_eq!(
            parse_upstream("http://broker").unwrap().base_path,
            "/".to_string()
        );
        assert_eq!(parse_upstream("http://broker/x").unwrap().base_path, "/x/");
        assert!(parse_upstream("https://broker/").is_err());
        assert!(parse_upstream("http://user:pw@broker/").is_err());
        assert!(parse_upstream("http://:80/").is_err());
    }

    #[test]
    fn forwards_only_model_routes() {
        assert_eq!(
            broker_route("/v1/chat/completions").as_deref(),
            Some("v1/inference/chat/completions")
        );
        assert_eq!(
            broker_route("/v1/models?x=1").as_deref(),
            Some("v1/inference/models?x=1")
        );
        assert_eq!(broker_route("/v1/token"), None);
        assert_eq!(broker_route("/v1/inference/chat/completions"), None);
        assert_eq!(broker_route("/"), None);
    }

    #[test]
    fn refuses_chunked_and_oversized_requests() {
        let chunked = b"POST /v1/chat/completions HTTP/1.1\r\nTransfer-Encoding: chunked\r\n\r\n";
        assert_eq!(read_request(&mut &chunked[..]).unwrap_err().status, 411);
        let huge = format!(
            "POST /v1/chat/completions HTTP/1.1\r\nContent-Length: {}\r\n\r\n",
            MAX_BODY_BYTES + 1
        );
        assert_eq!(read_request(&mut huge.as_bytes()).unwrap_err().status, 413);
        assert!(read_request(&mut &b""[..]).unwrap().is_none());
    }

    #[test]
    fn replaces_the_harness_key_with_the_broker_token() {
        let request = Request {
            method: "POST".into(),
            target: "/v1/chat/completions".into(),
            headers: vec![
                ("Authorization".into(), "Bearer harness-key".into()),
                ("Content-Type".into(), "application/json".into()),
                ("Connection".into(), "keep-alive".into()),
                ("Host".into(), "127.0.0.1:4791".into()),
            ],
            body: b"{}".to_vec(),
        };
        let upstream = parse_upstream("http://broker:4790/").unwrap();
        let head = upstream_head(
            &request,
            &upstream,
            "v1/inference/chat/completions",
            "pcbt1_x",
        );
        assert!(head.starts_with("POST /v1/inference/chat/completions HTTP/1.1\r\n"));
        assert!(head.contains("Authorization: Bearer pcbt1_x\r\n"));
        assert!(!head.contains("harness-key"));
        assert!(head.contains("Content-Type: application/json\r\n"));
        assert!(head.contains("Content-Length: 2\r\n"));
        assert_eq!(head.matches("Connection:").count(), 1);
        assert_eq!(head.matches("Host:").count(), 1);
    }

    fn temp_token(contents: &str) -> PathBuf {
        let path = std::env::temp_dir().join(format!(
            "campus-model-proxy-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::write(&path, contents).unwrap();
        path
    }

    /// A broker that records one request and streams a two-part response.
    fn fake_broker() -> (u16, mpsc::Receiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let (sender, receiver) = mpsc::channel();
        std::thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut reader = BufReader::new(stream.try_clone().unwrap());
            let request = read_request(&mut reader).unwrap().unwrap();
            let auth = request
                .headers
                .iter()
                .find(|(name, _)| name == "Authorization")
                .map(|(_, value)| value.clone())
                .unwrap_or_default();
            sender
                .send(format!(
                    "{} {} {} {}",
                    request.method,
                    request.target,
                    auth,
                    String::from_utf8(request.body).unwrap()
                ))
                .unwrap();
            stream
                .write_all(b"HTTP/1.1 200 OK\r\nContent-Type: text/event-stream\r\nConnection: keep-alive\r\n\r\ndata: one\n\n")
                .unwrap();
            stream.flush().unwrap();
            std::thread::sleep(Duration::from_millis(50));
            stream.write_all(b"data: [DONE]\n\n").unwrap();
        });
        (port, receiver)
    }

    #[test]
    fn streams_a_model_response_with_the_current_token() {
        let (port, received) = fake_broker();
        let token_file = temp_token("pcbt1_current\n");
        let config = Arc::new(Config {
            upstream: parse_upstream(&format!("http://127.0.0.1:{port}/")).unwrap(),
            token_file: token_file.clone(),
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy = listener.local_addr().unwrap();
        std::thread::spawn(move || serve_forever(listener, config));

        let mut client = TcpStream::connect(proxy).unwrap();
        client
            .write_all(b"POST /v1/chat/completions HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer anything\r\nContent-Length: 2\r\n\r\n{}")
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        std::fs::remove_file(token_file).unwrap();

        assert_eq!(
            received.recv().unwrap(),
            "POST /v1/inference/chat/completions Bearer pcbt1_current {}"
        );
        assert!(response.starts_with("HTTP/1.1 200 OK\r\n"));
        assert!(response.contains("Connection: close\r\n"));
        assert!(!response.contains("keep-alive"));
        assert!(response.ends_with("data: one\n\ndata: [DONE]\n\n"));
    }

    #[test]
    fn says_so_when_the_lab_has_no_token() {
        let config = Arc::new(Config {
            upstream: parse_upstream("http://127.0.0.1:9/").unwrap(),
            token_file: std::env::temp_dir().join("campus-model-proxy-missing-token"),
        });
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let proxy = listener.local_addr().unwrap();
        std::thread::spawn(move || serve_forever(listener, config));
        let mut client = TcpStream::connect(proxy).unwrap();
        client
            .write_all(b"POST /v1/chat/completions HTTP/1.1\r\nContent-Length: 0\r\n\r\n")
            .unwrap();
        let mut response = String::new();
        client.read_to_string(&mut response).unwrap();
        assert!(response.starts_with("HTTP/1.1 503 "));
        assert!(response.contains("no model access token"));
    }
}
