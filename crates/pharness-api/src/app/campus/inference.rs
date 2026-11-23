//! Real model inference behind the Campus broker: OpenAI-compatible chat
//! completions forwarded to one model provider.
//!
//! The provider key lives here, in the control plane, and never enters a
//! sandbox. A lab names a Campus model alias (`campus-default`); the broker
//! maps it to the provider's model, caps the output tokens and streams the
//! provider's answer back. Without this configuration the broker keeps
//! serving its deterministic fake model.
//!
//! Any OpenAI-compatible provider works: Fireworks for the proof of
//! concept, or a model the institution hosts.

use super::error::CampusError;
use anyhow::{bail, Context};
use axum::body::Body;
use axum::http::{header, HeaderValue, StatusCode};
use axum::response::Response;
use secrecy::{ExposeSecret, SecretString};
use serde_json::{json, Map, Value};
use std::collections::BTreeMap;
use std::path::Path;
use std::time::Duration;
use url::Url;

pub(crate) const ENV_BASE_URL: &str = "PHARNESS_CAMPUS_INFERENCE_BASE_URL";
pub(crate) const ENV_API_KEY_FILE: &str = "PHARNESS_CAMPUS_INFERENCE_API_KEY_FILE";
pub(crate) const ENV_MODELS: &str = "PHARNESS_CAMPUS_INFERENCE_MODELS";
pub(crate) const ENV_MAX_OUTPUT_TOKENS: &str = "PHARNESS_CAMPUS_INFERENCE_MAX_OUTPUT_TOKENS";

/// The alias used when a request names no model.
pub(crate) const DEFAULT_MODEL_ALIAS: &str = "campus-default";
const DEFAULT_MAX_OUTPUT_TOKENS: u64 = 8192;
/// The most a lab may send in one request: an agent's whole conversation.
pub(crate) const MAX_REQUEST_BYTES: usize = 8 * 1024 * 1024;
const MAX_MESSAGES: usize = 4096;
const OUTPUT_TOKEN_FIELDS: &[&str] = &["max_tokens", "max_completion_tokens"];

pub(crate) struct CampusInference {
    chat_url: Url,
    api_key: SecretString,
    /// Campus model alias to the provider's model id.
    models: BTreeMap<String, String>,
    max_output_tokens: u64,
    client: reqwest::Client,
}

impl std::fmt::Debug for CampusInference {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CampusInference")
            .field("chat_url", &self.chat_url.as_str())
            .field("models", &self.models)
            .field("max_output_tokens", &self.max_output_tokens)
            .finish_non_exhaustive()
    }
}

fn is_alias(alias: &str) -> bool {
    !alias.is_empty()
        && alias.len() <= 64
        && alias
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '.')
}

impl CampusInference {
    pub(crate) fn new(
        base_url: &str,
        api_key: SecretString,
        models: BTreeMap<String, String>,
        max_output_tokens: u64,
    ) -> anyhow::Result<Self> {
        let mut base = Url::parse(base_url.trim()).context("the inference base URL is invalid")?;
        let loopback = matches!(base.host_str(), Some("127.0.0.1" | "localhost" | "[::1]"));
        if base.scheme() != "https" && !(base.scheme() == "http" && loopback) {
            bail!("the inference base URL must use https");
        }
        if !base.username().is_empty() || base.password().is_some() || base.query().is_some() {
            bail!("the inference base URL must carry no credentials or query");
        }
        if !base.path().ends_with('/') {
            let path = format!("{}/", base.path());
            base.set_path(&path);
        }
        if api_key.expose_secret().trim().is_empty() {
            bail!("the inference API key is empty");
        }
        if models.is_empty() {
            bail!("at least one Campus model alias is required");
        }
        for (alias, model) in &models {
            if !is_alias(alias) {
                bail!("model alias {alias:?} must be lowercase letters, digits, '-' or '.'");
            }
            if model.trim().is_empty() {
                bail!("model alias {alias} names no provider model");
            }
        }
        if max_output_tokens == 0 {
            bail!("the output token cap must be positive");
        }
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(10))
            .read_timeout(Duration::from_secs(300))
            .build()
            .context("failed to build the inference HTTP client")?;
        Ok(Self {
            chat_url: base.join("chat/completions")?,
            api_key,
            models,
            max_output_tokens,
            client,
        })
    }

    pub(crate) fn aliases(&self) -> Vec<String> {
        self.models.keys().cloned().collect()
    }

    /// The provider request for a lab's request: its alias mapped to the
    /// provider model and its output tokens capped. Returns the alias too.
    pub(crate) fn prepare(&self, request: Value) -> Result<(String, Value), CampusError> {
        let Value::Object(mut body) = request else {
            return Err(CampusError::bad_request(
                "the request body must be a chat completion request",
            ));
        };
        match body.get("messages") {
            Some(Value::Array(messages)) if !messages.is_empty() => {
                if messages.len() > MAX_MESSAGES {
                    return Err(CampusError::bad_request(
                        "a chat completion has too many messages",
                    ));
                }
            }
            _ => {
                return Err(CampusError::bad_request(
                    "a chat completion needs at least one message",
                ))
            }
        }
        let alias = match body.get("model") {
            None | Some(Value::Null) => DEFAULT_MODEL_ALIAS.to_string(),
            Some(Value::String(alias)) => alias.clone(),
            Some(_) => return Err(CampusError::bad_request("the model must be a string")),
        };
        let Some(model) = self.models.get(&alias) else {
            return Err(
                CampusError::bad_request("this model is not available in Campus labs")
                    .with_code("model_not_allowed"),
            );
        };
        body.insert("model".into(), Value::String(model.clone()));
        cap_output_tokens(&mut body, self.max_output_tokens);
        Ok((alias, Value::Object(body)))
    }

    /// Sends a prepared request and streams the provider's answer back,
    /// status and all.
    pub(crate) async fn forward(&self, body: Value) -> Result<Response, CampusError> {
        let upstream = self
            .client
            .post(self.chat_url.clone())
            .bearer_auth(self.api_key.expose_secret())
            .json(&body)
            .send()
            .await
            .map_err(|error| {
                tracing::warn!(error = %error.without_url(), "campus inference provider unreachable");
                CampusError::bad_gateway("the model provider is unreachable")
            })?;
        let status =
            StatusCode::from_u16(upstream.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let content_type = upstream
            .headers()
            .get(header::CONTENT_TYPE)
            .and_then(|value| HeaderValue::from_bytes(value.as_bytes()).ok())
            .unwrap_or(HeaderValue::from_static("application/json"));
        let mut response = Response::new(Body::from_stream(upstream.bytes_stream()));
        *response.status_mut() = status;
        response
            .headers_mut()
            .insert(header::CONTENT_TYPE, content_type);
        Ok(response)
    }

    /// The OpenAI-style model list a harness can read.
    pub(crate) fn models_view(&self) -> Value {
        models_view(self.models.keys().map(String::as_str))
    }
}

pub(crate) fn models_view<'a>(aliases: impl Iterator<Item = &'a str>) -> Value {
    json!({
        "object": "list",
        "data": aliases
            .map(|alias| json!({ "id": alias, "object": "model", "owned_by": "campus" }))
            .collect::<Vec<_>>(),
    })
}

fn cap_output_tokens(body: &mut Map<String, Value>, cap: u64) {
    let mut named = false;
    for field in OUTPUT_TOKEN_FIELDS {
        if let Some(value) = body.get_mut(*field) {
            named = true;
            if value.as_u64().map_or(true, |tokens| tokens > cap) {
                *value = json!(cap);
            }
        }
    }
    if !named {
        body.insert("max_tokens".into(), json!(cap));
    }
}

fn read_key(path: &Path) -> anyhow::Result<SecretString> {
    let key = std::fs::read_to_string(path)
        .with_context(|| format!("failed to read {ENV_API_KEY_FILE}"))?;
    Ok(SecretString::from(key.trim().to_string()))
}

/// Inference from the environment, or `None` when no base URL is set.
pub(crate) fn load_campus_inference_from_env() -> anyhow::Result<Option<CampusInference>> {
    let Some(base_url) = std::env::var(ENV_BASE_URL)
        .ok()
        .filter(|value| !value.trim().is_empty())
    else {
        return Ok(None);
    };
    let key_file = std::env::var(ENV_API_KEY_FILE)
        .with_context(|| format!("{ENV_BASE_URL} needs {ENV_API_KEY_FILE}"))?;
    let models: BTreeMap<String, String> = serde_json::from_str(
        &std::env::var(ENV_MODELS).with_context(|| format!("{ENV_BASE_URL} needs {ENV_MODELS}"))?,
    )
    .with_context(|| format!("{ENV_MODELS} must be a JSON object of alias to model"))?;
    let max_output_tokens = match std::env::var(ENV_MAX_OUTPUT_TOKENS) {
        Ok(value) => value
            .trim()
            .parse()
            .with_context(|| format!("{ENV_MAX_OUTPUT_TOKENS} must be a positive integer"))?,
        Err(_) => DEFAULT_MAX_OUTPUT_TOKENS,
    };
    CampusInference::new(
        &base_url,
        read_key(Path::new(&key_file))?,
        models,
        max_output_tokens,
    )
    .map(Some)
}

#[cfg(test)]
mod tests {
    use super::CampusInference;
    use secrecy::SecretString;
    use serde_json::{json, Value};
    use std::collections::BTreeMap;

    fn inference() -> CampusInference {
        CampusInference::new(
            "https://api.example.test/inference/v1",
            SecretString::from("provider-key".to_string()),
            BTreeMap::from([("campus-default".into(), "accounts/x/models/small".into())]),
            4096,
        )
        .unwrap()
    }

    fn chat(extra: Value) -> Value {
        let mut body = json!({ "messages": [{ "role": "user", "content": "hi" }] });
        body.as_object_mut()
            .unwrap()
            .extend(extra.as_object().unwrap().clone());
        body
    }

    #[test]
    fn maps_the_alias_and_caps_output_tokens() {
        let inference = inference();
        assert_eq!(
            inference.chat_url.as_str(),
            "https://api.example.test/inference/v1/chat/completions"
        );
        let (alias, body) = inference
            .prepare(chat(
                json!({ "model": "campus-default", "max_tokens": 100000, "tools": [] }),
            ))
            .unwrap();
        assert_eq!(alias, "campus-default");
        assert_eq!(body["model"], "accounts/x/models/small");
        assert_eq!(body["max_tokens"], 4096);
        assert_eq!(body["tools"], json!([]));

        let (_, body) = inference
            .prepare(chat(json!({ "max_tokens": 10 })))
            .unwrap();
        assert_eq!(body["model"], "accounts/x/models/small");
        assert_eq!(body["max_tokens"], 10);

        let (_, body) = inference
            .prepare(chat(json!({ "max_completion_tokens": 9000 })))
            .unwrap();
        assert_eq!(body["max_completion_tokens"], 4096);
        assert!(body.get("max_tokens").is_none());

        let (_, body) = inference.prepare(chat(json!({}))).unwrap();
        assert_eq!(body["max_tokens"], 4096);
    }

    #[test]
    fn refuses_other_models_and_empty_requests() {
        let inference = inference();
        assert!(inference
            .prepare(chat(json!({ "model": "accounts/x/models/small" })))
            .is_err());
        assert!(inference
            .prepare(chat(json!({ "model": "gpt-5" })))
            .is_err());
        assert!(inference.prepare(json!({ "messages": [] })).is_err());
        assert!(inference.prepare(json!([1])).is_err());
    }

    #[test]
    fn needs_https_a_key_and_valid_aliases() {
        let key = || SecretString::from("k".to_string());
        let models = || BTreeMap::from([("campus-default".to_string(), "m".to_string())]);
        assert!(CampusInference::new("http://api.example.test/v1/", key(), models(), 1).is_err());
        assert!(CampusInference::new("http://127.0.0.1:9/v1/", key(), models(), 1).is_ok());
        assert!(CampusInference::new(
            "https://api.example.test/v1/",
            SecretString::from(" ".to_string()),
            models(),
            1
        )
        .is_err());
        assert!(CampusInference::new(
            "https://api.example.test/v1/",
            key(),
            BTreeMap::from([("Campus Default".to_string(), "m".to_string())]),
            1
        )
        .is_err());
        assert!(
            CampusInference::new("https://api.example.test/v1/", key(), BTreeMap::new(), 1)
                .is_err()
        );
        assert!(format!("{:?}", inference()).contains("chat_url"));
        assert!(!format!("{:?}", inference()).contains("provider-key"));
    }
}
