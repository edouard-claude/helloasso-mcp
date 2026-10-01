//! Forms: the campaigns an organization runs, from memberships to shops.

use rmcp::{ErrorData as McpError, Json, handler::server::wrapper::Parameters};
use serde::{Deserialize, Serialize};
use serde_json::{Map, Value};

use crate::{
    client::Request,
    error::Error,
    resolve::{FormRef, FormType},
    server::HelloAssoServer,
    tools::{ApiObject, Body, Done, Listing, PageArgs, QueryBuilder, ok, require_confirmation},
};

/// The publication state of a form.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
pub enum FormState {
    /// Visible, and findable on search engines.
    Public,
    /// Visible only to people who have the URL.
    Private,
    /// Not published yet.
    Draft,
    /// Deleted.
    Deleted,
    /// Disabled: visible to nobody, no longer taking orders.
    Disabled,
}

/// How a form is named: by its URL, or by its type and slug.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct FormArgs {
    /// The form's public URL, e.g.
    /// `https://www.helloasso.com/associations/{org}/evenements/{slug}`. Replaces
    /// the three fields below.
    pub form_url: Option<String>,
    /// The organization slug. Defaults to the configured organization.
    pub organization: Option<String>,
    /// The form type: Membership, Event, Donation, CrowdFunding, PaymentForm,
    /// Shop or Checkout (French and URL spellings accepted).
    pub form_type: Option<String>,
    /// The form slug, the last segment of its URL.
    pub form_slug: Option<String>,
}

impl HelloAssoServer {
    /// Resolve a [`FormArgs`] into a fully qualified form.
    pub(crate) async fn form_ref(&self, form: &FormArgs) -> Result<FormRef, Error> {
        self.resolver()
            .form(
                form.organization.as_deref(),
                form.form_type.as_deref(),
                form.form_slug.as_deref(),
                form.form_url.as_deref(),
            )
            .await
    }
}

/// Arguments of `list_forms`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListFormsArgs {
    /// The organization slug. Defaults to the configured organization.
    pub organization: Option<String>,
    /// Keep only forms in these states. All states when absent.
    pub states: Option<Vec<FormState>>,
    /// Keep only these form types: Membership, Event, Donation, CrowdFunding,
    /// PaymentForm, Shop, Checkout.
    pub form_types: Option<Vec<FormType>>,
    /// Pagination.
    #[serde(flatten)]
    pub page: PageArgs,
}

/// Arguments of `list_form_types`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListFormTypesArgs {
    /// The organization slug. Defaults to the configured organization.
    pub organization: Option<String>,
    /// Count only forms in these states.
    pub states: Option<Vec<FormState>>,
}

/// Arguments of `update_form_state`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct UpdateFormStateArgs {
    /// The form.
    #[serde(flatten)]
    pub form: FormArgs,
    /// The new state.
    pub state: FormState,
    /// Must be `true` to move a form to `Deleted`.
    pub confirm: Option<bool>,
}

/// Arguments of `list_activity_types`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ActivityTypesArgs {
    /// The form type whose subtypes to list.
    pub form_type: String,
}

/// A place, for an event.
#[derive(Debug, Clone, Default, Deserialize, Serialize, schemars::JsonSchema)]
pub struct Place {
    /// The venue's name.
    pub name: Option<String>,
    /// Street address.
    pub address: Option<String>,
    /// City.
    pub city: Option<String>,
    /// Postal code.
    pub zip_code: Option<String>,
    /// Three-letter country code. Defaults to FRA.
    pub country: Option<String>,
}

/// A price tier.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct Tier {
    /// What the payer sees, e.g. "Adult", "Reduced rate".
    pub label: String,
    /// The price in cents; 0 for a free tier.
    pub price_cents: i64,
}

/// Arguments of `quick_create_event`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct QuickCreateEventArgs {
    /// The organization slug. Defaults to the configured organization.
    pub organization: Option<String>,
    /// The event's title. It also becomes the URL slug, which cannot change.
    pub title: String,
    /// A short description.
    pub description: Option<String>,
    /// When the event starts, ISO 8601 datetime.
    pub start_date: Option<String>,
    /// When the event ends, ISO 8601 datetime.
    pub end_date: Option<String>,
    /// Where it happens.
    pub place: Option<Place>,
    /// The price tiers. Without any, HelloAsso creates the form without
    /// tickets.
    pub tiers: Option<Vec<Tier>>,
    /// When ticket sales open, ISO 8601 datetime. Defaults to publication.
    pub sale_start_date: Option<String>,
    /// When ticket sales close, ISO 8601 datetime. Defaults to the end.
    pub sale_end_date: Option<String>,
    /// The maximum number of entries for the whole event. Unlimited if absent.
    pub max_entries: Option<i64>,
    /// An activity subtype id, from `list_activity_types` with form_type Event.
    pub activity_type_id: Option<i64>,
    /// A title shown only in the back office.
    pub private_title: Option<String>,
    /// Any other field of HelloAsso's `FormQuickCreateRequest`, passed as is
    /// (`longDescription`, `generateTickets`, `contact`, `color`…).
    pub extra: Option<Map<String, Value>>,
}

#[allow(missing_docs, reason = "the router function is generated by the macro")]
#[rmcp::tool_router(router = forms_router, vis = "pub")]
impl HelloAssoServer {
    /// List an organization's forms (memberships, events, donations,
    /// crowdfunding, shops…), newest first, with their type, slug, state, dates
    /// and public URL.
    #[rmcp::tool(annotations(title = "List forms", read_only_hint = true, open_world_hint = true))]
    pub async fn list_forms(
        &self,
        Parameters(args): Parameters<ListFormsArgs>,
    ) -> std::result::Result<Json<Listing>, McpError> {
        let org = self
            .resolver()
            .organization(args.organization.as_deref())
            .await?;
        let query = QueryBuilder::new()
            .list("states", args.states.as_deref())
            .list("formTypes", args.form_types.as_deref())
            .paged(&args.page)
            .build();
        let page: Value = self
            .client()
            .call(Request::get(format!("/organizations/{org}/forms")).query(query))
            .await?;
        ok(Listing::from_value(page))
    }

    /// List the form types an organization has at least one form of, such as
    /// `["Membership", "Event"]`, optionally counting only some states.
    #[rmcp::tool(annotations(
        title = "List form types in use",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_form_types(
        &self,
        Parameters(args): Parameters<ListFormTypesArgs>,
    ) -> std::result::Result<Json<Listing>, McpError> {
        let org = self
            .resolver()
            .organization(args.organization.as_deref())
            .await?;
        let query = QueryBuilder::new()
            .list("states", args.states.as_deref())
            .build();
        let list: Value = self
            .client()
            .call(Request::get(format!("/organizations/{org}/formTypes")).query(query))
            .await?;
        ok(Listing::from_value(list))
    }

    /// Read a form's full public detail: title, descriptions, dates, place,
    /// tiers with their prices (in cents), custom fields, and widget URLs.
    #[rmcp::tool(annotations(title = "Get a form", read_only_hint = true, open_world_hint = true))]
    pub async fn get_form(
        &self,
        Parameters(form): Parameters<FormArgs>,
    ) -> std::result::Result<Json<ApiObject>, McpError> {
        let form = self.form_ref(&form).await?;
        let detail: Value = self
            .client()
            .call(Request::get(format!("{}/public", form.path())))
            .await?;
        ok(ApiObject::from_value(detail))
    }

    /// Read a form's statistics: total participants, and per tier and per
    /// option how many were sold and for how much (in cents). Needs the
    /// FormAdmin or OrganizationAdmin role.
    #[rmcp::tool(annotations(
        title = "Get form statistics",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_form_stats(
        &self,
        Parameters(form): Parameters<FormArgs>,
    ) -> std::result::Result<Json<ApiObject>, McpError> {
        let form = self.form_ref(&form).await?;
        let stats: Value = self
            .client()
            .call(Request::get(format!("{}/stats", form.path())))
            .await?;
        ok(ApiObject::from_value(stats))
    }

    /// List the activity subtypes HelloAsso offers for a form type (concert,
    /// sports tournament, training…), to pick an `activity_type_id`.
    #[rmcp::tool(annotations(
        title = "List activity types",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_activity_types(
        &self,
        Parameters(args): Parameters<ActivityTypesArgs>,
    ) -> std::result::Result<Json<Listing>, McpError> {
        let form_type = FormType::parse(&args.form_type)?;
        let list: Value = self
            .client()
            .call(Request::get(format!(
                "/values/form/{}/types",
                form_type.as_str()
            )))
            .await?;
        ok(Listing::from_value(list))
    }

    /// Create an event (billetterie) in one call, with a title, dates, a place
    /// and simple price tiers in cents. It can be refined afterwards in the
    /// HelloAsso back office. Answers the new form's slug and public URL.
    #[rmcp::tool(annotations(
        title = "Quick-create an event",
        read_only_hint = false,
        destructive_hint = false,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn quick_create_event(
        &self,
        Parameters(args): Parameters<QuickCreateEventArgs>,
    ) -> std::result::Result<Json<ApiObject>, McpError> {
        if args.title.trim().is_empty() {
            return Err(Error::invalid("title cannot be empty").into());
        }
        let org = self
            .resolver()
            .organization(args.organization.as_deref())
            .await?;
        let tiers: Option<Vec<Value>> = args.tiers.map(|tiers| {
            tiers
                .into_iter()
                .map(|t| serde_json::json!({ "label": t.label, "price": t.price_cents }))
                .collect()
        });
        if let Some(price) = tiers
            .iter()
            .flatten()
            .filter_map(|t| t["price"].as_i64())
            .find(|p| *p < 0)
        {
            return Err(Error::invalid(format!("a tier price cannot be negative: {price}")).into());
        }
        let place = args.place.map(|p| {
            Body::new()
                .opt("name", p.name)
                .opt("address", p.address)
                .opt("city", p.city)
                .opt("zipCode", p.zip_code)
                .set("country", p.country.unwrap_or_else(|| "FRA".to_owned()))
                .build()
        });
        let mut body = Body::new()
            .set("title", args.title.trim())
            .opt("description", args.description)
            .opt("startDate", args.start_date)
            .opt("endDate", args.end_date)
            .opt("place", place)
            .opt("tierList", tiers)
            .opt("saleStartDate", args.sale_start_date)
            .opt("saleEndDate", args.sale_end_date)
            .opt("maxEntries", args.max_entries)
            .opt("activityTypeId", args.activity_type_id)
            .opt("privateTitle", args.private_title)
            .build();
        if let (Some(extra), Some(object)) = (args.extra, body.as_object_mut()) {
            for (key, value) in extra {
                object.entry(key).or_insert(value);
            }
        }
        let created: Value = self
            .client()
            .call(
                Request::post(format!(
                    "/organizations/{org}/forms/Event/action/quick-create"
                ))
                .json(body),
            )
            .await?;
        ok(ApiObject::from_value(created))
    }

    /// Change a form's publication state: Public, Private, Draft, Disabled, or
    /// Deleted. Deleting needs `confirm: true`; the others are reversible.
    #[rmcp::tool(annotations(
        title = "Update a form's state",
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn update_form_state(
        &self,
        Parameters(args): Parameters<UpdateFormStateArgs>,
    ) -> std::result::Result<Json<Done>, McpError> {
        let form = self.form_ref(&args.form).await?;
        if args.state == FormState::Deleted {
            require_confirmation(
                args.confirm,
                &format!(
                    "the form {} ({}) would be deleted",
                    form.form_slug,
                    form.form_type.as_str()
                ),
            )?;
        }
        let answer: Value = self
            .client()
            .call(
                Request::put(format!("{}/state", form.path()))
                    .json(serde_json::json!({ "state": args.state })),
            )
            .await?;
        ok(Done::new(
            format!(
                "{} {} is now {:?}",
                form.form_type.as_str(),
                form.form_slug,
                args.state
            ),
            answer,
        ))
    }
}
