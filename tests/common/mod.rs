//! A fake HelloAsso, so the tests exercise the real client and the real MCP
//! surface without touching anybody's money.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    clippy::needless_pass_by_value,
    dead_code,
    reason = "test support code"
)]

use serde_json::{Value, json};
use wiremock::{
    Mock, MockServer, ResponseTemplate,
    matchers::{method, path},
};

/// The organization every test works on.
pub const ORG: &str = "les-amis-du-velo";
/// A form of that organization.
pub const FORM: &str = "gala-2026";
/// The order the fake already holds.
pub const ORDER_ID: i64 = 12_345;
/// The payment the fake already holds.
pub const PAYMENT_ID: i64 = 67_890;

/// A running fake HelloAsso.
pub struct Fake {
    pub server: MockServer,
}

impl Fake {
    /// Start a fake with only the token endpoint mounted.
    pub async fn bare() -> Self {
        let fake = Self {
            server: MockServer::start().await,
        };
        fake.mount_token().await;
        fake
    }

    /// Start a fake with the read surface mounted.
    pub async fn start() -> Self {
        let fake = Self::bare().await;
        fake.mount(
            "GET",
            "/v5/users/me/organizations",
            json!([{ "name": "Les Amis du Vélo", "organizationSlug": ORG, "role": "OrganizationAdmin" }]),
        )
        .await;
        fake.mount(
            "GET",
            &format!("/v5/organizations/{ORG}"),
            json!({ "name": "Les Amis du Vélo", "organizationSlug": ORG, "type": "Association1901" }),
        )
        .await;
        fake.mount(
            "GET",
            &format!("/v5/organizations/{ORG}/forms"),
            page(vec![json!({ "title": "Gala 2026", "formSlug": FORM, "formType": "Event", "state": "Public" })]),
        )
        .await;
        fake.mount(
            "GET",
            &format!("/v5/organizations/{ORG}/payments"),
            page(vec![
                payment(
                    1,
                    1500,
                    "Authorized",
                    "Event",
                    FORM,
                    "2026-09-14T10:00:00+02:00",
                ),
                payment(
                    2,
                    3000,
                    "Authorized",
                    "Membership",
                    "adhesion-2026",
                    "2026-09-20T10:00:00+02:00",
                ),
                payment(
                    3,
                    900,
                    "Refused",
                    "Event",
                    FORM,
                    "2026-09-21T10:00:00+02:00",
                ),
            ]),
        )
        .await;
        fake.mount(
            "GET",
            &format!("/v5/payments/{PAYMENT_ID}"),
            payment(
                PAYMENT_ID,
                2500,
                "Authorized",
                "Event",
                FORM,
                "2026-09-14T10:00:00+02:00",
            ),
        )
        .await;
        fake.mount(
            "GET",
            &format!("/v5/orders/{ORDER_ID}"),
            json!({ "id": ORDER_ID, "amount": { "total": 2500 }, "formSlug": FORM, "formType": "Event" }),
        )
        .await;
        fake
    }

    /// The token endpoint: a 30-minute token, as HelloAsso hands out.
    pub async fn mount_token(&self) {
        Mock::given(method("POST"))
            .and(path("/oauth2/token"))
            .respond_with(ResponseTemplate::new(200).set_body_json(json!({
                "access_token": "test-access-token",
                "refresh_token": "test-refresh-token",
                "token_type": "bearer",
                "expires_in": 1800
            })))
            .mount(&self.server)
            .await;
    }

    /// Mount a 200 answer for one route.
    pub async fn mount(&self, verb: &str, route: &str, body: Value) {
        self.mount_status(verb, route, 200, body).await;
    }

    /// Mount an arbitrary status for one route.
    pub async fn mount_status(&self, verb: &str, route: &str, status: u16, body: Value) {
        let template = if body.is_null() {
            ResponseTemplate::new(status)
        } else {
            ResponseTemplate::new(status).set_body_json(body)
        };
        Mock::given(method(verb))
            .and(path(route.to_owned()))
            .respond_with(template)
            .mount(&self.server)
            .await;
    }

    /// The API root to hand to the server under test.
    pub fn base_url(&self) -> String {
        format!("{}/v5", self.server.uri())
    }

    /// The token endpoint to hand to the server under test.
    pub fn token_url(&self) -> String {
        format!("{}/oauth2/token", self.server.uri())
    }

    /// Every request the fake received on one path.
    pub async fn requests(&self, route: &str) -> Vec<wiremock::Request> {
        self.server
            .received_requests()
            .await
            .unwrap_or_default()
            .into_iter()
            .filter(|request| request.url.path() == route)
            .collect()
    }
}

/// Wrap objects in HelloAsso's pagination envelope.
pub fn page(items: Vec<Value>) -> Value {
    json!({
        "data": items,
        "pagination": {
            "pageSize": 20,
            "totalCount": items.len(),
            "pageIndex": 1,
            "totalPages": 1,
            "continuationToken": "next-token"
        }
    })
}

/// A payment, as the API returns one.
pub fn payment(
    id: i64,
    amount: i64,
    state: &str,
    form_type: &str,
    form_slug: &str,
    date: &str,
) -> Value {
    json!({
        "id": id,
        "amount": amount,
        "state": state,
        "paymentMeans": "Card",
        "date": date,
        "payer": { "firstName": "Camille", "lastName": "Martin", "email": "camille@example.test" },
        "order": { "id": ORDER_ID, "formType": form_type, "formSlug": form_slug }
    })
}

/// The environment a test server runs with.
pub fn env(fake: &Fake, extra: &[(&str, &str)]) -> Vec<(String, String)> {
    let mut vars = vec![
        ("HELLOASSO_CLIENT_ID".to_owned(), "test-client".to_owned()),
        (
            "HELLOASSO_CLIENT_SECRET".to_owned(),
            "test-secret".to_owned(),
        ),
        ("HELLOASSO_BASE_URL".to_owned(), fake.base_url()),
        ("HELLOASSO_TOKEN_URL".to_owned(), fake.token_url()),
        ("HELLOASSO_MAX_RETRIES".to_owned(), "1".to_owned()),
    ];
    for (key, value) in extra {
        vars.retain(|(existing, _)| existing != key);
        vars.push(((*key).to_owned(), (*value).to_owned()));
    }
    vars
}

/// Build a configuration from a list of pairs.
pub fn config(vars: &[(String, String)]) -> mcp_helloasso::Config {
    let owned = vars.to_vec();
    mcp_helloasso::Config::from_getter(move |key| {
        owned
            .iter()
            .find(|(existing, _)| existing == key)
            .map(|(_, value)| value.clone())
    })
    .expect("test configuration")
}
