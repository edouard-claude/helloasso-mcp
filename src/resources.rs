//! MCP resources: the same data as the read tools, for clients that would
//! rather attach context than call a function.
//!
//! Three fixed URIs and two templates. Everything is JSON, everything is
//! read-only, and everything is scoped to the API client this process holds.

use rmcp::model::{ReadResourceResult, Resource, ResourceContents, ResourceTemplate};
use serde_json::Value;

use crate::{
    client::Request,
    error::{Error, Result},
    resolve::FormType,
    server::HelloAssoServer,
    tools::Listing,
};

/// The URI scheme every resource of this server lives under.
pub const SCHEME: &str = "helloasso://";

/// The default organization's public profile.
pub const URI_ORGANIZATION: &str = "helloasso://organization";
/// The default organization's forms.
pub const URI_FORMS: &str = "helloasso://forms";
/// The organizations this client holds a role on.
pub const URI_MY_ORGANIZATIONS: &str = "helloasso://my-organizations";

/// The resources a client can read without filling in anything.
pub fn catalogue() -> Vec<Resource> {
    vec![
        Resource::new(URI_ORGANIZATION, "organization")
            .with_title("Organization")
            .with_description("The default organization's public profile.")
            .with_mime_type("application/json"),
        Resource::new(URI_FORMS, "forms")
            .with_title("Forms")
            .with_description(
                "The default organization's forms (up to 100, newest first), with type, slug, state and URL.",
            )
            .with_mime_type("application/json"),
        Resource::new(URI_MY_ORGANIZATIONS, "my-organizations")
            .with_title("My organizations")
            .with_description("The organizations this API client holds a role on.")
            .with_mime_type("application/json"),
    ]
}

/// The resources a client reads by filling in a variable.
pub fn templates() -> Vec<ResourceTemplate> {
    vec![
        ResourceTemplate::new("helloasso://forms/{form_type}/{form_slug}", "form")
            .with_title("A form")
            .with_description(
                "One form of the default organization in full: tiers and prices in cents, custom fields, dates. form_type is Event, Membership, Donation, CrowdFunding, PaymentForm or Shop.",
            )
            .with_mime_type("application/json"),
        ResourceTemplate::new("helloasso://orders/{order_id}", "order")
            .with_title("An order")
            .with_description(
                "One order in full: payer, items with participants, payments. Amounts in cents.",
            )
            .with_mime_type("application/json"),
    ]
}

/// Read one resource by URI.
pub async fn read(server: &HelloAssoServer, uri: &str) -> Result<ReadResourceResult> {
    let client = server.client();
    let json: Value = match uri {
        URI_ORGANIZATION => {
            let org = server.resolver().organization(None).await?;
            client
                .call(Request::get(format!("/organizations/{org}")))
                .await?
        }
        URI_FORMS => {
            let org = server.resolver().organization(None).await?;
            let page: Value = client
                .call(
                    Request::get(format!("/organizations/{org}/forms"))
                        .query(vec![("pageSize".into(), "100".into())]),
                )
                .await?;
            serde_json::to_value(Listing::from_value(page)).map_err(|source| Error::Decode {
                endpoint: uri.to_owned(),
                source,
            })?
        }
        URI_MY_ORGANIZATIONS => Value::Array(server.resolver().my_organizations().await?),
        other => {
            let rest = other.strip_prefix(SCHEME).ok_or_else(|| unknown(other))?;
            let parts: Vec<&str> = rest.split('/').collect();
            match parts.as_slice() {
                ["forms", kind, slug] => {
                    let form = server
                        .resolver()
                        .form(
                            None,
                            Some(FormType::parse(kind)?.as_str()),
                            Some(slug),
                            None,
                        )
                        .await?;
                    client
                        .call(Request::get(format!("{}/public", form.path())))
                        .await?
                }
                ["orders", id] => {
                    let id: i64 = id
                        .parse()
                        .ok()
                        .filter(|n| *n > 0)
                        .ok_or_else(|| Error::invalid(format!("{id:?} is not an order id")))?;
                    client.call(Request::get(format!("/orders/{id}"))).await?
                }
                _ => return Err(unknown(other)),
            }
        }
    };

    let text = serde_json::to_string_pretty(&json).map_err(|source| Error::Decode {
        endpoint: uri.to_owned(),
        source,
    })?;
    Ok(ReadResourceResult::new(vec![
        ResourceContents::text(text, uri).with_mime_type("application/json"),
    ]))
}

fn unknown(uri: &str) -> Error {
    Error::not_found(format!(
        "no resource at {uri:?}. This server serves {URI_ORGANIZATION}, {URI_FORMS}, {URI_MY_ORGANIZATIONS}, helloasso://forms/{{form_type}}/{{form_slug}} and helloasso://orders/{{order_id}}."
    ))
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

    #[test]
    fn the_catalogue_and_the_templates_are_documented() {
        for resource in catalogue() {
            assert!(resource.uri.starts_with(SCHEME), "{}", resource.uri);
            assert!(resource.title.is_some(), "{} has no title", resource.uri);
            assert!(resource.description.is_some(), "{}", resource.uri);
            assert_eq!(resource.mime_type.as_deref(), Some("application/json"));
        }
        for template in templates() {
            assert!(template.uri_template.contains('{'), "no variable");
            assert!(template.description.is_some());
        }
    }

    #[test]
    fn an_unknown_uri_lists_what_exists() {
        let message = unknown("helloasso://nope").to_string();
        assert!(message.contains(URI_FORMS), "{message}");
        assert!(message.contains("orders/{order_id}"), "{message}");
    }
}
