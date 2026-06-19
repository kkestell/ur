//! The DeepSeek client handle, its builder, and the HTTP-client wrapper.

use std::sync::Arc;
use std::time::Duration;

use ur_core::{Error, Result};

/// Environment variable consulted for the API key when none is set explicitly.
const API_KEY_ENV: &str = "DEEPSEEK_API_KEY";

/// Default base URL for the DeepSeek API.
pub(crate) const DEFAULT_BASE_URL: &str = "https://api.deepseek.com";

/// Base URL for DeepSeek's beta API, required for strict-mode tools.
pub(crate) const BETA_BASE_URL: &str = "https://api.deepseek.com/beta";

/// Default per-request timeout. The server may hold a connection up to ten
/// minutes before inference starts.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(15 * 60);

/// Default number of automatic retries for retryable failures.
const DEFAULT_MAX_RETRIES: u32 = 3;

/// Maximum length of a `user_id`.
const MAX_USER_ID_LEN: usize = 512;

/// A preconfigured HTTP client for the DeepSeek provider.
#[derive(Clone, Debug)]
pub struct DeepSeekHttpClient {
    pub(crate) client: reqwest::Client,
}

impl DeepSeekHttpClient {
    /// Wraps a preconfigured reqwest client for the DeepSeek provider.
    pub fn from_reqwest(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl Default for DeepSeekHttpClient {
    fn default() -> Self {
        Self::from_reqwest(reqwest::Client::new())
    }
}

/// Resolved client configuration shared behind an [`Arc`].
pub(crate) struct Config {
    pub(crate) http: DeepSeekHttpClient,
    pub(crate) api_key: String,
    pub(crate) base_url: String,
    pub(crate) user_id: Option<String>,
    pub(crate) timeout: Duration,
    pub(crate) max_retries: u32,
}

impl Config {
    /// Returns whether the active base URL is DeepSeek's beta endpoint, which
    /// strict-mode tools require.
    pub(crate) fn is_beta(&self) -> bool {
        self.base_url == BETA_BASE_URL
    }
}

/// A cheap-to-clone handle over a DeepSeek connection pool, auth, and retry
/// policy.
#[derive(Clone)]
pub struct DeepSeekClient {
    config: Arc<Config>,
}

impl DeepSeekClient {
    /// Reads the key from `$DEEPSEEK_API_KEY`.
    pub fn try_from_env() -> Result<Self> {
        Self::builder().build()
    }

    /// Reads the key from `$DEEPSEEK_API_KEY`. Panics if unset.
    pub fn from_env() -> Self {
        Self::try_from_env().expect("DEEPSEEK_API_KEY is set and the client configuration is valid")
    }

    /// A client with the given API key and otherwise-default settings.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::builder()
            .api_key(api_key)
            .build()
            .expect("a client with an explicit API key and default settings is valid")
    }

    /// Returns a new client builder.
    pub fn builder() -> DeepSeekClientBuilder {
        DeepSeekClientBuilder::default()
    }

    pub(crate) fn config(&self) -> &Config {
        &self.config
    }

    pub(crate) fn shared_config(&self) -> Arc<Config> {
        Arc::clone(&self.config)
    }
}

impl std::fmt::Debug for DeepSeekClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("DeepSeekClient")
            .field("base_url", &self.config.base_url)
            .finish_non_exhaustive()
    }
}

/// A non-consuming builder for [`DeepSeekClient`].
#[derive(Clone, Debug, Default)]
pub struct DeepSeekClientBuilder {
    api_key: Option<String>,
    base_url: Option<String>,
    beta: bool,
    user_id: Option<String>,
    timeout: Option<Duration>,
    max_retries: Option<u32>,
    http_client: Option<DeepSeekHttpClient>,
}

impl DeepSeekClientBuilder {
    /// Sets the API key. If never set, falls back to `$DEEPSEEK_API_KEY` at
    /// build time.
    pub fn api_key(&mut self, key: impl Into<String>) -> &mut Self {
        self.api_key = Some(key.into());
        self
    }

    /// Overrides the base URL. Default: `https://api.deepseek.com`.
    pub fn base_url(&mut self, url: impl Into<String>) -> &mut Self {
        self.base_url = Some(url.into());
        self
    }

    /// Selects the beta base URL, required for strict-mode tools and prefix
    /// completion. An explicit `base_url` takes precedence.
    pub fn beta(&mut self, enabled: bool) -> &mut Self {
        self.beta = enabled;
        self
    }

    /// Sets an optional content-safety / cache / scheduling isolation key.
    pub fn user_id(&mut self, id: impl Into<String>) -> &mut Self {
        self.user_id = Some(id.into());
        self
    }

    /// Sets the per-request timeout.
    pub fn timeout(&mut self, dur: Duration) -> &mut Self {
        self.timeout = Some(dur);
        self
    }

    /// Sets the maximum number of automatic retries.
    pub fn max_retries(&mut self, n: u32) -> &mut Self {
        self.max_retries = Some(n);
        self
    }

    /// Supplies a preconfigured HTTP client.
    pub fn http_client(&mut self, client: DeepSeekHttpClient) -> &mut Self {
        self.http_client = Some(client);
        self
    }

    /// Validates the configuration and constructs the client.
    pub fn build(&mut self) -> Result<DeepSeekClient> {
        let api_key = resolve_api_key(self.api_key.clone(), std::env::var(API_KEY_ENV).ok())?;

        let base_url = match &self.base_url {
            Some(url) => url.clone(),
            None if self.beta => BETA_BASE_URL.to_owned(),
            None => DEFAULT_BASE_URL.to_owned(),
        };

        if reqwest::Url::parse(&base_url).is_err() {
            return Err(Error::Config {
                message: format!("base_url '{base_url}' is not a valid URL"),
            });
        }

        if let Some(user_id) = &self.user_id {
            validate_user_id(user_id)?;
        }

        Ok(DeepSeekClient {
            config: Arc::new(Config {
                http: self.http_client.clone().unwrap_or_default(),
                api_key,
                base_url,
                user_id: self.user_id.clone(),
                timeout: self.timeout.unwrap_or(DEFAULT_TIMEOUT),
                max_retries: self.max_retries.unwrap_or(DEFAULT_MAX_RETRIES),
            }),
        })
    }
}

/// Resolves the API key from an explicit value, falling back to the
/// environment. An empty value is treated as absent.
fn resolve_api_key(explicit: Option<String>, from_env: Option<String>) -> Result<String> {
    ur_openai_compat::keys::resolve_api_key(explicit, from_env, API_KEY_ENV)
}

fn validate_user_id(user_id: &str) -> Result<()> {
    let valid = !user_id.is_empty()
        && user_id.len() <= MAX_USER_ID_LEN
        && user_id
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));

    if valid {
        Ok(())
    } else {
        Err(Error::Config {
            message: format!("invalid user_id '{user_id}'"),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn api_key_resolution_prefers_explicit_then_environment() {
        assert_eq!(
            resolve_api_key(Some("explicit".to_owned()), Some("env".to_owned())).unwrap(),
            "explicit"
        );
        assert_eq!(
            resolve_api_key(None, Some("env".to_owned())).unwrap(),
            "env"
        );
    }

    #[test]
    fn missing_or_empty_api_key_is_rejected() {
        assert!(matches!(
            resolve_api_key(None, None),
            Err(Error::Config { .. })
        ));
        assert!(matches!(
            resolve_api_key(None, Some(String::new())),
            Err(Error::Config { .. })
        ));
    }

    #[test]
    fn explicit_api_key_is_stored_with_default_base_url() {
        let client = DeepSeekClient::builder()
            .api_key("explicit")
            .build()
            .unwrap();
        assert_eq!(client.config().api_key, "explicit");
        assert_eq!(client.config().base_url, DEFAULT_BASE_URL);
    }

    #[test]
    fn beta_selects_the_beta_base_url() {
        let client = DeepSeekClient::builder()
            .api_key("key")
            .beta(true)
            .build()
            .unwrap();
        assert_eq!(client.config().base_url, BETA_BASE_URL);
    }

    #[test]
    fn explicit_base_url_takes_precedence_over_beta() {
        let client = DeepSeekClient::builder()
            .api_key("key")
            .beta(true)
            .base_url("https://proxy.example.com")
            .build()
            .unwrap();
        assert_eq!(client.config().base_url, "https://proxy.example.com");
    }

    #[test]
    fn invalid_base_url_is_rejected() {
        let error = DeepSeekClient::builder()
            .api_key("key")
            .base_url("not a url")
            .build()
            .unwrap_err();
        assert!(matches!(error, Error::Config { .. }));
    }

    #[test]
    fn timeout_and_retry_overrides_are_kept() {
        let client = DeepSeekClient::builder()
            .api_key("key")
            .timeout(Duration::from_secs(30))
            .max_retries(7)
            .build()
            .unwrap();
        assert_eq!(client.config().timeout, Duration::from_secs(30));
        assert_eq!(client.config().max_retries, 7);
    }

    #[test]
    fn defaults_apply_without_overrides() {
        let client = DeepSeekClient::new("key");
        assert_eq!(client.config().timeout, DEFAULT_TIMEOUT);
        assert_eq!(client.config().max_retries, DEFAULT_MAX_RETRIES);
        assert_eq!(client.config().user_id, None);
    }

    #[test]
    fn user_id_is_validated() {
        let ok = DeepSeekClient::builder()
            .api_key("key")
            .user_id("tenant-42_a")
            .build()
            .unwrap();
        assert_eq!(ok.config().user_id.as_deref(), Some("tenant-42_a"));

        let error = DeepSeekClient::builder()
            .api_key("key")
            .user_id("has spaces")
            .build()
            .unwrap_err();
        assert!(matches!(error, Error::Config { .. }));

        let too_long = "a".repeat(MAX_USER_ID_LEN + 1);
        let error = DeepSeekClient::builder()
            .api_key("key")
            .user_id(too_long)
            .build()
            .unwrap_err();
        assert!(matches!(error, Error::Config { .. }));
    }

    #[test]
    fn debug_is_opaque_and_hides_the_api_key() {
        let client = DeepSeekClient::new("super-secret");
        let debug = format!("{client:?}");
        assert!(debug.contains("DeepSeekClient"));
        assert!(!debug.contains("super-secret"));
    }
}
