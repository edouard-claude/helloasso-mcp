//! Configuration, read once from the environment at startup.
//!
//! Nothing here reads the environment lazily: a missing or malformed variable
//! stops the process with a message that names the variable, rather than
//! surfacing as a confusing tool failure three calls later.

use std::{collections::BTreeSet, fmt, net::SocketAddr, str::FromStr, time::Duration};

use crate::error::{Error, Result};

/// The production API root, versioned prefix included.
pub const PRODUCTION_BASE_URL: &str = "https://api.helloasso.com/v5";
/// The production OAuth2 token endpoint.
pub const PRODUCTION_TOKEN_URL: &str = "https://api.helloasso.com/oauth2/token";
/// The sandbox API root, for testing against fake money.
pub const SANDBOX_BASE_URL: &str = "https://api.helloasso-sandbox.com/v5";
/// The sandbox OAuth2 token endpoint.
pub const SANDBOX_TOKEN_URL: &str = "https://api.helloasso-sandbox.com/oauth2/token";

/// A string that never shows up in a log or a panic message.
#[derive(Clone, PartialEq, Eq)]
pub struct Secret(String);

impl Secret {
    /// Wrap a value so it is redacted when formatted.
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The value itself. Only the HTTP layer should need this.
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl fmt::Debug for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("Secret(***)")
    }
}

impl fmt::Display for Secret {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str("***")
    }
}

/// Which HelloAsso the process talks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Environment {
    /// Real associations, real money.
    Production,
    /// HelloAsso's sandbox, with its own API clients and fake cards.
    Sandbox,
}

impl Environment {
    /// The name used in `HELLOASSO_ENVIRONMENT`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Production => "production",
            Self::Sandbox => "sandbox",
        }
    }
}

/// How the process authenticates against HelloAsso.
#[derive(Debug, Clone)]
pub enum Credentials {
    /// An API client: `client_credentials` grant, renewed by the client itself.
    Client {
        /// The client id, from the association's back office.
        client_id: String,
        /// The client secret.
        client_secret: Secret,
    },
    /// A ready-made access token, used as is and never renewed.
    Token(Secret),
}

/// A group of tools, so a deployment can expose only what it needs.
///
/// Tool count is context: a treasurer who only reads payments has no use for
/// the partner administration tools, and every unused schema is tokens spent on
/// every request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
#[non_exhaustive]
pub enum Toolset {
    /// The organization itself, the accounts reachable, and the reference lists.
    Organization,
    /// Forms: listing, public detail, statistics, quick creation, state.
    Forms,
    /// Orders, items, payments, refunds, cancellations and cash-outs.
    Sales,
    /// The checkout flow: payment links for an amount.
    Checkout,
    /// HelloAsso's public directory of organizations, forms and tags.
    Directory,
    /// Partner administration: the partner account and its webhooks.
    Partner,
}

impl Toolset {
    /// Every toolset, in the order they are documented.
    pub const ALL: [Self; 6] = [
        Self::Organization,
        Self::Forms,
        Self::Sales,
        Self::Checkout,
        Self::Directory,
        Self::Partner,
    ];

    /// The name used in `HELLOASSO_TOOLSETS`.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Organization => "organization",
            Self::Forms => "forms",
            Self::Sales => "sales",
            Self::Checkout => "checkout",
            Self::Directory => "directory",
            Self::Partner => "partner",
        }
    }
}

impl FromStr for Toolset {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self> {
        match s.trim().to_ascii_lowercase().as_str() {
            "organization" | "organisation" | "org" | "values" => Ok(Self::Organization),
            "forms" | "form" | "campaigns" => Ok(Self::Forms),
            "sales" | "orders" | "payments" | "items" => Ok(Self::Sales),
            "checkout" | "checkouts" => Ok(Self::Checkout),
            "directory" | "tags" => Ok(Self::Directory),
            "partner" | "partners" | "notifications" | "webhooks" => Ok(Self::Partner),
            other => Err(Error::config(format!(
                "HELLOASSO_TOOLSETS: unknown toolset {other:?}, expected one of organization, forms, sales, checkout, directory, partner, or all"
            ))),
        }
    }
}

impl fmt::Display for Toolset {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// Everything the process needs to know, resolved from the environment.
#[derive(Debug, Clone)]
#[non_exhaustive]
pub struct Config {
    /// How requests are authenticated.
    pub credentials: Credentials,
    /// Production or sandbox, for messages; the URLs below are what is used.
    pub environment: Environment,
    /// The API root, prefix included. No trailing slash.
    pub base_url: String,
    /// The OAuth2 token endpoint.
    pub token_url: String,
    /// The organization slug used when a tool omits one.
    pub default_organization: Option<String>,
    /// The toolsets to expose.
    pub toolsets: BTreeSet<Toolset>,
    /// When true, no write tool is registered at all.
    pub read_only: bool,
    /// Per-request timeout.
    pub timeout: Duration,
    /// How many times a transient failure is retried.
    pub max_retries: u32,
    /// Listen address for the Streamable HTTP transport, when not stdio.
    pub http_bind: Option<SocketAddr>,
    /// Bearer token required by the HTTP transport.
    pub http_token: Option<Secret>,
}

impl Config {
    /// Read the configuration from the process environment.
    pub fn from_env() -> Result<Self> {
        Self::from_getter(|key| std::env::var(key).ok())
    }

    /// Read the configuration from an arbitrary source, which is what the tests
    /// use.
    pub fn from_getter(get: impl Fn(&str) -> Option<String>) -> Result<Self> {
        let get_trimmed = |key: &str| -> Option<String> {
            get(key)
                .map(|v| v.trim().to_owned())
                .filter(|v| !v.is_empty())
        };
        let from_file = |key: &str| -> Result<Option<String>> {
            match get_trimmed(key) {
                Some(value) => Ok(Some(value)),
                None => match get_trimmed(&format!("{key}_FILE")) {
                    Some(path) => Ok(Some(
                        std::fs::read_to_string(&path)
                            .map_err(|source| Error::Io { path, source })?
                            .trim()
                            .to_owned(),
                    )
                    .filter(|v| !v.is_empty())),
                    None => Ok(None),
                },
            }
        };

        let credentials = match (
            get_trimmed("HELLOASSO_CLIENT_ID"),
            from_file("HELLOASSO_CLIENT_SECRET")?,
            from_file("HELLOASSO_ACCESS_TOKEN")?,
        ) {
            (Some(client_id), Some(secret), _) => Credentials::Client {
                client_id,
                client_secret: Secret::new(secret),
            },
            (Some(_), None, None) => {
                return Err(Error::config(
                    "HELLOASSO_CLIENT_ID is set but HELLOASSO_CLIENT_SECRET is not (or HELLOASSO_CLIENT_SECRET_FILE pointing at a file holding it).",
                ));
            }
            (None, Some(_), None) => {
                return Err(Error::config(
                    "HELLOASSO_CLIENT_SECRET is set but HELLOASSO_CLIENT_ID is not.",
                ));
            }
            (_, _, Some(token)) => Credentials::Token(Secret::new(token)),
            (None, None, None) => {
                return Err(Error::config(
                    "HELLOASSO_CLIENT_ID and HELLOASSO_CLIENT_SECRET are required. Create an API client in the HelloAsso back office: My account → Integrations and API.",
                ));
            }
        };

        let environment = match get_trimmed("HELLOASSO_ENVIRONMENT")
            .map(|e| e.to_ascii_lowercase())
            .as_deref()
        {
            None | Some("production" | "prod" | "live") => Environment::Production,
            Some("sandbox" | "test") => Environment::Sandbox,
            Some(other) => {
                return Err(Error::config(format!(
                    "HELLOASSO_ENVIRONMENT must be production or sandbox, got {other:?}"
                )));
            }
        };
        let (default_base, default_token) = match environment {
            Environment::Production => (PRODUCTION_BASE_URL, PRODUCTION_TOKEN_URL),
            Environment::Sandbox => (SANDBOX_BASE_URL, SANDBOX_TOKEN_URL),
        };
        let base_url = url_var(
            "HELLOASSO_BASE_URL",
            get_trimmed("HELLOASSO_BASE_URL"),
            default_base,
        )?;
        let token_url = url_var(
            "HELLOASSO_TOKEN_URL",
            get_trimmed("HELLOASSO_TOKEN_URL"),
            default_token,
        )?;

        let default_organization = get_trimmed("HELLOASSO_ORGANIZATION")
            .or_else(|| get_trimmed("HELLOASSO_ORGANIZATION_SLUG"));
        if let Some(slug) = default_organization
            .as_deref()
            .filter(|s| !looks_like_slug(s))
        {
            return Err(Error::config(format!(
                "HELLOASSO_ORGANIZATION must be an organization slug such as \"mon-association\", got {slug:?}"
            )));
        }

        let toolsets = parse_toolsets(get_trimmed("HELLOASSO_TOOLSETS").as_deref())?;
        let read_only = parse_bool(
            "HELLOASSO_READ_ONLY",
            get_trimmed("HELLOASSO_READ_ONLY").as_deref(),
            false,
        )?;

        let http_bind = match get_trimmed("HELLOASSO_HTTP_BIND") {
            Some(addr) => Some(addr.parse::<SocketAddr>().map_err(|e| {
                Error::config(format!("HELLOASSO_HTTP_BIND is not a socket address: {e}"))
            })?),
            None => None,
        };
        let http_token = get_trimmed("HELLOASSO_HTTP_TOKEN").map(Secret::new);
        if let Some(addr) = http_bind.filter(|a| !a.ip().is_loopback() && http_token.is_none()) {
            return Err(Error::config(format!(
                "HELLOASSO_HTTP_BIND is {addr}, which is reachable from outside this machine: set HELLOASSO_HTTP_TOKEN so the endpoint requires a bearer token, or bind to 127.0.0.1"
            )));
        }

        Ok(Self {
            credentials,
            environment,
            base_url,
            token_url,
            default_organization,
            toolsets,
            read_only,
            timeout: Duration::from_secs(parse_u64(
                "HELLOASSO_TIMEOUT_SECS",
                get_trimmed("HELLOASSO_TIMEOUT_SECS").as_deref(),
                30,
            )?),
            max_retries: u32::try_from(parse_u64(
                "HELLOASSO_MAX_RETRIES",
                get_trimmed("HELLOASSO_MAX_RETRIES").as_deref(),
                3,
            )?)
            .map_err(|_| Error::config("HELLOASSO_MAX_RETRIES is too large"))?,
            http_bind,
            http_token,
        })
    }

    /// True when the given toolset is exposed.
    pub fn has(&self, toolset: Toolset) -> bool {
        self.toolsets.contains(&toolset)
    }

    /// The client id, when authenticating as an API client.
    pub fn client_id(&self) -> Option<&str> {
        match &self.credentials {
            Credentials::Client { client_id, .. } => Some(client_id),
            Credentials::Token(_) => None,
        }
    }
}

fn url_var(name: &str, raw: Option<String>, default: &str) -> Result<String> {
    let url = raw
        .unwrap_or_else(|| default.to_owned())
        .trim_end_matches('/')
        .to_owned();
    if !url.starts_with("http://") && !url.starts_with("https://") {
        return Err(Error::config(format!(
            "{name} must start with http:// or https://, got {url:?}"
        )));
    }
    Ok(url)
}

fn parse_toolsets(raw: Option<&str>) -> Result<BTreeSet<Toolset>> {
    let Some(raw) = raw else {
        return Ok(Toolset::ALL.into_iter().collect());
    };
    let mut set = BTreeSet::new();
    for part in raw.split(',').filter(|p| !p.trim().is_empty()) {
        if part.trim().eq_ignore_ascii_case("all") {
            set.extend(Toolset::ALL);
            continue;
        }
        set.insert(part.parse::<Toolset>()?);
    }
    if set.is_empty() {
        return Err(Error::config(
            "HELLOASSO_TOOLSETS is set but selects no toolset",
        ));
    }
    Ok(set)
}

fn parse_bool(name: &str, raw: Option<&str>, default: bool) -> Result<bool> {
    match raw {
        None => Ok(default),
        Some(v) => match v.to_ascii_lowercase().as_str() {
            "1" | "true" | "yes" | "on" => Ok(true),
            "0" | "false" | "no" | "off" => Ok(false),
            other => Err(Error::config(format!(
                "{name} expects a boolean (1/0, true/false), got {other:?}"
            ))),
        },
    }
}

fn parse_u64(name: &str, raw: Option<&str>, default: u64) -> Result<u64> {
    match raw {
        None => Ok(default),
        Some(v) => v
            .parse::<u64>()
            .map_err(|_| Error::config(format!("{name} must be a positive integer, got {v:?}"))),
    }
}

/// A shape check: HelloAsso slugs are lowercase ASCII words joined by dashes.
pub fn looks_like_slug(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 250
        && value
            .chars()
            .all(|c| c.is_ascii_lowercase() || c.is_ascii_digit() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    #![allow(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        reason = "a test that cannot fail loudly is not a test"
    )]
    use super::*;

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> + use<> {
        let owned: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |key| owned.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone())
    }

    const CLIENT: [(&str, &str); 2] = [
        ("HELLOASSO_CLIENT_ID", "id"),
        ("HELLOASSO_CLIENT_SECRET", "secret"),
    ];

    fn with(extra: &[(&'static str, &'static str)]) -> Vec<(&'static str, &'static str)> {
        let mut pairs = CLIENT.to_vec();
        pairs.extend_from_slice(extra);
        pairs
    }

    #[test]
    fn missing_credentials_name_the_variables_and_where_to_get_them() {
        let message = Config::from_getter(env(&[])).unwrap_err().to_string();
        assert!(message.contains("HELLOASSO_CLIENT_ID"), "{message}");
        assert!(message.contains("Integrations and API"), "{message}");
    }

    #[test]
    fn half_a_client_is_refused() {
        let err = Config::from_getter(env(&[("HELLOASSO_CLIENT_ID", "id")])).unwrap_err();
        assert!(err.to_string().contains("HELLOASSO_CLIENT_SECRET"), "{err}");
    }

    #[test]
    fn a_ready_made_token_is_enough() {
        let cfg = Config::from_getter(env(&[("HELLOASSO_ACCESS_TOKEN", "jwt")])).unwrap();
        assert!(matches!(cfg.credentials, Credentials::Token(_)));
    }

    #[test]
    fn defaults_are_the_documented_ones() {
        let cfg = Config::from_getter(env(&CLIENT)).unwrap();
        assert_eq!(cfg.base_url, PRODUCTION_BASE_URL);
        assert_eq!(cfg.token_url, PRODUCTION_TOKEN_URL);
        assert_eq!(cfg.toolsets.len(), Toolset::ALL.len());
        assert!(!cfg.read_only);
        assert_eq!(cfg.timeout, Duration::from_secs(30));
        assert_eq!(cfg.client_id(), Some("id"));
    }

    #[test]
    fn the_sandbox_switches_both_urls() {
        let cfg = Config::from_getter(env(&with(&[("HELLOASSO_ENVIRONMENT", "sandbox")]))).unwrap();
        assert_eq!(cfg.base_url, SANDBOX_BASE_URL);
        assert_eq!(cfg.token_url, SANDBOX_TOKEN_URL);
    }

    #[test]
    fn the_secret_is_never_printed() {
        let cfg = Config::from_getter(env(&[
            ("HELLOASSO_CLIENT_ID", "id"),
            ("HELLOASSO_CLIENT_SECRET", "super-secret"),
        ]))
        .unwrap();
        let printed = format!("{cfg:?}");
        assert!(!printed.contains("super-secret"), "{printed}");
    }

    #[test]
    fn a_trailing_slash_on_the_base_url_is_dropped() {
        let cfg = Config::from_getter(env(&with(&[(
            "HELLOASSO_BASE_URL",
            "https://example.test/v5/",
        )])))
        .unwrap();
        assert_eq!(cfg.base_url, "https://example.test/v5");
    }

    #[test]
    fn a_base_url_without_a_scheme_is_refused() {
        let err = Config::from_getter(env(&with(&[("HELLOASSO_BASE_URL", "api.helloasso.com")])))
            .unwrap_err();
        assert!(err.to_string().contains("http://"), "{err}");
    }

    #[test]
    fn toolsets_accept_aliases_and_all() {
        let cfg =
            Config::from_getter(env(&with(&[("HELLOASSO_TOOLSETS", "payments, forms")]))).unwrap();
        assert!(cfg.has(Toolset::Sales));
        assert!(cfg.has(Toolset::Forms));
        assert!(!cfg.has(Toolset::Partner));
    }

    #[test]
    fn an_unknown_toolset_lists_the_valid_ones() {
        let err =
            Config::from_getter(env(&with(&[("HELLOASSO_TOOLSETS", "members")]))).unwrap_err();
        assert!(err.to_string().contains("checkout"), "{err}");
    }

    #[test]
    fn an_organization_that_is_not_a_slug_is_refused() {
        let err = Config::from_getter(env(&with(&[("HELLOASSO_ORGANIZATION", "Mon Association")])))
            .unwrap_err();
        assert!(err.to_string().contains("slug"), "{err}");
    }

    #[test]
    fn binding_http_off_loopback_without_a_token_is_refused() {
        let err = Config::from_getter(env(&with(&[("HELLOASSO_HTTP_BIND", "0.0.0.0:8080")])))
            .unwrap_err();
        assert!(err.to_string().contains("HELLOASSO_HTTP_TOKEN"), "{err}");
        assert!(
            Config::from_getter(env(&with(&[("HELLOASSO_HTTP_BIND", "127.0.0.1:8080")]))).is_ok()
        );
    }

    #[test]
    fn slug_shape_check() {
        assert!(looks_like_slug("les-amis-du-velo"));
        assert!(!looks_like_slug("Les Amis"));
        assert!(!looks_like_slug(""));
    }
}
