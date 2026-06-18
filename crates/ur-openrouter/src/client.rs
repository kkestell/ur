//! The OpenRouter client handle, its builder, and the HTTP-client wrapper.

use std::sync::Arc;
use std::time::Duration;

use ur_core::{Error, Result};

const API_KEY_ENV: &str = "OPENROUTER_API_KEY";

pub(crate) const DEFAULT_BASE_URL: &str = "https://openrouter.ai/api/v1";

const DEFAULT_TIMEOUT: Duration = Duration::from_secs(10 * 60);
const DEFAULT_MAX_RETRIES: u32 = 3;
const MAX_USER_LEN: usize = 512;

/// Provider-routing preferences serialized into OpenRouter's `provider` object.
///
/// OpenRouter fronts many upstream providers for a given model and chooses one
/// per request. These fields steer that choice; leave them at their defaults to
/// use OpenRouter's own ordering.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ProviderRouting {
    /// Provider slugs to try, in order of preference.
    pub order: Vec<String>,
    /// Whether OpenRouter may fall back to providers outside `order`/`only`.
    pub allow_fallbacks: Option<bool>,
    /// Sort strategy across candidate providers: `"price"`, `"throughput"`, or
    /// `"latency"`.
    pub sort: Option<String>,
    /// Restrict routing to only these provider slugs.
    pub only: Vec<String>,
    /// Never route to these provider slugs.
    pub ignore: Vec<String>,
}

impl ProviderRouting {
    /// True when no preference is set, so no `provider` object is sent.
    pub(crate) fn is_empty(&self) -> bool {
        self.order.is_empty()
            && self.allow_fallbacks.is_none()
            && self.sort.is_none()
            && self.only.is_empty()
            && self.ignore.is_empty()
    }
}

/// A preconfigured HTTP client for the OpenRouter provider.
#[derive(Clone, Debug)]
pub struct OpenRouterHttpClient {
    pub(crate) client: reqwest::Client,
}

impl OpenRouterHttpClient {
    /// Wraps a preconfigured reqwest client for the OpenRouter provider.
    pub fn from_reqwest(client: reqwest::Client) -> Self {
        Self { client }
    }
}

impl Default for OpenRouterHttpClient {
    fn default() -> Self {
        Self::from_reqwest(reqwest::Client::new())
    }
}

pub(crate) struct Config {
    pub(crate) http: OpenRouterHttpClient,
    pub(crate) api_key: String,
    pub(crate) base_url: String,
    pub(crate) user: Option<String>,
    pub(crate) referer: Option<String>,
    pub(crate) title: Option<String>,
    pub(crate) provider_routing: Option<ProviderRouting>,
    pub(crate) timeout: Duration,
    pub(crate) max_retries: u32,
}

/// A cheap-to-clone handle over an OpenRouter connection pool, auth, and retry
/// policy.
#[derive(Clone)]
pub struct OpenRouterClient {
    config: Arc<Config>,
}

impl OpenRouterClient {
    /// Reads the key from `$OPENROUTER_API_KEY`.
    pub fn try_from_env() -> Result<Self> {
        Self::builder().build()
    }

    /// Reads the key from `$OPENROUTER_API_KEY`. Panics if unset.
    pub fn from_env() -> Self {
        Self::try_from_env()
            .expect("OPENROUTER_API_KEY is set and the client configuration is valid")
    }

    /// A client with the given API key and otherwise-default settings.
    pub fn new(api_key: impl Into<String>) -> Self {
        Self::builder()
            .api_key(api_key)
            .build()
            .expect("a client with an explicit API key and default settings is valid")
    }

    /// Returns a new client builder.
    pub fn builder() -> OpenRouterClientBuilder {
        OpenRouterClientBuilder::default()
    }

    pub(crate) fn config(&self) -> &Config {
        &self.config
    }

    pub(crate) fn shared_config(&self) -> Arc<Config> {
        Arc::clone(&self.config)
    }
}

impl std::fmt::Debug for OpenRouterClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("OpenRouterClient")
            .field("base_url", &self.config.base_url)
            .finish_non_exhaustive()
    }
}

/// A non-consuming builder for [`OpenRouterClient`].
#[derive(Clone, Debug, Default)]
pub struct OpenRouterClientBuilder {
    api_key: Option<String>,
    base_url: Option<String>,
    user: Option<String>,
    referer: Option<String>,
    title: Option<String>,
    provider_routing: Option<ProviderRouting>,
    timeout: Option<Duration>,
    max_retries: Option<u32>,
    http_client: Option<OpenRouterHttpClient>,
}

impl OpenRouterClientBuilder {
    /// Sets the API key. If never set, falls back to `$OPENROUTER_API_KEY` at
    /// build time.
    pub fn api_key(&mut self, key: impl Into<String>) -> &mut Self {
        self.api_key = Some(key.into());
        self
    }

    /// Overrides the base URL. Default: `https://openrouter.ai/api/v1`.
    pub fn base_url(&mut self, url: impl Into<String>) -> &mut Self {
        self.base_url = Some(url.into());
        self
    }

    /// Sets the optional end-user identifier sent as `user`.
    pub fn user(&mut self, user: impl Into<String>) -> &mut Self {
        self.user = Some(user.into());
        self
    }

    /// Sets the `HTTP-Referer` header used for OpenRouter app attribution.
    pub fn referer(&mut self, referer: impl Into<String>) -> &mut Self {
        self.referer = Some(referer.into());
        self
    }

    /// Sets the `X-Title` header used for OpenRouter app attribution.
    pub fn title(&mut self, title: impl Into<String>) -> &mut Self {
        self.title = Some(title.into());
        self
    }

    /// Sets fixed provider-routing preferences applied to every request.
    pub fn provider_routing(&mut self, routing: ProviderRouting) -> &mut Self {
        self.provider_routing = Some(routing);
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
    pub fn http_client(&mut self, client: OpenRouterHttpClient) -> &mut Self {
        self.http_client = Some(client);
        self
    }

    /// Validates the configuration and constructs the client.
    pub fn build(&mut self) -> Result<OpenRouterClient> {
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

        let provider_routing = self
            .provider_routing
            .clone()
            .filter(|routing| !routing.is_empty());

        Ok(OpenRouterClient {
            config: Arc::new(Config {
                http: self.http_client.clone().unwrap_or_default(),
                api_key,
                base_url,
                user: self.user.clone(),
                referer: self.referer.clone(),
                title: self.title.clone(),
                provider_routing,
                timeout: self.timeout.unwrap_or(DEFAULT_TIMEOUT),
                max_retries: self.max_retries.unwrap_or(DEFAULT_MAX_RETRIES),
            }),
        })
    }
}

fn resolve_api_key(explicit: Option<String>, from_env: Option<String>) -> Result<String> {
    explicit
        .or(from_env)
        .filter(|key| !key.is_empty())
        .ok_or_else(|| Error::Config {
            message: format!("no API key set and {API_KEY_ENV} is empty or unset"),
        })
}

fn validate_user(user: &str) -> Result<()> {
    let valid = !user.is_empty()
        && user.len() <= MAX_USER_LEN
        && user
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'));

    if valid {
        Ok(())
    } else {
        Err(Error::Config {
            message: format!("invalid user '{user}'"),
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
        let client = OpenRouterClient::builder()
            .api_key("explicit")
            .build()
            .unwrap();
        assert_eq!(client.config().api_key, "explicit");
        assert_eq!(client.config().base_url, DEFAULT_BASE_URL);
    }

    #[test]
    fn invalid_base_url_is_rejected() {
        let error = OpenRouterClient::builder()
            .api_key("key")
            .base_url("not a url")
            .build()
            .unwrap_err();
        assert!(matches!(error, Error::Config { .. }));
    }

    #[test]
    fn attribution_headers_and_routing_are_kept() {
        let client = OpenRouterClient::builder()
            .api_key("key")
            .referer("https://example.com")
            .title("Example App")
            .provider_routing(ProviderRouting {
                order: vec!["openai".to_owned()],
                allow_fallbacks: Some(false),
                sort: Some("throughput".to_owned()),
                ..Default::default()
            })
            .build()
            .unwrap();
        assert_eq!(
            client.config().referer.as_deref(),
            Some("https://example.com")
        );
        assert_eq!(client.config().title.as_deref(), Some("Example App"));
        let routing = client.config().provider_routing.as_ref().unwrap();
        assert_eq!(routing.order, vec!["openai".to_owned()]);
        assert_eq!(routing.allow_fallbacks, Some(false));
        assert_eq!(routing.sort.as_deref(), Some("throughput"));
    }

    #[test]
    fn empty_routing_is_dropped() {
        let client = OpenRouterClient::builder()
            .api_key("key")
            .provider_routing(ProviderRouting::default())
            .build()
            .unwrap();
        assert!(client.config().provider_routing.is_none());
    }

    #[test]
    fn timeout_retry_and_http_client_overrides_are_kept() {
        let http = OpenRouterHttpClient::from_reqwest(reqwest::Client::new());
        let client = OpenRouterClient::builder()
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
        let client = OpenRouterClient::new("key");
        assert_eq!(client.config().timeout, DEFAULT_TIMEOUT);
        assert_eq!(client.config().max_retries, DEFAULT_MAX_RETRIES);
        assert_eq!(client.config().user, None);
        assert_eq!(client.config().referer, None);
        assert_eq!(client.config().title, None);
    }

    #[test]
    fn user_is_validated() {
        let ok = OpenRouterClient::builder()
            .api_key("key")
            .user("tenant-42_a")
            .build()
            .unwrap();
        assert_eq!(ok.config().user.as_deref(), Some("tenant-42_a"));

        let error = OpenRouterClient::builder()
            .api_key("key")
            .user("has spaces")
            .build()
            .unwrap_err();
        assert!(matches!(error, Error::Config { .. }));

        let too_long = "a".repeat(MAX_USER_LEN + 1);
        let error = OpenRouterClient::builder()
            .api_key("key")
            .user(too_long)
            .build()
            .unwrap_err();
        assert!(matches!(error, Error::Config { .. }));
    }

    #[test]
    fn debug_is_opaque_and_hides_the_api_key() {
        let client = OpenRouterClient::new("super-secret");
        let debug = format!("{client:?}");
        assert!(debug.contains("OpenRouterClient"));
        assert!(!debug.contains("super-secret"));
    }
}
