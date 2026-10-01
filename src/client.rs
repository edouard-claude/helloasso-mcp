//! The HelloAsso v5 client: REST over HTTP, with the API's manners respected.
//!
//! Three things are handled here, once, so no tool has to think about them:
//!
//! * **The token lifecycle.** HelloAsso hands out 30-minute access tokens for a
//!   `client_credentials` grant, plus a refresh token. The client asks for one
//!   lazily, renews it a minute before it expires (refresh first, a new grant if
//!   the refresh is refused), and retries a request once on a 401, so a token
//!   revoked server-side never surfaces as a tool failure.
//! * **Retries.** 429 and the transient 5xx family are retried with backoff,
//!   honouring `Retry-After` when it is there.
//! * **Error envelopes.** HelloAsso answers errors in at least three shapes
//!   (its own `errors[]`, ASP.NET's validation problem, OAuth's `error`). They
//!   are all read into one [`Error::Api`] with a code and a sentence.

use std::time::Duration;

use reqwest::{Method, StatusCode, header};
use serde::{Deserialize, de::DeserializeOwned};
use serde_json::Value;
use tokio::{sync::Mutex, time::Instant};

use crate::{
    config::{Config, Credentials},
    error::{Error, Result},
};

/// The `User-Agent` every request carries.
pub const USER_AGENT: &str = concat!("mcp-helloasso/", env!("CARGO_PKG_VERSION"));

/// How long before its announced expiry a token is renewed.
const RENEW_MARGIN: Duration = Duration::from_secs(60);

/// A query string under construction: repeated keys are how HelloAsso reads
/// arrays (`states=Public&states=Private`).
pub type Query = Vec<(String, String)>;

/// One request to send, before authentication is added.
#[derive(Debug, Clone)]
pub struct Request {
    method: Method,
    path: String,
    query: Query,
    body: Option<Value>,
    headers: Vec<(String, String)>,
    accept: &'static str,
}

impl Request {
    /// A request for `path`, relative to the API root, for instance `/orders/42`.
    pub fn new(method: Method, path: impl Into<String>) -> Self {
        Self {
            method,
            path: path.into(),
            query: Vec::new(),
            body: None,
            headers: Vec::new(),
            accept: "application/json",
        }
    }

    /// A `GET`.
    pub fn get(path: impl Into<String>) -> Self {
        Self::new(Method::GET, path)
    }

    /// A `POST`.
    pub fn post(path: impl Into<String>) -> Self {
        Self::new(Method::POST, path)
    }

    /// A `PUT`.
    pub fn put(path: impl Into<String>) -> Self {
        Self::new(Method::PUT, path)
    }

    /// A `DELETE`.
    pub fn delete(path: impl Into<String>) -> Self {
        Self::new(Method::DELETE, path)
    }

    /// Add the query string.
    #[must_use]
    pub fn query(mut self, query: Query) -> Self {
        self.query = query;
        self
    }

    /// Add a JSON body.
    #[must_use]
    pub fn json(mut self, body: Value) -> Self {
        self.body = Some(body);
        self
    }

    /// Add a header, when there is a value for it.
    #[must_use]
    pub fn header_opt(mut self, name: &str, value: Option<String>) -> Self {
        if let Some(value) = value {
            self.headers.push((name.to_owned(), value));
        }
        self
    }

    /// Ask for something other than JSON back.
    #[must_use]
    pub fn accept(mut self, accept: &'static str) -> Self {
        self.accept = accept;
        self
    }

    /// `GET /orders/42`, for messages.
    pub fn label(&self) -> String {
        format!("{} {}", self.method, self.path)
    }
}

/// What a request answered, before decoding.
#[derive(Debug, Clone)]
pub struct Response {
    /// The body.
    pub bytes: Vec<u8>,
    /// The media type, as the server labelled it.
    pub content_type: String,
}

/// A client for one HelloAsso API client.
#[derive(Debug)]
pub struct HelloAsso {
    http: reqwest::Client,
    config: Config,
    token: Mutex<Option<Token>>,
}

#[derive(Debug, Clone)]
struct Token {
    access: String,
    refresh: Option<String>,
    expires_at: Option<Instant>,
}

impl Token {
    fn is_fresh(&self) -> bool {
        self.expires_at
            .is_none_or(|at| Instant::now() + RENEW_MARGIN < at)
    }
}

/// What the token endpoint answers.
#[derive(Debug, Deserialize)]
struct TokenResponse {
    access_token: String,
    refresh_token: Option<String>,
    expires_in: Option<u64>,
}

impl HelloAsso {
    /// Build a client from a configuration. No request is sent until the first
    /// call.
    pub fn new(config: Config) -> Result<Self> {
        let http = reqwest::Client::builder()
            .user_agent(USER_AGENT)
            .timeout(config.timeout)
            .build()
            .map_err(|e| Error::Transport(e.to_string()))?;
        let token = match &config.credentials {
            Credentials::Token(token) => Some(Token {
                access: token.expose().to_owned(),
                refresh: None,
                expires_at: None,
            }),
            Credentials::Client { .. } => None,
        };
        Ok(Self {
            http,
            config,
            token: Mutex::new(token),
        })
    }

    /// The configuration this client was built from.
    pub fn config(&self) -> &Config {
        &self.config
    }

    /// Send a request and decode its JSON answer. An empty body reads as
    /// `null`, which is what HelloAsso's write endpoints answer.
    pub async fn call<R: DeserializeOwned>(&self, request: Request) -> Result<R> {
        let label = request.label();
        let response = self.send(request).await?;
        let bytes = if response.bytes.iter().all(u8::is_ascii_whitespace) {
            b"null".as_slice()
        } else {
            response.bytes.as_slice()
        };
        serde_json::from_slice(bytes).map_err(|source| Error::Decode {
            endpoint: label,
            source,
        })
    }

    /// Send a request and return its raw body, whatever its type.
    pub async fn send(&self, request: Request) -> Result<Response> {
        let label = request.label();
        let url = format!("{}{}", self.config.base_url, request.path);
        let mut attempt = 0;
        let mut reauthenticated = false;
        loop {
            attempt += 1;
            let token = self.access_token().await?;
            tracing::debug!(endpoint = %label, attempt, "helloasso request");
            let mut builder = self
                .http
                .request(request.method.clone(), &url)
                .bearer_auth(&token)
                .header(header::ACCEPT, request.accept);
            if !request.query.is_empty() {
                builder = builder.query(&request.query);
            }
            for (name, value) in &request.headers {
                builder = builder.header(name.as_str(), value.as_str());
            }
            if let Some(body) = &request.body {
                builder = builder.json(body);
            }
            let response = match builder.send().await {
                Ok(response) => response,
                Err(e) => {
                    let err = Error::Transport(e.to_string());
                    if self.should_retry(&err, attempt) {
                        backoff(attempt, None).await;
                        continue;
                    }
                    return Err(err);
                }
            };
            let status = response.status();
            let retry_after = retry_after(&response);
            let content_type = response
                .headers()
                .get(header::CONTENT_TYPE)
                .and_then(|v| v.to_str().ok())
                .unwrap_or("application/octet-stream")
                .to_owned();
            let bytes = response
                .bytes()
                .await
                .map_err(|e| Error::Transport(e.to_string()))?
                .to_vec();
            if status.is_success() {
                return Ok(Response {
                    bytes,
                    content_type,
                });
            }
            // A token can be revoked before it expires; get a new one, once.
            if status == StatusCode::UNAUTHORIZED
                && !reauthenticated
                && matches!(self.config.credentials, Credentials::Client { .. })
            {
                reauthenticated = true;
                *self.token.lock().await = None;
                continue;
            }
            let err = api_error(&label, status, &bytes);
            if self.should_retry(&err, attempt) {
                tracing::warn!(endpoint = %label, attempt, status = status.as_u16(), "retrying");
                backoff(attempt, retry_after).await;
                continue;
            }
            return Err(err);
        }
    }

    /// A valid access token, obtained or renewed as needed.
    pub async fn access_token(&self) -> Result<String> {
        let mut slot = self.token.lock().await;
        if let Some(token) = slot.as_ref().filter(|t| t.is_fresh()) {
            return Ok(token.access.clone());
        }
        let Credentials::Client {
            client_id,
            client_secret,
        } = &self.config.credentials
        else {
            // A static token cannot be renewed; let the API say it expired.
            return slot
                .as_ref()
                .map(|t| t.access.clone())
                .ok_or_else(|| Error::Auth("no access token".into()));
        };

        let refreshed = match slot.as_ref().and_then(|t| t.refresh.clone()) {
            Some(refresh) => self
                .grant(&[
                    ("grant_type", "refresh_token"),
                    ("client_id", client_id),
                    ("refresh_token", &refresh),
                ])
                .await
                .inspect_err(|err| tracing::debug!(%err, "refresh refused, asking for a new grant"))
                .ok(),
            None => None,
        };
        let token = match refreshed {
            Some(token) => token,
            None => {
                self.grant(&[
                    ("grant_type", "client_credentials"),
                    ("client_id", client_id),
                    ("client_secret", client_secret.expose()),
                ])
                .await?
            }
        };
        let access = token.access.clone();
        *slot = Some(token);
        Ok(access)
    }

    async fn grant(&self, form: &[(&str, &str)]) -> Result<Token> {
        let mut attempt = 0;
        loop {
            attempt += 1;
            let response = self
                .http
                .post(&self.config.token_url)
                .header(header::ACCEPT, "application/json")
                .form(form)
                .send()
                .await;
            let response = match response {
                Ok(response) => response,
                Err(e) if attempt <= self.config.max_retries => {
                    tracing::warn!(error = %e, "token endpoint unreachable, retrying");
                    backoff(attempt, None).await;
                    continue;
                }
                Err(e) => return Err(Error::Auth(format!("token endpoint unreachable: {e}"))),
            };
            let status = response.status();
            let retry_after = retry_after(&response);
            let bytes = response
                .bytes()
                .await
                .map_err(|e| Error::Transport(e.to_string()))?;
            if status.is_success() {
                let parsed: TokenResponse = serde_json::from_slice(&bytes).map_err(|e| {
                    Error::Auth(format!(
                        "the token endpoint answered something unreadable: {e}"
                    ))
                })?;
                return Ok(Token {
                    access: parsed.access_token,
                    refresh: parsed.refresh_token,
                    expires_at: parsed
                        .expires_in
                        .map(|secs| Instant::now() + Duration::from_secs(secs)),
                });
            }
            if (status == StatusCode::TOO_MANY_REQUESTS || status.is_server_error())
                && attempt <= self.config.max_retries
            {
                backoff(attempt, retry_after).await;
                continue;
            }
            let detail = match api_error("POST /oauth2/token", status, &bytes) {
                Error::Api { code, message, .. } => [code, message]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(": "),
                other => other.to_string(),
            };
            return Err(Error::Auth(format!(
                "HelloAsso refused the credentials (HTTP {}){}. Check HELLOASSO_CLIENT_ID and HELLOASSO_CLIENT_SECRET, and HELLOASSO_ENVIRONMENT: a sandbox client does not work in production, nor the reverse.",
                status.as_u16(),
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(" — {detail}")
                }
            )));
        }
    }

    fn should_retry(&self, err: &Error, attempt: u32) -> bool {
        attempt <= self.config.max_retries
            && match err {
                Error::Transport(_) => true,
                Error::Api { status, .. } => matches!(status, 429 | 502 | 503 | 504),
                _ => false,
            }
    }
}

async fn backoff(attempt: u32, retry_after: Option<Duration>) {
    let wait = retry_after
        .unwrap_or_else(|| Duration::from_millis(500 * u64::from(attempt)))
        .min(Duration::from_secs(30));
    tokio::time::sleep(wait).await;
}

fn retry_after(response: &reqwest::Response) -> Option<Duration> {
    response
        .headers()
        .get(header::RETRY_AFTER)?
        .to_str()
        .ok()?
        .trim()
        .parse::<u64>()
        .ok()
        .map(Duration::from_secs)
}

/// Read whichever error envelope HelloAsso used, or fall back to the raw body.
pub(crate) fn api_error(endpoint: &str, status: StatusCode, body: &[u8]) -> Error {
    let parsed: Value = serde_json::from_slice(body).unwrap_or(Value::Null);
    let text = |v: &Value| {
        v.as_str()
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(str::to_owned)
    };

    let mut code = None;
    let mut messages: Vec<String> = Vec::new();

    // HelloAsso's own shape: {"errors":[{"code":"…","message":"…"}]}
    // ASP.NET's validation shape: {"title":"…","errors":{"field":["…"]}}
    match parsed.get("errors") {
        Some(Value::Array(errors)) => {
            for e in errors {
                if code.is_none() {
                    code = e.get("code").and_then(text);
                }
                if let Some(m) = e.get("message").and_then(text) {
                    messages.push(m);
                }
            }
        }
        Some(Value::Object(fields)) => {
            for (field, problems) in fields {
                let list: Vec<String> = problems
                    .as_array()
                    .map(|a| a.iter().filter_map(text).collect())
                    .unwrap_or_default();
                if !list.is_empty() {
                    messages.push(format!("{field}: {}", list.join(" ")));
                }
            }
        }
        _ => {}
    }
    // OAuth's shape: {"error":"invalid_client","error_description":"…"}
    if code.is_none() {
        code = parsed.get("error").and_then(text);
    }
    for key in ["error_description", "message", "detail", "title"] {
        if messages.is_empty()
            && let Some(m) = parsed.get(key).and_then(text)
        {
            messages.push(m);
        }
    }
    if messages.is_empty() && parsed.is_null() {
        let raw = String::from_utf8_lossy(body).trim().to_owned();
        if !raw.is_empty() {
            messages.push(truncate(&raw, 300));
        }
    }
    if messages.is_empty() {
        let hint = match status {
            StatusCode::UNAUTHORIZED => Some(
                "the access token was refused — check HELLOASSO_CLIENT_ID, HELLOASSO_CLIENT_SECRET and HELLOASSO_ENVIRONMENT",
            ),
            StatusCode::FORBIDDEN => Some(
                "this API client lacks the role or the privilege this endpoint requires (OrganizationAdmin, FormAdmin, AccessTransactions, RefundManagement, Checkout…): ask HelloAsso to grant it, or use a client of the organization itself",
            ),
            StatusCode::NOT_FOUND => Some("nothing at this address: check the slug or the id"),
            _ => None,
        };
        messages.extend(hint.map(str::to_owned));
    }
    // A 403 body says "insufficient Privileges" and nothing about which one;
    // the spec does, so say it.
    if status == StatusCode::FORBIDDEN
        && let Some(needed) = required_privilege(endpoint)
    {
        messages.push(needed.to_owned());
    }
    Error::Api {
        endpoint: endpoint.to_owned(),
        status: status.as_u16(),
        code,
        message: (!messages.is_empty()).then(|| messages.join("; ")),
    }
}

/// The privilege an endpoint needs that an association's own client lacks.
fn required_privilege(endpoint: &str) -> Option<&'static str> {
    let path = endpoint.split_once(' ').map_or(endpoint, |(_, p)| p);
    if path.starts_with("/directory/organizations") {
        Some(
            "this route needs the OrganizationOpenDirectory privilege, which association clients do not have: HelloAsso grants it to partners on request",
        )
    } else if path.starts_with("/directory/forms") || path.starts_with("/tags/") {
        Some(
            "this route needs the FormOpenDirectory privilege, which association clients do not have: HelloAsso grants it to partners on request",
        )
    } else if path.starts_with("/values/form/") {
        Some(
            "this route needs the FormAdministration privilege, which association clients do not have by default: ask HelloAsso for it",
        )
    } else if path.starts_with("/partners/") {
        Some(
            "partner routes need a partner API client, obtained from HelloAsso's partnership team (partenariats@helloasso.org); an association's client cannot call them",
        )
    } else if path.ends_with("/refund") || path.ends_with("/cancel") {
        Some(
            "refunds and cancellations need the RefundManagement privilege: ask HelloAsso to enable it on this client",
        )
    } else {
        None
    }
}

fn truncate(value: &str, max: usize) -> String {
    if value.chars().count() <= max {
        return value.to_owned();
    }
    let kept: String = value.chars().take(max).collect();
    format!("{kept}…")
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

    fn parts(err: Error) -> (u16, Option<String>, Option<String>) {
        match err {
            Error::Api {
                status,
                code,
                message,
                ..
            } => (status, code, message),
            other => panic!("expected an API error, got {other:?}"),
        }
    }

    #[test]
    fn helloasso_error_list_is_read() {
        let (status, code, message) = parts(api_error(
            "POST /payments/1/refund",
            StatusCode::CONFLICT,
            br#"{"errors":[{"code":"RefundAlreadyInProgress","message":"A refund is already in progress"}]}"#,
        ));
        assert_eq!(status, 409);
        assert_eq!(code.as_deref(), Some("RefundAlreadyInProgress"));
        assert_eq!(message.as_deref(), Some("A refund is already in progress"));
    }

    #[test]
    fn aspnet_validation_errors_are_read() {
        let (_, _, message) = parts(api_error(
            "POST /organizations/x/checkout-intents",
            StatusCode::BAD_REQUEST,
            br#"{"title":"One or more validation errors occurred.","errors":{"TotalAmount":["must equal the sum of the terms"]}}"#,
        ));
        assert_eq!(
            message.as_deref(),
            Some("TotalAmount: must equal the sum of the terms")
        );
    }

    #[test]
    fn oauth_errors_are_read() {
        let (_, code, message) = parts(api_error(
            "POST /oauth2/token",
            StatusCode::BAD_REQUEST,
            br#"{"error":"unauthorized_client","error_description":"Invalid client"}"#,
        ));
        assert_eq!(code.as_deref(), Some("unauthorized_client"));
        assert_eq!(message.as_deref(), Some("Invalid client"));
    }

    #[test]
    fn a_403_names_the_privilege_the_spec_requires() {
        let message = api_error(
            "POST /directory/forms",
            StatusCode::FORBIDDEN,
            b"Forbidden - insufficient Privileges",
        )
        .to_string();
        assert!(message.contains("insufficient Privileges"), "{message}");
        assert!(message.contains("FormOpenDirectory"), "{message}");
    }

    #[test]
    fn an_empty_403_still_says_what_is_missing() {
        let message =
            api_error("GET /organizations/x/payments", StatusCode::FORBIDDEN, b"").to_string();
        assert!(message.contains("privilege"), "{message}");
    }
}
