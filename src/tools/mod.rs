//! The MCP tools, one module per toolset.
//!
//! Every HelloAsso v5 endpoint is reachable: 36 routes, one tool each, plus
//! `whoami` and `summarize_payments`, which exist because the API has no
//! "who am I" for an API client and no aggregate, and a treasurer needs both.
//!
//! Naming follows the same rule everywhere: `verb_noun`, and the doc comment of
//! the function is the description a model reads.

pub mod checkout;
pub mod directory;
pub mod forms;
pub mod organization;
pub mod partner;
pub mod sales;

use rmcp::{ErrorData as McpError, Json, handler::server::router::tool::ToolRouter};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    client::Query,
    config::{Config, Toolset},
    error::Error,
    server::HelloAssoServer,
};

/// Assemble the tool router the configuration asks for.
///
/// Read-only mode is not a list of tool names to keep in sync: it drops every
/// route that does not annotate itself `readOnlyHint`, so a new write tool is
/// excluded by construction.
pub fn router(config: &Config) -> ToolRouter<HelloAssoServer> {
    let mut router = ToolRouter::new();
    if config.has(Toolset::Organization) {
        router += HelloAssoServer::organization_router();
    }
    if config.has(Toolset::Forms) {
        router += HelloAssoServer::forms_router();
    }
    if config.has(Toolset::Sales) {
        router += HelloAssoServer::sales_router();
    }
    if config.has(Toolset::Checkout) {
        router += HelloAssoServer::checkout_router();
    }
    if config.has(Toolset::Directory) {
        router += HelloAssoServer::directory_router();
    }
    if config.has(Toolset::Partner) {
        router += HelloAssoServer::partner_router();
    }

    if config.read_only {
        let writers: Vec<String> = router
            .list_all()
            .into_iter()
            .filter(|tool| {
                !tool
                    .annotations
                    .as_ref()
                    .and_then(|a| a.read_only_hint)
                    .unwrap_or(false)
            })
            .map(|tool| tool.name.to_string())
            .collect();
        for name in writers {
            router.remove_route(&name);
        }
    }

    router
}

/// One HelloAsso object, as the API returned it.
///
/// The v5 models are large and HelloAsso adds fields without notice, so they
/// are passed through whole rather than squeezed into structs that would
/// silently drop what they do not know. Amounts are in **cents**.
#[derive(Debug, Clone, Default, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(transparent)]
pub struct ApiObject(pub Map<String, Value>);

impl ApiObject {
    /// Wrap whatever came back; a non-object is kept under `value`.
    pub fn from_value(value: Value) -> Self {
        match value {
            Value::Object(map) => Self(map),
            Value::Null => Self(Map::new()),
            other => {
                let mut map = Map::new();
                map.insert("value".into(), other);
                Self(map)
            }
        }
    }
}

/// What a list tool answers with.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Listing {
    /// How many objects this answer carries.
    pub returned: usize,
    /// How many objects match upstream, when HelloAsso says (it answers -1 on
    /// the directory and partner listings, which is left out here).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_count: Option<i64>,
    /// The page this answer is, from 1.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub page_index: Option<i64>,
    /// How many pages there are, when known.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub total_pages: Option<i64>,
    /// Pass this back as `continuation_token` to get the next page.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub continuation_token: Option<String>,
    /// The objects. Amounts are in cents.
    pub items: Vec<Value>,
    /// Set when the answer is worth a word of explanation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

impl Listing {
    /// Read HelloAsso's `{ data, pagination }` envelope, or a bare array.
    pub fn from_value(value: Value) -> Self {
        match value {
            Value::Array(items) => Self::all(items),
            Value::Null => Self::all(Vec::new()),
            Value::Object(mut map) => {
                let items = match map.remove("data") {
                    Some(Value::Array(items)) => items,
                    _ => Vec::new(),
                };
                let pagination = map.remove("pagination").unwrap_or(Value::Null);
                let int = |key: &str| {
                    pagination
                        .get(key)
                        .and_then(Value::as_i64)
                        .filter(|n| *n >= 0)
                };
                Self {
                    returned: items.len(),
                    total_count: int("totalCount"),
                    page_index: int("pageIndex"),
                    total_pages: int("totalPages"),
                    continuation_token: pagination
                        .get("continuationToken")
                        .and_then(Value::as_str)
                        .filter(|t| !t.is_empty())
                        .map(str::to_owned),
                    items,
                    note: None,
                }
            }
            other => Self::all(vec![other]),
        }
    }

    /// Wrap objects that were not paginated upstream.
    pub fn all(items: Vec<Value>) -> Self {
        Self {
            returned: items.len(),
            total_count: i64::try_from(items.len()).ok(),
            page_index: None,
            total_pages: None,
            continuation_token: None,
            items,
            note: None,
        }
    }

    /// Add the word of explanation.
    #[must_use]
    pub fn with_note(mut self, note: impl Into<String>) -> Self {
        self.note = Some(note.into());
        self
    }
}

/// What a write tool with an empty upstream answer reports.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct Done {
    /// Always true: a failure is an error, not a `false`.
    pub ok: bool,
    /// What was done, in one sentence.
    pub action: String,
    /// What HelloAsso answered, when it answered something.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub response: Option<Value>,
}

impl Done {
    /// Report a successful write.
    pub fn new(action: impl Into<String>, response: Value) -> Self {
        Self {
            ok: true,
            action: action.into(),
            response: (!response.is_null()).then_some(response),
        }
    }
}

/// Sort direction, as HelloAsso spells it.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, schemars::JsonSchema)]
pub enum SortOrder {
    /// Oldest first.
    Asc,
    /// Newest first, the default.
    Desc,
}

/// Which date a listing is sorted on.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, schemars::JsonSchema)]
pub enum SortField {
    /// The date of the operation, the default.
    Date,
    /// The date of the last update.
    UpdateDate,
    /// The creation date.
    CreationDate,
}

/// Pagination, the same on every list tool.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct PageArgs {
    /// How many objects per page. Default 20, maximum 100.
    pub page_size: Option<u32>,
    /// The page to read, from 1. Ignored when `continuation_token` is given.
    pub page_index: Option<u32>,
    /// The `continuation_token` of a previous answer, to read the next page.
    pub continuation_token: Option<String>,
}

/// The page size used when a tool asks for none.
pub const DEFAULT_PAGE_SIZE: u32 = 20;
/// The page size HelloAsso refuses to exceed.
pub const MAX_PAGE_SIZE: u32 = 100;

impl PageArgs {
    /// Fold the pagination into a query string.
    pub fn apply(&self, query: &mut QueryBuilder) {
        query.push(
            "pageSize",
            self.page_size
                .unwrap_or(DEFAULT_PAGE_SIZE)
                .clamp(1, MAX_PAGE_SIZE),
        );
        match self.continuation_token.as_deref().map(str::trim) {
            Some(token) if !token.is_empty() => query.push("continuationToken", token),
            _ => {
                if let Some(index) = self.page_index {
                    query.push("pageIndex", index.max(1));
                }
            }
        }
    }
}

/// A query string under construction. Absent arguments stay absent.
#[derive(Debug, Default)]
pub struct QueryBuilder(Query);

impl QueryBuilder {
    /// An empty query.
    pub fn new() -> Self {
        Self(Vec::new())
    }

    /// Add one pair.
    #[allow(
        clippy::needless_pass_by_value,
        reason = "callers pass literals and owned values alike"
    )]
    pub fn push(&mut self, key: &str, value: impl ToString) {
        self.0.push((key.to_owned(), value.to_string()));
    }

    /// Add a pair when there is a value.
    #[must_use]
    pub fn opt(mut self, key: &str, value: Option<impl ToString>) -> Self {
        if let Some(value) = value {
            self.push(key, value);
        }
        self
    }

    /// Add an enum by its API spelling.
    #[must_use]
    pub fn opt_enum<T: Serialize>(self, key: &str, value: Option<T>) -> Self {
        let value = value.map(|v| api_name(&v));
        self.opt(key, value)
    }

    /// Add a list as repeated keys, which is how HelloAsso reads arrays.
    #[must_use]
    pub fn list<T: Serialize>(mut self, key: &str, values: Option<&[T]>) -> Self {
        for value in values.unwrap_or_default() {
            self.push(key, api_name(value));
        }
        self
    }

    /// Fold pagination in.
    #[must_use]
    pub fn paged(mut self, page: &PageArgs) -> Self {
        page.apply(&mut self);
        self
    }

    /// The query, ready to send.
    pub fn build(self) -> Query {
        self.0
    }
}

/// An enum's spelling on the wire, which is its serde name.
pub fn api_name<T: Serialize>(value: &T) -> String {
    match serde_json::to_value(value) {
        Ok(Value::String(s)) => s,
        Ok(other) => other.to_string(),
        Err(_) => String::new(),
    }
}

/// A JSON body under construction. Absent arguments stay absent.
#[derive(Debug, Default)]
pub struct Body(Map<String, Value>);

impl Body {
    /// An empty body.
    pub fn new() -> Self {
        Self(Map::new())
    }

    /// Set a field.
    #[must_use]
    pub fn set(mut self, key: &str, value: impl Into<Value>) -> Self {
        self.0.insert(key.to_owned(), value.into());
        self
    }

    /// Set a field, when there is something to set.
    #[must_use]
    pub fn opt<V: Into<Value>>(mut self, key: &str, value: Option<V>) -> Self {
        if let Some(value) = value {
            self.0.insert(key.to_owned(), value.into());
        }
        self
    }

    /// Set a field from anything serializable, when there is something to set.
    #[must_use]
    pub fn opt_ser<V: Serialize>(mut self, key: &str, value: Option<&V>) -> Self {
        if let Some(value) = value.and_then(|v| serde_json::to_value(v).ok())
            && !value.is_null()
        {
            self.0.insert(key.to_owned(), value);
        }
        self
    }

    /// The body, ready to send.
    pub fn build(self) -> Value {
        Value::Object(self.0)
    }
}

/// Wrap a value as a structured tool result.
pub fn ok<T>(value: T) -> Result<Json<T>, McpError>
where
    T: Serialize + schemars::JsonSchema + 'static,
{
    Ok(Json(value))
}

/// Refuse an irreversible call that was not confirmed.
///
/// Refunds, cancellations and webhook deletions move money or break an
/// integration, so they are fail-closed: without `confirm: true` nothing
/// leaves the process.
pub fn require_confirmation(confirm: Option<bool>, what: &str) -> Result<(), McpError> {
    if confirm == Some(true) {
        return Ok(());
    }
    Err(Error::invalid(format!(
        "{what}, and HelloAsso has no undo. Show the person exactly what is about to happen, get their explicit agreement, then call again with confirm: true."
    ))
    .into())
}

/// Read a positive identifier.
pub fn positive(kind: &str, id: i64) -> Result<i64, McpError> {
    if id > 0 {
        Ok(id)
    } else {
        Err(Error::invalid(format!("{kind} must be a positive integer, got {id}")).into())
    }
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
    use crate::config::Config;

    fn config(pairs: &[(&str, &str)]) -> Config {
        let mut owned: Vec<(String, String)> = vec![
            ("HELLOASSO_CLIENT_ID".into(), "id".into()),
            ("HELLOASSO_CLIENT_SECRET".into(), "secret".into()),
        ];
        owned.extend(
            pairs
                .iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned())),
        );
        Config::from_getter(|key| owned.iter().find(|(k, _)| k == key).map(|(_, v)| v.clone()))
            .expect("test configuration")
    }

    fn tool_names(router: &ToolRouter<HelloAssoServer>) -> Vec<String> {
        router
            .list_all()
            .into_iter()
            .map(|t| t.name.to_string())
            .collect()
    }

    #[test]
    fn every_endpoint_of_the_api_has_a_tool() {
        let router = router(&config(&[]));
        // 36 documented routes, plus whoami and summarize_payments.
        assert_eq!(router.list_all().len(), 38, "{:?}", tool_names(&router));
    }

    #[test]
    fn a_toolset_selection_only_exposes_that_toolset() {
        let names = tool_names(&router(&config(&[("HELLOASSO_TOOLSETS", "sales")])));
        assert!(names.contains(&"list_payments".to_owned()));
        assert!(!names.contains(&"create_checkout_intent".to_owned()));
        assert!(!names.contains(&"set_notification_url".to_owned()));
    }

    #[test]
    fn read_only_mode_keeps_only_tools_that_declare_themselves_read_only() {
        let router = router(&config(&[("HELLOASSO_READ_ONLY", "1")]));
        for tool in router.list_all() {
            assert_eq!(
                tool.annotations.as_ref().and_then(|a| a.read_only_hint),
                Some(true),
                "{} survived read-only mode",
                tool.name
            );
        }
        let names = tool_names(&router);
        assert!(names.contains(&"list_orders".to_owned()));
        assert!(!names.contains(&"refund_payment".to_owned()));
        assert!(!names.contains(&"cancel_order".to_owned()));
        assert!(!names.contains(&"create_checkout_intent".to_owned()));
    }

    #[test]
    fn every_tool_is_named_annotated_and_described() {
        let router = router(&config(&[]));
        for tool in router.list_all() {
            let annotations = tool
                .annotations
                .as_ref()
                .unwrap_or_else(|| panic!("{} has no annotations", tool.name));
            assert!(annotations.title.is_some(), "{} has no title", tool.name);
            assert!(
                annotations.read_only_hint.is_some(),
                "{} does not say whether it writes",
                tool.name
            );
            let description = tool
                .description
                .as_deref()
                .unwrap_or_else(|| panic!("{} has no description", tool.name));
            assert!(
                description.len() > 40,
                "{} has a description too short to be useful: {description:?}",
                tool.name
            );
            assert!(
                tool.name
                    .chars()
                    .all(|c| c.is_ascii_lowercase() || c == '_'),
                "{} is not snake_case",
                tool.name
            );
            assert!(
                tool.output_schema.is_some(),
                "{} has no output schema",
                tool.name
            );
        }
    }

    #[test]
    fn money_moving_tools_are_marked_destructive() {
        let router = router(&config(&[]));
        for name in [
            "refund_payment",
            "cancel_order",
            "delete_notification_url",
            "delete_organization_notification_url",
        ] {
            let tool = router
                .list_all()
                .into_iter()
                .find(|t| t.name == name)
                .unwrap_or_else(|| panic!("{name} is missing"));
            assert_eq!(
                tool.annotations.as_ref().and_then(|a| a.destructive_hint),
                Some(true),
                "{name} is not marked destructive"
            );
        }
    }

    #[test]
    fn an_unconfirmed_refund_says_what_to_do() {
        let err = require_confirmation(None, "this payment would be refunded").unwrap_err();
        assert!(err.message.contains("confirm: true"), "{}", err.message);
        assert!(require_confirmation(Some(false), "x").is_err());
        assert!(require_confirmation(Some(true), "x").is_ok());
    }

    #[test]
    fn the_pagination_envelope_is_read_and_minus_one_dropped() {
        let listing = Listing::from_value(serde_json::json!({
            "data": [{"id": 1}, {"id": 2}],
            "pagination": {"pageSize": 20, "totalCount": -1, "pageIndex": 1, "totalPages": -1, "continuationToken": "abc"}
        }));
        assert_eq!(listing.returned, 2);
        assert_eq!(listing.total_count, None);
        assert_eq!(listing.page_index, Some(1));
        assert_eq!(listing.continuation_token.as_deref(), Some("abc"));
    }

    #[test]
    fn arrays_become_repeated_keys_and_tokens_win_over_indexes() {
        let query = QueryBuilder::new()
            .list("states", Some(&[SortOrder::Asc, SortOrder::Desc][..]))
            .paged(&PageArgs {
                page_size: Some(500),
                page_index: Some(3),
                continuation_token: Some("tok".into()),
            })
            .build();
        assert_eq!(
            query,
            vec![
                ("states".to_owned(), "Asc".to_owned()),
                ("states".to_owned(), "Desc".to_owned()),
                ("pageSize".to_owned(), "100".to_owned()),
                ("continuationToken".to_owned(), "tok".to_owned()),
            ]
        );
    }
}
