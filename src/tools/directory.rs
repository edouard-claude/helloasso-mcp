//! HelloAsso's public directory: every organization and form that chose to be
//! listed, and the tags that classify them.
//!
//! The two search endpoints are built for synchronisation, not search: results
//! come oldest-update first, there is no total, and the `continuation_token`
//! is how you walk on. They need the `OrganizationOpenDirectory` and
//! `FormOpenDirectory` privileges, which a plain association client lacks.

use std::fmt::Write as _;

use rmcp::{ErrorData as McpError, Json, handler::server::wrapper::Parameters};
use serde::Deserialize;
use serde_json::Value;

use crate::{
    client::Request,
    error::Error,
    resolve::FormType,
    server::HelloAssoServer,
    tools::{ApiObject, Body, Listing, QueryBuilder, ok, organization::NoArgs},
};

/// Pagination of the directory, which only knows continuation tokens.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct DirectoryPage {
    /// How many results per page. Default 20, maximum 100.
    pub page_size: Option<u32>,
    /// The `continuation_token` of a previous answer, to read on.
    pub continuation_token: Option<String>,
}

impl DirectoryPage {
    fn query(&self) -> Vec<(String, String)> {
        QueryBuilder::new()
            .opt("pageSize", Some(self.page_size.unwrap_or(20).clamp(1, 100)))
            .opt(
                "continuationToken",
                self.continuation_token
                    .as_deref()
                    .filter(|t| !t.trim().is_empty()),
            )
            .build()
    }
}

/// Arguments of `search_organizations`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct SearchOrganizationsArgs {
    /// Text to find in the organization's name.
    pub name: Option<String>,
    /// Text to find in its description.
    pub description: Option<String>,
    /// Categories, as `list_organization_categories` names them.
    pub categories: Option<Vec<String>>,
    /// Organization types, e.g. `Association1901`, `FondDotation`.
    pub types: Option<Vec<String>>,
    /// Postal codes.
    pub zip_codes: Option<Vec<String>>,
    /// Cities.
    pub cities: Option<Vec<String>>,
    /// Regions.
    pub regions: Option<Vec<String>>,
    /// Departments.
    pub departments: Option<Vec<String>>,
    /// Keep only organizations entitled to issue tax receipts.
    pub fiscal_receipt_eligibility: Option<bool>,
    /// Public tags.
    pub tags: Option<Vec<String>>,
    /// Internal tags (special operations only).
    pub internal_tags: Option<Vec<String>>,
    /// Linked partners.
    pub linked_partners: Option<Vec<String>>,
    /// Pagination.
    #[serde(flatten)]
    pub page: DirectoryPage,
}

/// Arguments of `search_forms`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct SearchFormsArgs {
    /// Text to find in the form's name.
    pub form_name: Option<String>,
    /// Text to find in the form's description.
    pub form_description: Option<String>,
    /// Form types.
    pub form_types: Option<Vec<FormType>>,
    /// Activity types, as `list_activity_types` names them.
    pub activity_types: Option<Vec<String>>,
    /// Postal codes where the forms take place.
    pub zip_codes: Option<Vec<String>>,
    /// Cities where the forms take place.
    pub cities: Option<Vec<String>>,
    /// Regions where the forms take place.
    pub regions: Option<Vec<String>>,
    /// Departments where the forms take place.
    pub departments: Option<Vec<String>>,
    /// Countries where the forms take place.
    pub countries: Option<Vec<String>>,
    /// Earliest start date, inclusive (ISO datetime).
    pub start_date_min: Option<String>,
    /// Latest start date, exclusive.
    pub start_date_max: Option<String>,
    /// Earliest end date, inclusive.
    pub end_date_min: Option<String>,
    /// Latest end date, exclusive.
    pub end_date_max: Option<String>,
    /// Earliest publication date, inclusive.
    pub publication_start_date_min: Option<String>,
    /// Latest publication date, exclusive.
    pub publication_start_date_max: Option<String>,
    /// Keep only free forms.
    pub is_free: Option<bool>,
    /// Keep only forms with entries left.
    pub has_remaining_entries: Option<bool>,
    /// Public tags of the form.
    pub public_tags: Option<Vec<String>>,
    /// Internal tags of the form (special operations only).
    pub internal_tags: Option<Vec<String>>,
    /// Text to find in the organization's name.
    pub organization_name: Option<String>,
    /// Text to find in the organization's description.
    pub organization_description: Option<String>,
    /// Organization categories.
    pub organization_categories: Option<Vec<String>>,
    /// Organization types.
    pub organization_types: Option<Vec<String>>,
    /// Organization postal codes.
    pub organization_zip_codes: Option<Vec<String>>,
    /// Organization cities.
    pub organization_cities: Option<Vec<String>>,
    /// Organization regions.
    pub organization_regions: Option<Vec<String>>,
    /// Organization departments.
    pub organization_departments: Option<Vec<String>>,
    /// Keep only organizations entitled to issue tax receipts.
    pub organization_fiscal_receipt_eligibility: Option<bool>,
    /// Organizations' linked partners.
    pub organization_linked_partners: Option<Vec<String>>,
    /// Pagination.
    #[serde(flatten)]
    pub page: DirectoryPage,
}

/// Arguments of `get_tag`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct GetTagArgs {
    /// The tag's name.
    pub tag_name: String,
    /// Count how many forms use it.
    pub with_count: Option<bool>,
    /// Total what forms carrying it collected, in cents.
    pub with_amount: Option<bool>,
}

fn list(values: Option<Vec<String>>) -> Option<Vec<String>> {
    values.filter(|v| !v.is_empty())
}

#[allow(missing_docs, reason = "the router function is generated by the macro")]
#[rmcp::tool_router(router = directory_router, vis = "pub")]
impl HelloAssoServer {
    /// Search HelloAsso's public directory of organizations by name, place,
    /// category, type or tag. Needs the OrganizationOpenDirectory privilege.
    #[rmcp::tool(annotations(
        title = "Search the organization directory",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn search_organizations(
        &self,
        Parameters(args): Parameters<SearchOrganizationsArgs>,
    ) -> std::result::Result<Json<Listing>, McpError> {
        let body = Body::new()
            .opt("name", args.name)
            .opt("description", args.description)
            .opt("categories", list(args.categories))
            .opt("types", list(args.types))
            .opt("zipCodes", list(args.zip_codes))
            .opt("cities", list(args.cities))
            .opt("regions", list(args.regions))
            .opt("departments", list(args.departments))
            .opt("fiscalReceiptEligibility", args.fiscal_receipt_eligibility)
            .opt("tags", list(args.tags))
            .opt("internalTags", list(args.internal_tags))
            .opt("linkedPartners", list(args.linked_partners))
            .build();
        let page: Value = self
            .client()
            .call(
                Request::post("/directory/organizations")
                    .query(args.page.query())
                    .json(body),
            )
            .await?;
        ok(Listing::from_value(page))
    }

    /// Search HelloAsso's public directory of forms (events, donations,
    /// memberships…) by text, place, dates, type, price or tag. Needs the
    /// FormOpenDirectory privilege.
    #[rmcp::tool(annotations(
        title = "Search the form directory",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn search_forms(
        &self,
        Parameters(args): Parameters<SearchFormsArgs>,
    ) -> std::result::Result<Json<Listing>, McpError> {
        let body = Body::new()
            .opt("formName", args.form_name)
            .opt("formDescription", args.form_description)
            .opt_ser(
                "formTypes",
                args.form_types.as_ref().filter(|v| !v.is_empty()),
            )
            .opt("formActivityType", list(args.activity_types))
            .opt("formZipCodes", list(args.zip_codes))
            .opt("formCities", list(args.cities))
            .opt("formRegions", list(args.regions))
            .opt("formDepartments", list(args.departments))
            .opt("formCountries", list(args.countries))
            .opt("formStartDateMin", args.start_date_min)
            .opt("formStartDateMax", args.start_date_max)
            .opt("formEndDateMin", args.end_date_min)
            .opt("formEndDateMax", args.end_date_max)
            .opt(
                "formPublicationStartDateMin",
                args.publication_start_date_min,
            )
            .opt(
                "formPublicationStartDateMax",
                args.publication_start_date_max,
            )
            .opt("formIsFree", args.is_free)
            .opt("formHasRemainingEntries", args.has_remaining_entries)
            .opt("formPublicTags", list(args.public_tags))
            .opt("formInternalTags", list(args.internal_tags))
            .opt("organizationName", args.organization_name)
            .opt("organizationDescription", args.organization_description)
            .opt("organizationCategories", list(args.organization_categories))
            .opt("organizationTypes", list(args.organization_types))
            .opt("organizationZipCodes", list(args.organization_zip_codes))
            .opt("organizationCities", list(args.organization_cities))
            .opt("organizationRegions", list(args.organization_regions))
            .opt(
                "organizationDepartments",
                list(args.organization_departments),
            )
            .opt(
                "organizationFiscalReceiptEligibility",
                args.organization_fiscal_receipt_eligibility,
            )
            .opt(
                "organizationLinkedPartners",
                list(args.organization_linked_partners),
            )
            .build();
        let page: Value = self
            .client()
            .call(
                Request::post("/directory/forms")
                    .query(args.page.query())
                    .json(body),
            )
            .await?;
        ok(Listing::from_value(page))
    }

    /// Read one tag of HelloAsso's taxonomy, optionally with how many forms use
    /// it and what they collected. Needs the FormOpenDirectory privilege.
    #[rmcp::tool(annotations(title = "Get a tag", read_only_hint = true, open_world_hint = true))]
    pub async fn get_tag(
        &self,
        Parameters(args): Parameters<GetTagArgs>,
    ) -> std::result::Result<Json<ApiObject>, McpError> {
        let name = args.tag_name.trim();
        if name.is_empty() || name.chars().all(|c| c == '.') {
            return Err(Error::invalid("tag_name must be a tag's name").into());
        }
        // Tags hold spaces and accents, so they are percent-encoded rather
        // than checked like slugs.
        let encoded = encode_segment(name);
        let query = QueryBuilder::new()
            .opt("withCount", args.with_count)
            .opt("withAmount", args.with_amount)
            .build();
        let tag: Value = self
            .client()
            .call(Request::get(format!("/tags/{encoded}")).query(query))
            .await?;
        ok(ApiObject::from_value(tag))
    }

    /// List the public tags in use on HelloAsso, to filter the directory.
    #[rmcp::tool(annotations(
        title = "List public tags",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_public_tags(
        &self,
        Parameters(_): Parameters<NoArgs>,
    ) -> std::result::Result<Json<Listing>, McpError> {
        let list: Value = self.client().call(Request::get("/values/tags")).await?;
        ok(Listing::from_value(list))
    }
}

/// Percent-encode a path segment: everything but unreserved characters.
fn encode_segment(raw: &str) -> String {
    let mut out = String::with_capacity(raw.len());
    for byte in raw.bytes() {
        if byte.is_ascii_alphanumeric() || matches!(byte, b'-' | b'_' | b'.' | b'~') {
            out.push(char::from(byte));
        } else {
            let _ = write!(out, "%{byte:02X}");
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_tag_cannot_escape_its_segment() {
        assert_eq!(encode_segment("Été solidaire"), "%C3%89t%C3%A9%20solidaire");
        assert_eq!(encode_segment("../partners"), "..%2Fpartners");
    }
}
