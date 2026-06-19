//! The OpenAI client handle, its builder, and the HTTP-client wrapper.

use std::sync::Arc;
use std::time::Duration;

use ur_core::{Error, Result};

const API_KEY_ENV: &str = "OPENAI_API_KEY";

pub(crate) const DEFAULT_BASE_URL: &str = "https://api.openai.com/v1";

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const DEFAULT_MAX_RETRIES: u32 = 3;
const MAX_USER_LEN: usize = 512;

/// A preconfigured HTTP client for the OpenAI provider.
#[derive(Clone, Debug)]
pub struct OpenAiHttpClient {
    pub(crate) client: reqwest::Client,
}

impl OpenAiHttpClient {
    /// Wraps a preconfigured reqwest client for the OpenAI provider.
    pub fn from_reqwest(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl Default for OpenAiHttpClient {
    fn default() -> Self {
        Self::from_reqwest(reqwest::Client::new())
    }
}

pub(crate) struct Config {
    pub(crate) http: OpenAiHttpClient,
    pub(crate) api_key: String,
    pub(crate) base_url: String,
    pub(crate) user: Option<String>,
    pub(crate) timeout: Duration,
    pub(crate) max_retries: u32,
}

/// A cheap-to-clone handle over an OpenAI connection pool, auth, and retry
/// policy.
#[derive(Clone)]
pub struct OpenAiClient {
    config: Arc<Config>,
}

impl OpenAiClient {
    /// Reads the key from `$OPENAI_API_KEY`.
    pub fn try_from_env() -> Result<Self> {
        Self::builder().build()
    }

    /// Reads the key from `$OPENAI_API_KEY`. Panics if unset.
    pub fn from_env() -> Self {
        Self::try_from_env().expect("OPENAI_API_KEY is set and the client configuration is valid")
    }

    /// A client with the given API key and otherwise-default settings.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::builder()
            .api_key(api_key)
            .build()
            .expect("a client with an explicit API key and default settings is valid")
    }

    /// Returns a new client builder.
    pub fn builder() -> OpenAiClientBuilder {
        OpenAiClientBuilder::default()
    }

    pub(crate) fn config(&self) -> &Config {
        &self.config
    }

    pub(crate) fn shared_config(&self) -> Arc<Config> {
        Arc::clone(&self.config)
    }
}

impl std::fmt::Debug for OpenAiClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenAiClient")
            .field("base_url", &self.config.base_url)
            .finish_non_exhaustive()
    }
}

/// A non-consuming builder for [`OpenAiClient`].
#[derive(Clone, Debug, Default)]
pub struct OpenAiClientBuilder {
    api_key: Option<String>,
    base_url: Option<String>,
    user: Option<String>,
    timeout: Option<Duration>,
    max_retries: Option<u32>,
    http_client: Option<OpenAiHttpClient>,
}

impl OpenAiClientBuilder {
    /// Sets the API key. If never set, falls back to `$OPENAI_API_KEY` at build
    /// time.
    pub fn api_key(&mut self, key: impl Into<String>) -> &mut Self {
        self.api_key = Some(key.into());
        self
    }

    /// Overrides the base URL. Default: `https://api.openai.com/v1`.
    pub fn base_url(&mut self, url: impl Into<String>) -> &mut Self {
        self.base_url = Some(url.into());
        self
    }

    /// Sets the optional end-user identifier sent as `user`.
    pub fn user(&mut self, user: impl Into<String>) -> &mut Self {
        self.user = Some(user.into());
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
    pub fn http_client(&mut self, client: OpenAiHttpClient) -> &mut Self {
        self.http_client = Some(client);
        self
    }

    /// Validates the configuration and constructs the client.
    pub fn build(&mut self) -> Result<OpenAiClient> {
        let api_key = resolve_api_key(self.api_key.clone(), std::env::var(API_KEY_ENV).ok())?;
        let base_url = self
            .base_url
            .clone()
            .unwrap_or_else(|| DEFAULT_BASE_URL.to_owned());

        if reqwest::Url::parse(&base_url).is_err() {
            return Err(Error::Config {
                message: format!("base_url '{base_url}' is not a valid URL"),
            });
        }

        if let Some(user) = &self.user {
            validate_user(user)?;
        }

        Ok(OpenAiClient {
            config: Arc::new(Config {
                http: self.http_client.clone().unwrap_or_default(),
                api_key,
                base_url,
                user: self.user.clone(),
                timeout: self.timeout.unwrap_or(DEFAULT_TIMEOUT),
                max_retries: self.max_retries.unwrap_or(DEFAULT_MAX_RETRIES),
            }),
        })
    }
}

fn resolve_api_key(explicit: Option<String>, from_env: Option<String>) -> Result<String> {
    ur_openai_compat::keys::resolve_api_key(explicit, from_env, API_KEY_ENV)
}

fn validate_user(user: &str) -> Result<()> {
    ur_openai_compat::keys::validate_user(user, MAX_USER_LEN)
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
        let client = OpenAiClient::builder().api_key("explicit").build().unwrap();
        assert_eq!(client.config().api_key, "explicit");
        assert_eq!(client.config().base_url, DEFAULT_BASE_URL);
    }

    #[test]
    fn invalid_base_url_is_rejected() {
        let error = OpenAiClient::builder()
            .api_key("key")
            .base_url("not a url")
            .build()
            .unwrap_err();
        assert!(matches!(error, Error::Config { .. }));
    }

    #[test]
    fn timeout_retry_and_http_client_overrides_are_kept() {
        let http = OpenAiHttpClient::from_reqwest(reqwest::Client::new());
        let client = OpenAiClient::builder()
            .api_key("key")
            .timeout(Duration::from_secs(30))
            .max_retries(7)
            .http_client(http)
            .build()
            .unwrap();
        assert_eq!(client.config().timeout, Duration::from_secs(30));
        assert_eq!(client.config().max_retries, 7);
    }

    #[test]
    fn defaults_apply_without_overrides() {
        let client = OpenAiClient::new("key");
        assert_eq!(client.config().timeout, DEFAULT_TIMEOUT);
        assert_eq!(client.config().max_retries, DEFAULT_MAX_RETRIES);
        assert_eq!(client.config().user, None);
    }

    #[test]
    fn user_is_validated() {
        let ok = OpenAiClient::builder()
            .api_key("key")
            .user("tenant-42_a")
            .build()
            .unwrap();
        assert_eq!(ok.config().user.as_deref(), Some("tenant-42_a"));

        let error = OpenAiClient::builder()
            .api_key("key")
            .user("has spaces")
            .build()
            .unwrap_err();
        assert!(matches!(error, Error::Config { .. }));

        let too_long = "a".repeat(MAX_USER_LEN + 1);
        let error = OpenAiClient::builder()
            .api_key("key")
            .user(too_long)
            .build()
            .unwrap_err();
        assert!(matches!(error, Error::Config { .. }));
    }

    #[test]
    fn debug_is_opaque_and_hides_the_api_key() {
        let client = OpenAiClient::new("super-secret");
        let debug = format!("{client:?}");
        assert!(debug.contains("OpenAiClient"));
        assert!(!debug.contains("super-secret"));
    }
}
