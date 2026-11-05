//! In-process Campus test harness: a mock IdP with runtime-generated keys,
//! an injectable clock and the full application router.
//!
//! No private key material is committed; every signing key is generated when
//! a test runs.

mod broker;
mod broker_live;
mod config;
mod flow;
mod id_token;
mod lab_provider;
mod labs;
mod login_cookie;
mod routes;
mod teardown;
mod teardown_live;
mod terminal;

use super::config::CampusConfig;
use super::lab_catalog::CampusLabCatalog;
use super::labs::{CampusLabSweep, CampusLabs};
use super::login_cookie::LOGIN_COOKIE_KEY_BYTES;
use super::oidc::{IdTokenResponse, OidcHttp, OidcHttpError, TokenExchange};
use super::state::{CampusClock, CampusMount, CampusRuntime};
use super::terminal_registry::CampusTerminals;
use super::tokens::{pkce_challenge, sha256_hex};
use async_trait::async_trait;
use axum::body::Body;
use axum::http::{header, HeaderMap, Request, StatusCode};
use axum::response::Response;
use axum::Router;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use jsonwebtoken::Algorithm;
use pharness_core::campus_broker_token::CampusBrokerTokenIssuer;
use pharness_core::FakeCampusProvider;
use pharness_store::SqliteStore;
use ring::rand::{SecureRandom, SystemRandom};
use ring::signature::{EcdsaKeyPair, KeyPair, ECDSA_P256_SHA256_FIXED_SIGNING};
use secrecy::{Secret, SecretString};
use serde_json::{json, Value};
use sqlx::{Connection, Row};
use std::collections::{BTreeMap, HashMap};
use std::path::PathBuf;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use tower::ServiceExt;
use url::Url;

pub(super) const ISSUER: &str = "https://idp.campus.test/realms/pilot";
pub(super) const CLIENT_ID: &str = "pharness-campus";
pub(super) const CLIENT_SECRET: &str = "campus-client-secret";
pub(super) const ORIGIN: &str = "https://campus.pharness.test";
pub(super) const TENANT: &str = "tenant-a";
pub(super) const OPERATOR_TOKEN: &str = "operator-secret";
pub(super) const WORKER_TOKEN: &str = "worker-secret";
pub(super) const START: i64 = 1_900_000_000;

pub(super) struct TestClock(AtomicI64);

impl TestClock {
    pub(super) fn advance(&self, seconds: i64) {
        self.0.fetch_add(seconds, Ordering::SeqCst);
    }
}

impl CampusClock for TestClock {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

pub(super) fn random_login_cookie_key() -> [u8; LOGIN_COOKIE_KEY_BYTES] {
    let mut key = [0_u8; LOGIN_COOKIE_KEY_BYTES];
    SystemRandom::new().fill(&mut key).unwrap();
    key
}

pub(super) fn test_config() -> CampusConfig {
    CampusConfig {
        tenant_id: TENANT.into(),
        public_origin: ORIGIN.into(),
        issuer: ISSUER.into(),
        client_id: CLIENT_ID.into(),
        client_secret: SecretString::new(CLIENT_SECRET.into()),
        login_cookie_key: Secret::new(random_login_cookie_key()),
        authorization_endpoint: Url::parse("https://idp.campus.test/authorize?kc_idp_hint=campus")
            .unwrap(),
        token_endpoint: Url::parse("https://idp.campus.test/token").unwrap(),
        jwks_uri: Url::parse("https://idp.campus.test/jwks").unwrap(),
        redirect_uri: Url::parse(&format!("{ORIGIN}/api/campus/auth/callback")).unwrap(),
        scopes: "openid email profile".into(),
        allowed_algorithms: vec![Algorithm::RS256, Algorithm::ES256],
        clock_skew_seconds: 60,
        session_absolute_ttl_seconds: 8 * 60 * 60,
        session_idle_ttl_seconds: 30 * 60,
        bootstrap_admins: Vec::new(),
        post_login_path: "/".into(),
        allow_insecure_loopback_http: false,
    }
}

pub(super) fn b64(bytes: &[u8]) -> String {
    URL_SAFE_NO_PAD.encode(bytes)
}

/// Assemble a compact JWS from a header, claims and a signing function.
pub(super) fn compact_jws(
    header: &Value,
    claims: &Value,
    sign: impl Fn(&[u8]) -> Vec<u8>,
) -> String {
    let signing_input = format!(
        "{}.{}",
        b64(header.to_string().as_bytes()),
        b64(claims.to_string().as_bytes())
    );
    let signature = sign(signing_input.as_bytes());
    format!("{signing_input}.{}", b64(&signature))
}

/// A P-256 key generated at test time.
pub(super) struct EcKey {
    pub(super) kid: String,
    pair: EcdsaKeyPair,
    rng: SystemRandom,
}

impl EcKey {
    pub(super) fn generate(kid: &str) -> Self {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, &rng).unwrap();
        let pair = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_FIXED_SIGNING, pkcs8.as_ref(), &rng)
            .unwrap();
        Self {
            kid: kid.into(),
            pair,
            rng,
        }
    }

    pub(super) fn jwk(&self) -> Value {
        let point = self.pair.public_key().as_ref();
        assert_eq!(point.len(), 65, "uncompressed P-256 point");
        json!({
            "kty": "EC",
            "crv": "P-256",
            "use": "sig",
            "alg": "ES256",
            "kid": self.kid,
            "x": b64(&point[1..33]),
            "y": b64(&point[33..65]),
        })
    }

    pub(super) fn sign_raw(&self, message: &[u8]) -> Vec<u8> {
        self.pair
            .sign(&self.rng, message)
            .unwrap()
            .as_ref()
            .to_vec()
    }

    pub(super) fn sign(&self, claims: &Value) -> String {
        self.sign_with_header(
            &json!({"alg": "ES256", "typ": "JWT", "kid": self.kid}),
            claims,
        )
    }

    pub(super) fn sign_with_header(&self, header: &Value, claims: &Value) -> String {
        compact_jws(header, claims, |message| self.sign_raw(message))
    }
}

pub(super) fn base_claims(subject: &str, nonce: &str, now: i64) -> Value {
    json!({
        "iss": ISSUER,
        "aud": CLIENT_ID,
        "sub": subject,
        "exp": now + 300,
        "iat": now,
        "nonce": nonce,
        "email": format!("{subject}@campus.test"),
        "name": format!("User {subject}"),
    })
}

/// In-process IdP. Codes are single-use and redeem only with the PKCE
/// verifier whose S256 challenge was presented at authorization time.
#[derive(Default)]
pub(super) struct MockIdp {
    jwks: Mutex<Value>,
    pub(super) jwks_fetches: AtomicUsize,
    codes: Mutex<HashMap<String, (String, String)>>,
    pub(super) exchanges: Mutex<Vec<(String, String, String)>>,
}

impl MockIdp {
    pub(super) fn with_keys(keys: &[Value]) -> Arc<Self> {
        let idp = Arc::new(Self::default());
        idp.set_keys(keys);
        idp
    }

    pub(super) fn set_keys(&self, keys: &[Value]) {
        *self.jwks.lock().unwrap() = json!({ "keys": keys });
    }

    pub(super) fn set_raw_jwks(&self, jwks: Value) {
        *self.jwks.lock().unwrap() = jwks;
    }

    pub(super) fn fetches(&self) -> usize {
        self.jwks_fetches.load(Ordering::SeqCst)
    }

    pub(super) fn issue_code(&self, code: &str, challenge: &str, id_token: String) {
        self.codes
            .lock()
            .unwrap()
            .insert(code.into(), (challenge.into(), id_token));
    }
}

#[async_trait]
impl OidcHttp for MockIdp {
    async fn fetch_jwks(&self) -> Result<Vec<u8>, OidcHttpError> {
        self.jwks_fetches.fetch_add(1, Ordering::SeqCst);
        Ok(self.jwks.lock().unwrap().to_string().into_bytes())
    }

    async fn exchange_code(
        &self,
        request: TokenExchange<'_>,
    ) -> Result<IdTokenResponse, OidcHttpError> {
        self.exchanges.lock().unwrap().push((
            request.code.into(),
            request.code_verifier.into(),
            request.redirect_uri.into(),
        ));
        let Some((challenge, id_token)) = self.codes.lock().unwrap().remove(request.code) else {
            return Err(OidcHttpError::Rejected);
        };
        if pkce_challenge(request.code_verifier) != challenge {
            return Err(OidcHttpError::Rejected);
        }
        Ok(IdTokenResponse { id_token })
    }
}

pub(super) struct LoginStart {
    pub(super) state: String,
    pub(super) nonce: String,
    pub(super) challenge: String,
    pub(super) login_cookie: String,
    pub(super) location: Url,
}

#[derive(Clone)]
pub(super) struct Session {
    /// `name=value` for a `Cookie` header.
    pub(super) cookie: String,
    pub(super) csrf: String,
}

/// A temporary on-disk database, removed on drop, that a second connection
/// can inspect while the application uses it.
pub(super) struct DiskDatabase(PathBuf);

impl Drop for DiskDatabase {
    fn drop(&mut self) {
        for suffix in ["", "-wal", "-shm"] {
            let _ = std::fs::remove_file(format!("{}{suffix}", self.0.display()));
        }
    }
}

fn disk_path() -> PathBuf {
    std::env::temp_dir().join(format!(
        "pharness-campus-api-{}-{}.db",
        std::process::id(),
        uuid::Uuid::now_v7().simple()
    ))
}

pub(super) struct Harness {
    pub(super) app: Router,
    /// The broker listener's router.
    pub(super) broker: Router,
    /// The issuer a real execution provider would use for this harness.
    pub(super) broker_tokens: Arc<dyn CampusBrokerTokenIssuer>,
    pub(super) store: Arc<SqliteStore>,
    pub(super) idp: Arc<MockIdp>,
    pub(super) clock: Arc<TestClock>,
    pub(super) key: EcKey,
    pub(super) config: CampusConfig,
    pub(super) labs: Option<Arc<CampusLabs>>,
    pub(super) provider: Arc<FakeCampusProvider>,
    disk: Option<DiskDatabase>,
    next_code: AtomicUsize,
}

pub(super) fn full_app(store: Arc<SqliteStore>, campus: CampusMount) -> Router {
    full_routers(
        store,
        campus,
        Some(WORKER_TOKEN.to_string()),
        vec![("lucas".to_string(), OPERATOR_TOKEN.to_string())],
    )
    .app
}

pub(super) fn full_routers(
    store: Arc<SqliteStore>,
    campus: CampusMount,
    worker_token: Option<String>,
    operator_tokens: Vec<(String, String)>,
) -> crate::app::AppRouters {
    crate::app::routers_with_campus(
        store,
        crate::dispatch::RunDispatcher::Disabled,
        pharness_core::ReadOnlyClusterTools::default(),
        pharness_core::SafetyPolicy::default(),
        worker_token,
        operator_tokens,
        crate::workspace::WorkspaceProvisioner::new(std::env::temp_dir(), Vec::new()),
        pharness_config::InferenceGatewayConfig::legacy_default(),
        pharness_config::AgentExecutionBackendConfig::disabled_default(),
        pharness_core::hosted_sdlc::HostedWorkflowConfig::default(),
        campus,
    )
}

pub(super) async fn send(app: &Router, request: Request<Body>) -> Response {
    app.clone().oneshot(request).await.unwrap()
}

pub(super) async fn body_json(response: Response) -> Value {
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    serde_json::from_slice(&bytes).unwrap_or(Value::Null)
}

pub(super) async fn body_text(response: Response) -> String {
    let bytes = axum::body::to_bytes(response.into_body(), 1024 * 1024)
        .await
        .unwrap();
    String::from_utf8_lossy(&bytes).into_owned()
}

/// Full `Set-Cookie` header values for one cookie name.
pub(super) fn set_cookies(headers: &HeaderMap, name: &str) -> Vec<String> {
    headers
        .get_all(header::SET_COOKIE)
        .iter()
        .filter_map(|value| value.to_str().ok())
        .filter(|value| value.starts_with(&format!("{name}=")))
        .map(str::to_string)
        .collect()
}

/// The `name=value` pair of a cookie that was set with a non-empty value.
pub(super) fn issued_cookie(headers: &HeaderMap, name: &str) -> Option<String> {
    set_cookies(headers, name)
        .into_iter()
        .filter_map(|value| value.split(';').next().map(str::to_string))
        .find(|pair| pair.len() > name.len() + 1)
}

pub(super) fn request(method: &str, path: &str) -> axum::http::request::Builder {
    Request::builder().method(method).uri(path)
}

impl Harness {
    pub(super) async fn new() -> Self {
        Self::with_config(test_config()).await
    }

    pub(super) async fn with_config(config: CampusConfig) -> Self {
        let store = SqliteStore::connect_in_memory().await.unwrap();
        Self::build(config, store, None, None, None)
    }

    /// A harness whose labs run on a [`FakeCampusProvider`]; the reconciler
    /// runs only when a test calls [`Self::reconcile`]. Without
    /// `with_provider` the catalog is loaded but nothing can launch.
    pub(super) async fn with_labs(
        catalog: CampusLabCatalog,
        with_provider: bool,
        on_disk: bool,
    ) -> Self {
        Self::with_labs_and_terminals(catalog, with_provider, on_disk, None).await
    }

    /// [`Self::with_labs`] with browser terminals.
    pub(super) async fn with_labs_and_terminals(
        catalog: CampusLabCatalog,
        with_provider: bool,
        on_disk: bool,
        terminals: Option<Arc<CampusTerminals>>,
    ) -> Self {
        let (store, disk) = if on_disk {
            let path = disk_path();
            (
                SqliteStore::connect(&path).await.unwrap(),
                Some(DiskDatabase(path)),
            )
        } else {
            (SqliteStore::connect_in_memory().await.unwrap(), None)
        };
        let provider = Arc::new(FakeCampusProvider::new());
        let labs = Arc::new(CampusLabs::new(
            catalog,
            with_provider
                .then(|| provider.clone() as Arc<dyn pharness_core::CampusExecutionProvider>),
        ));
        labs.register_slots(&store, START).await.unwrap();
        let mut harness = Self::build(test_config(), store, disk, Some(labs), terminals);
        harness.provider = provider;
        harness
    }

    /// One reconciler pass at the current test time.
    pub(super) async fn reconcile(&self) -> CampusLabSweep {
        self.labs
            .as_ref()
            .expect("reconcile needs Harness::with_labs")
            .reconcile(&self.store, self.now())
            .await
    }

    /// A harness backed by a file database so that [`Self::row_counts`]
    /// can observe every table.
    pub(super) async fn on_disk() -> Self {
        let path = disk_path();
        let store = SqliteStore::connect(&path).await.unwrap();
        Self::build(test_config(), store, Some(DiskDatabase(path)), None, None)
    }

    fn runtime_for(
        config: &CampusConfig,
        idp: &Arc<MockIdp>,
        clock: &Arc<TestClock>,
        labs: Option<&Arc<CampusLabs>>,
        terminals: Option<&Arc<CampusTerminals>>,
    ) -> CampusRuntime {
        let mut runtime = CampusRuntime::new(config.clone(), idp.clone(), clock.clone());
        if let Some(labs) = labs {
            runtime = runtime.with_labs(labs.clone());
        }
        if let Some(terminals) = terminals {
            runtime = runtime.with_terminals(terminals.clone());
        }
        runtime
    }

    /// What another API process sharing this harness's IdP, clock and lab
    /// catalog needs to serve the same Campus.
    pub(super) fn process_parts(&self) -> ProcessParts {
        ProcessParts {
            config: self.config.clone(),
            idp: self.idp.clone(),
            clock: self.clock.clone(),
            labs: self.labs.clone(),
        }
    }

    pub(super) fn disk_path(&self) -> Option<PathBuf> {
        self.disk.as_ref().map(|disk| disk.0.clone())
    }

    fn build(
        config: CampusConfig,
        store: SqliteStore,
        disk: Option<DiskDatabase>,
        labs: Option<Arc<CampusLabs>>,
        terminals: Option<Arc<CampusTerminals>>,
    ) -> Self {
        let store = Arc::new(store);
        let key = EcKey::generate("campus-key-1");
        let idp = MockIdp::with_keys(&[key.jwk()]);
        let clock = Arc::new(TestClock(AtomicI64::new(START)));
        let runtime = Self::runtime_for(&config, &idp, &clock, labs.as_ref(), terminals.as_ref());
        let broker_tokens = runtime.broker_token_issuer(&store, "test-provider");
        let routers = full_routers(
            store.clone(),
            CampusMount::Enabled(Box::new(runtime)),
            Some(WORKER_TOKEN.to_string()),
            vec![("lucas".to_string(), OPERATOR_TOKEN.to_string())],
        );
        Self {
            app: routers.app,
            broker: routers.campus_broker,
            broker_tokens,
            store,
            idp,
            clock,
            key,
            config,
            labs,
            provider: Arc::new(FakeCampusProvider::new()),
            disk,
            next_code: AtomicUsize::new(1),
        }
    }

    /// Row count of every table, read through a separate connection.
    pub(super) async fn row_counts(&self) -> BTreeMap<String, i64> {
        let disk = self
            .disk
            .as_ref()
            .expect("row_counts needs Harness::on_disk");
        let mut connection =
            sqlx::SqliteConnection::connect(&format!("sqlite://{}?mode=ro", disk.0.display()))
                .await
                .unwrap();
        let tables: Vec<String> = sqlx::query_scalar(
            "SELECT name FROM sqlite_master WHERE type = 'table' AND name NOT LIKE 'sqlite_%'",
        )
        .fetch_all(&mut connection)
        .await
        .unwrap();
        let mut counts = BTreeMap::new();
        for table in tables {
            let row = sqlx::query(&format!("SELECT COUNT(*) AS count FROM \"{table}\""))
                .fetch_one(&mut connection)
                .await
                .unwrap();
            counts.insert(table, row.get::<i64, _>("count"));
        }
        assert!(counts.len() > 50, "every table was counted");
        counts
    }

    /// Seal a login cookie with the given key, tenant and issuer.
    pub(super) fn seal_login_cookie(
        &self,
        key: &[u8; LOGIN_COOKIE_KEY_BYTES],
        tenant_id: &str,
        issuer: &str,
        attempt: &super::login_cookie::LoginAttempt,
    ) -> String {
        let sealed = super::login_cookie::LoginCookieKey::new(key)
            .seal(&SystemRandom::new(), tenant_id, issuer, attempt)
            .unwrap();
        format!("{}={sealed}", super::session::LOGIN_COOKIE)
    }

    pub(super) fn now(&self) -> i64 {
        self.clock.now()
    }

    pub(super) async fn send(&self, request: Request<Body>) -> Response {
        send(&self.app, request).await
    }

    pub(super) async fn get(&self, path: &str, session: Option<&Session>) -> Response {
        let mut builder = request("GET", path);
        if let Some(session) = session {
            builder = builder.header(header::COOKIE, &session.cookie);
        }
        self.send(builder.body(Body::empty()).unwrap()).await
    }

    /// A state-changing JSON request carrying the session cookie, its CSRF
    /// token and the campus origin.
    pub(super) async fn mutate(
        &self,
        method: &str,
        path: &str,
        session: &Session,
        body: Value,
    ) -> Response {
        self.send(
            request(method, path)
                .header(header::COOKIE, &session.cookie)
                .header("x-campus-csrf", &session.csrf)
                .header(header::ORIGIN, ORIGIN)
                .header(header::CONTENT_TYPE, "application/json")
                .body(Body::from(body.to_string()))
                .unwrap(),
        )
        .await
    }

    pub(super) async fn start_login(&self) -> LoginStart {
        let response = self.get("/api/campus/auth/login", None).await;
        assert_eq!(response.status(), StatusCode::FOUND);
        let location = Url::parse(response.headers()[header::LOCATION].to_str().unwrap()).unwrap();
        let query = location
            .query_pairs()
            .into_owned()
            .collect::<HashMap<_, _>>();
        LoginStart {
            state: query["state"].clone(),
            nonce: query["nonce"].clone(),
            challenge: query["code_challenge"].clone(),
            login_cookie: issued_cookie(response.headers(), super::session::LOGIN_COOKIE)
                .expect("login cookie"),
            location,
        }
    }

    pub(super) fn code_for(&self, start: &LoginStart, id_token: String) -> String {
        let code = format!("code-{}", self.next_code.fetch_add(1, Ordering::SeqCst));
        self.idp.issue_code(&code, &start.challenge, id_token);
        code
    }

    pub(super) async fn callback(
        &self,
        state: &str,
        code: &str,
        cookies: Option<&str>,
    ) -> Response {
        let mut builder = request(
            "GET",
            &format!("/api/campus/auth/callback?code={code}&state={state}&session_state=ignored"),
        );
        if let Some(cookies) = cookies {
            builder = builder.header(header::COOKIE, cookies);
        }
        self.send(builder.body(Body::empty()).unwrap()).await
    }

    /// Run a login whose id_token claims are produced by `claims`.
    pub(super) async fn login_response(
        &self,
        claims: impl FnOnce(&LoginStart) -> Value,
    ) -> Response {
        let start = self.start_login().await;
        let token = self.key.sign(&claims(&start));
        let code = self.code_for(&start, token);
        self.callback(&start.state, &code, Some(&start.login_cookie))
            .await
    }

    pub(super) async fn session_from(&self, response: Response) -> Session {
        assert_eq!(response.status(), StatusCode::SEE_OTHER);
        let cookie = issued_cookie(response.headers(), super::session::SESSION_COOKIE)
            .expect("session cookie");
        let mut session = Session {
            cookie,
            csrf: String::new(),
        };
        let me = self.get("/api/campus/me", Some(&session)).await;
        assert_eq!(me.status(), StatusCode::OK);
        session.csrf = body_json(me).await["csrf_token"]
            .as_str()
            .unwrap()
            .to_string();
        session
    }

    pub(super) async fn login_with(&self, subject: &str, email: &str) -> Session {
        let now = self.now();
        let response = self
            .login_response(|start| {
                let mut claims = base_claims(subject, &start.nonce, now);
                claims["email"] = json!(email);
                claims
            })
            .await;
        self.session_from(response).await
    }

    pub(super) async fn login(&self, subject: &str) -> Session {
        self.login_with(subject, &format!("{subject}@campus.test"))
            .await
    }

    pub(super) async fn make_admins(&self, tenant: &str, subjects: &[&str]) {
        let admins = subjects
            .iter()
            .map(|subject| (ISSUER.to_string(), subject.to_string()))
            .collect::<Vec<_>>();
        self.store
            .sync_campus_bootstrap_admins(tenant, &admins, self.now())
            .await
            .unwrap();
    }

    pub(super) async fn enroll(&self, tenant: &str, course: &str, subject: &str, role: &str) {
        self.store
            .put_campus_course_membership(pharness_store::PutCampusMembership {
                key: pharness_store::CampusIdentityKey {
                    tenant_id: tenant.into(),
                    issuer: ISSUER.into(),
                    subject: subject.into(),
                },
                course_id: course.into(),
                role: pharness_store::CampusCourseRole::parse(role).unwrap(),
                granted_by_identity_id: "test".into(),
                now: self.now(),
            })
            .await
            .unwrap();
    }
}

#[derive(Clone)]
pub(super) struct ProcessParts {
    config: CampusConfig,
    idp: Arc<MockIdp>,
    clock: Arc<TestClock>,
    labs: Option<Arc<CampusLabs>>,
}

impl ProcessParts {
    /// The application of that process on `store`, with its own terminal
    /// registry.
    pub(super) fn app(&self, store: Arc<SqliteStore>, terminals: Arc<CampusTerminals>) -> Router {
        let runtime = Harness::runtime_for(
            &self.config,
            &self.idp,
            &self.clock,
            self.labs.as_ref(),
            Some(&terminals),
        );
        full_app(store, CampusMount::Enabled(Box::new(runtime)))
    }
}

pub(super) fn session_hash(session: &Session) -> String {
    sha256_hex(session.cookie.split_once('=').unwrap().1)
}
