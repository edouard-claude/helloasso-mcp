//! From what a person types to what the API wants: organization slugs, form
//! types and form slugs.
//!
//! People paste URLs. `https://www.helloasso.com/associations/les-amis-du-velo/evenements/gala-2026`
//! names an organization, a form type and a form at once, so every tool that
//! takes a form accepts that URL as well as the three parts separately.

use std::{sync::Arc, time::Duration};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::{sync::Mutex, time::Instant};

use crate::{
    client::{HelloAsso, Request},
    error::{Candidate, Error, Result},
};

/// How long the list of reachable organizations is reused.
const ORGANIZATIONS_TTL: Duration = Duration::from_secs(300);

/// The kinds of form HelloAsso knows.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
pub enum FormType {
    /// A fundraising campaign with a goal.
    CrowdFunding,
    /// Memberships (adhésions).
    Membership,
    /// Events, with tickets (billetterie).
    Event,
    /// Donation forms.
    Donation,
    /// Free-form payment forms (ventes).
    PaymentForm,
    /// Payments started through the checkout API.
    Checkout,
    /// Shops (boutiques).
    Shop,
}

impl FormType {
    /// Every form type, in HelloAsso's order.
    pub const ALL: [Self; 7] = [
        Self::CrowdFunding,
        Self::Membership,
        Self::Event,
        Self::Donation,
        Self::PaymentForm,
        Self::Checkout,
        Self::Shop,
    ];

    /// The spelling the API uses in paths and filters.
    pub fn as_str(self) -> &'static str {
        match self {
            Self::CrowdFunding => "CrowdFunding",
            Self::Membership => "Membership",
            Self::Event => "Event",
            Self::Donation => "Donation",
            Self::PaymentForm => "PaymentForm",
            Self::Checkout => "Checkout",
            Self::Shop => "Shop",
        }
    }

    /// Read a form type in any reasonable spelling, English or French, or as the
    /// path segment of a public HelloAsso URL.
    pub fn parse(raw: &str) -> Result<Self> {
        let key: String = raw
            .trim()
            .to_lowercase()
            .chars()
            .filter(|c| c.is_alphanumeric())
            .collect();
        let found = match key.as_str() {
            "crowdfunding" | "collecte" | "collectes" | "fundraising" => Self::CrowdFunding,
            "membership" | "memberships" | "adhesion" | "adhesions" | "adhésion" | "adhésions"
            | "cotisation" => Self::Membership,
            "event" | "events" | "evenement" | "evenements" | "événement" | "événements"
            | "billetterie" | "ticketing" => Self::Event,
            "donation" | "donations" | "don" | "dons" | "formulaire" | "formulaires" => {
                Self::Donation
            }
            "paymentform" | "payment" | "paiement" | "paiements" | "vente" | "ventes" => {
                Self::PaymentForm
            }
            "checkout" | "checkouts" => Self::Checkout,
            "shop" | "shops" | "boutique" | "boutiques" => Self::Shop,
            _ => {
                return Err(Error::invalid(format!(
                    "{raw:?} is not a form type. Use one of CrowdFunding, Membership, Event, Donation, PaymentForm, Checkout, Shop."
                )));
            }
        };
        Ok(found)
    }
}

/// A form, fully qualified.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, schemars::JsonSchema)]
pub struct FormRef {
    /// The organization slug.
    pub organization: String,
    /// The form type.
    pub form_type: FormType,
    /// The form slug.
    pub form_slug: String,
}

impl FormRef {
    /// `/organizations/{org}/forms/{type}/{slug}`, the prefix of every form route.
    pub fn path(&self) -> String {
        format!(
            "/organizations/{}/forms/{}/{}",
            self.organization,
            self.form_type.as_str(),
            self.form_slug
        )
    }
}

/// What a public HelloAsso URL names.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ParsedUrl {
    /// The organization slug.
    pub organization: Option<String>,
    /// The form type, when the URL points at a form.
    pub form_type: Option<FormType>,
    /// The form slug, when the URL points at a form.
    pub form_slug: Option<String>,
}

/// Read a `helloasso.com/associations/{org}[/{type}/{form}]` URL.
pub fn parse_url(raw: &str) -> Option<ParsedUrl> {
    let raw = raw.trim();
    if !raw.contains("helloasso") || !raw.contains('/') {
        return None;
    }
    let path = raw
        .split_once("://")
        .map_or(raw, |(_, rest)| rest)
        .split(['?', '#'])
        .next()
        .unwrap_or("");
    let segments: Vec<&str> = path.split('/').filter(|s| !s.is_empty()).skip(1).collect();
    let anchor = segments
        .iter()
        .position(|s| matches!(*s, "associations" | "organizations" | "organisations"))?;
    let organization = segments.get(anchor + 1).map(|s| (*s).to_owned())?;
    let mut parsed = ParsedUrl {
        organization: Some(organization),
        ..ParsedUrl::default()
    };
    if let (Some(kind), Some(slug)) = (segments.get(anchor + 2), segments.get(anchor + 3))
        && let Ok(form_type) = FormType::parse(kind)
    {
        parsed.form_type = Some(form_type);
        parsed.form_slug = Some((*slug).to_owned());
    }
    Some(parsed)
}

/// Refuse anything that could escape its path segment.
pub fn segment(kind: &str, raw: &str) -> Result<String> {
    let value = raw.trim();
    if value.is_empty()
        || value.len() > 250
        || !value
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
    {
        return Err(Error::invalid(format!(
            "{value:?} is not a valid {kind}: expected lowercase letters, digits and dashes, as in the HelloAsso URL"
        )));
    }
    Ok(value.to_owned())
}

/// The organization and form resolver, with its small cache.
#[derive(Debug)]
pub struct Resolver {
    client: Arc<HelloAsso>,
    organizations: Mutex<Option<(Instant, Vec<Value>)>>,
}

impl Resolver {
    /// Wrap a client.
    pub fn new(client: Arc<HelloAsso>) -> Self {
        Self {
            client,
            organizations: Mutex::new(None),
        }
    }

    /// The HTTP client.
    pub fn client(&self) -> &Arc<HelloAsso> {
        &self.client
    }

    /// The organizations this API client can act on, as `GET /users/me/organizations`
    /// lists them, cached for a few minutes.
    pub async fn my_organizations(&self) -> Result<Vec<Value>> {
        let mut slot = self.organizations.lock().await;
        if let Some((at, list)) = slot.as_ref()
            && at.elapsed() < ORGANIZATIONS_TTL
        {
            return Ok(list.clone());
        }
        let list: Vec<Value> = self
            .client
            .call::<Option<Vec<Value>>>(Request::get("/users/me/organizations"))
            .await?
            .unwrap_or_default();
        *slot = Some((Instant::now(), list.clone()));
        Ok(list)
    }

    /// Resolve an organization argument: a slug, a HelloAsso URL, or nothing,
    /// in which case the configured default, or the only organization this
    /// client can reach, is used.
    pub async fn organization(&self, raw: Option<&str>) -> Result<String> {
        if let Some(raw) = raw.map(str::trim).filter(|r| !r.is_empty()) {
            if let Some(slug) = parse_url(raw).and_then(|u| u.organization) {
                return segment("organization slug", &slug);
            }
            return segment("organization slug", raw);
        }
        if let Some(default) = &self.client.config().default_organization {
            return Ok(default.clone());
        }
        let reachable = self.my_organizations().await.map_err(|err| {
            Error::invalid(format!(
                "no organization given and none configured: pass `organization` (the slug in the HelloAsso URL), or set HELLOASSO_ORGANIZATION. Listing the organizations this client can reach failed too: {err}"
            ))
        })?;
        let slugs: Vec<Candidate> = reachable
            .iter()
            .filter_map(|o| {
                Some(Candidate {
                    id: o.get("organizationSlug")?.as_str()?.to_owned(),
                    name: o
                        .get("name")
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_owned(),
                })
            })
            .collect();
        match slugs.as_slice() {
            [only] => Ok(only.id.clone()),
            [] => Err(Error::invalid(
                "no organization given, none configured, and this API client reaches none: pass `organization` or set HELLOASSO_ORGANIZATION",
            )),
            _ => Err(Error::Ambiguous {
                message: format!(
                    "this API client reaches {} organizations: pass `organization` with one of the slugs below, or set HELLOASSO_ORGANIZATION",
                    slugs.len()
                ),
                candidates: slugs,
            }),
        }
    }

    /// Resolve a form from a URL, or from its type and slug.
    pub async fn form(
        &self,
        organization: Option<&str>,
        form_type: Option<&str>,
        form_slug: Option<&str>,
        form_url: Option<&str>,
    ) -> Result<FormRef> {
        // A URL pasted into any of the fields is the most complete answer.
        let url = [form_url, form_slug, organization]
            .into_iter()
            .flatten()
            .find_map(parse_url)
            .filter(|u| u.form_slug.is_some());
        if let Some(ParsedUrl {
            organization: Some(org),
            form_type: Some(kind),
            form_slug: Some(slug),
        }) = url
        {
            return Ok(FormRef {
                organization: segment("organization slug", &org)?,
                form_type: kind,
                form_slug: segment("form slug", &slug)?,
            });
        }
        if form_url.is_some() {
            return Err(Error::invalid(
                "form_url does not look like a HelloAsso form URL, which reads https://www.helloasso.com/associations/{organization}/{type}/{form}",
            ));
        }
        let form_type = form_type.ok_or_else(|| {
            Error::invalid("name the form: either form_url, or form_type with form_slug")
        })?;
        let form_slug = form_slug.ok_or_else(|| {
            Error::invalid("name the form: either form_url, or form_type with form_slug")
        })?;
        Ok(FormRef {
            organization: self.organization(organization).await?,
            form_type: FormType::parse(form_type)?,
            form_slug: segment("form slug", form_slug)?,
        })
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

    #[test]
    fn form_types_read_in_english_french_and_url_spelling() {
        assert_eq!(FormType::parse("event").unwrap(), FormType::Event);
        assert_eq!(FormType::parse("Événements").unwrap(), FormType::Event);
        assert_eq!(FormType::parse("adhesions").unwrap(), FormType::Membership);
        assert_eq!(
            FormType::parse("Payment Form").unwrap(),
            FormType::PaymentForm
        );
        assert_eq!(FormType::parse("boutiques").unwrap(), FormType::Shop);
        assert!(FormType::parse("raffle").is_err());
    }

    #[test]
    fn a_form_url_names_all_three_parts() {
        let parsed = parse_url(
            "https://www.helloasso.com/associations/les-amis-du-velo/evenements/gala-2026?utm=x",
        )
        .unwrap();
        assert_eq!(parsed.organization.as_deref(), Some("les-amis-du-velo"));
        assert_eq!(parsed.form_type, Some(FormType::Event));
        assert_eq!(parsed.form_slug.as_deref(), Some("gala-2026"));
    }

    #[test]
    fn an_organization_url_names_the_organization() {
        let parsed = parse_url("helloasso.com/associations/les-amis-du-velo").unwrap();
        assert_eq!(parsed.organization.as_deref(), Some("les-amis-du-velo"));
        assert_eq!(parsed.form_slug, None);
        assert_eq!(parse_url("les-amis-du-velo"), None);
    }

    #[test]
    fn a_segment_cannot_escape_its_path() {
        assert!(segment("form slug", "../partners/me").is_err());
        assert!(segment("form slug", "a/b").is_err());
        assert!(segment("form slug", "gala-2026").is_ok());
    }
}
