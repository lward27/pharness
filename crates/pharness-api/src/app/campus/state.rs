use super::config::CampusConfig;
use super::login_cookie::LoginCookieKey;
use super::oidc::{IdTokenValidator, OidcHttp, ReqwestOidcHttp};
use pharness_store::SqliteStore;
use ring::rand::SystemRandom;
use secrecy::ExposeSecret;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Seconds since the Unix epoch. Injected so session and token expiry can be
/// tested without waiting.
pub(crate) trait CampusClock: Send + Sync {
    fn now(&self) -> i64;
}

pub(crate) struct SystemClock;

impl CampusClock for SystemClock {
    fn now(&self) -> i64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map(|elapsed| i64::try_from(elapsed.as_secs()).unwrap_or(i64::MAX))
            .unwrap_or_default()
    }
}

pub(crate) struct CampusRuntime {
    pub(crate) config: CampusConfig,
    pub(crate) http: Arc<dyn OidcHttp>,
    pub(crate) validator: IdTokenValidator,
    pub(crate) clock: Arc<dyn CampusClock>,
    pub(crate) rng: SystemRandom,
    pub(crate) login_cookie_key: LoginCookieKey,
}

impl CampusRuntime {
    pub(crate) fn production(config: CampusConfig) -> anyhow::Result<Self> {
        let http: Arc<dyn OidcHttp> = Arc::new(ReqwestOidcHttp::new(&config)?);
        Ok(Self::new(config, http, Arc::new(SystemClock)))
    }

    pub(crate) fn new(
        config: CampusConfig,
        http: Arc<dyn OidcHttp>,
        clock: Arc<dyn CampusClock>,
    ) -> Self {
        let validator = IdTokenValidator::new(&config, http.clone());
        let login_cookie_key = LoginCookieKey::new(config.login_cookie_key.expose_secret());
        Self {
            config,
            http,
            validator,
            clock,
            rng: SystemRandom::new(),
            login_cookie_key,
        }
    }
}

/// How the Campus boundary is mounted. Anything other than `Enabled` answers
/// every `/api/campus/*` path with an error and never creates a session.
pub(crate) enum CampusMount {
    Disabled,
    Unavailable(&'static str),
    Enabled(Box<CampusRuntime>),
}

#[derive(Clone)]
pub(crate) struct CampusState {
    pub(crate) store: Arc<SqliteStore>,
    pub(crate) runtime: Arc<CampusRuntime>,
}
