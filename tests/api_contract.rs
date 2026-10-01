//! What this client promises about HelloAsso's manners.
//!
//! Each test here stands for a line of the README: one token reused, a revoked
//! token renewed, a 429 retried, the three error envelopes read, arrays sent as
//! repeated keys.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test that cannot fail loudly is not a test"
)]

mod common;

use common::{Fake, ORG, config, env, page};
use mcp_helloasso::{HelloAsso, client::Request, error::Error};
use serde_json::{Value, json};
use wiremock::{
    Mock, ResponseTemplate,
    matchers::{header, method, path},
};

fn client(fake: &Fake) -> HelloAsso {
    HelloAsso::new(config(&env(fake, &[]))).expect("client")
}

#[tokio::test]
async fn one_token_serves_many_requests() {
    let fake = Fake::start().await;
    let client = client(&fake);
    for _ in 0..3 {
        let _: Value = client
            .call(Request::get(format!("/organizations/{ORG}")))
            .await
            .expect("the call succeeds");
    }
    assert_eq!(
        fake.requests("/oauth2/token").await.len(),
        1,
        "the token is reused"
    );
    let request = &fake.requests(&format!("/v5/organizations/{ORG}")).await[0];
    assert_eq!(
        request.headers.get("authorization").unwrap(),
        "Bearer test-access-token"
    );
}

#[tokio::test]
async fn the_grant_is_client_credentials_in_a_form() {
    let fake = Fake::start().await;
    client(&fake).access_token().await.expect("a token");
    let body = String::from_utf8(fake.requests("/oauth2/token").await[0].body.clone()).unwrap();
    assert!(body.contains("grant_type=client_credentials"), "{body}");
    assert!(body.contains("client_id=test-client"), "{body}");
    assert!(body.contains("client_secret=test-secret"), "{body}");
}

#[tokio::test]
async fn a_revoked_token_is_renewed_once_and_the_call_succeeds() {
    let fake = Fake::bare().await;
    Mock::given(method("GET"))
        .and(path("/v5/values/tags"))
        .respond_with(ResponseTemplate::new(401))
        .up_to_n_times(1)
        .mount(&fake.server)
        .await;
    fake.mount("GET", "/v5/values/tags", json!([{ "name": "solidarité" }]))
        .await;
    let tags: Value = client(&fake)
        .call(Request::get("/values/tags"))
        .await
        .expect("the second attempt succeeds");
    assert_eq!(tags[0]["name"], "solidarité");
    assert_eq!(fake.requests("/oauth2/token").await.len(), 2);
}

#[tokio::test]
async fn refused_credentials_name_the_variables() {
    let fake = Fake {
        server: wiremock::MockServer::start().await,
    };
    Mock::given(method("POST"))
        .and(path("/oauth2/token"))
        .respond_with(ResponseTemplate::new(400).set_body_json(json!({
            "error": "unauthorized_client",
            "error_description": "Invalid client"
        })))
        .mount(&fake.server)
        .await;
    let err = client(&fake).access_token().await.unwrap_err();
    let message = err.to_string();
    assert!(matches!(err, Error::Auth(_)), "{err:?}");
    assert!(message.contains("HELLOASSO_CLIENT_SECRET"), "{message}");
    assert!(message.contains("Invalid client"), "{message}");
}

#[tokio::test]
async fn a_rate_limited_request_is_retried_and_then_succeeds() {
    let fake = Fake::bare().await;
    Mock::given(method("GET"))
        .and(path("/v5/values/organization/categories"))
        .respond_with(ResponseTemplate::new(429).insert_header("retry-after", "0"))
        .up_to_n_times(1)
        .mount(&fake.server)
        .await;
    fake.mount("GET", "/v5/values/organization/categories", json!([]))
        .await;
    let _: Value = client(&fake)
        .call(Request::get("/values/organization/categories"))
        .await
        .expect("the retry succeeds");
    assert_eq!(
        fake.requests("/v5/values/organization/categories")
            .await
            .len(),
        2
    );
}

#[tokio::test]
async fn a_403_says_which_privilege_is_missing() {
    let fake = Fake::bare().await;
    fake.mount_status("GET", "/v5/partners/me", 403, Value::Null)
        .await;
    let err = client(&fake)
        .call::<Value>(Request::get("/partners/me"))
        .await
        .unwrap_err();
    assert_eq!(err.status(), Some(403));
    assert!(err.to_string().contains("privilege"), "{err}");
}

#[tokio::test]
async fn an_empty_write_answer_reads_as_null() {
    let fake = Fake::bare().await;
    fake.mount_status("POST", "/v5/orders/1/cancel", 200, Value::Null)
        .await;
    let answer: Value = client(&fake)
        .call(Request::post("/orders/1/cancel"))
        .await
        .expect("an empty 200 is a success");
    assert!(answer.is_null());
}

#[tokio::test]
async fn arrays_travel_as_repeated_keys() {
    let fake = Fake::bare().await;
    fake.mount(
        "GET",
        &format!("/v5/organizations/{ORG}/forms"),
        page(vec![]),
    )
    .await;
    let _: Value = client(&fake)
        .call(
            Request::get(format!("/organizations/{ORG}/forms")).query(vec![
                ("states".into(), "Public".into()),
                ("states".into(), "Private".into()),
            ]),
        )
        .await
        .unwrap();
    let request = &fake
        .requests(&format!("/v5/organizations/{ORG}/forms"))
        .await[0];
    assert_eq!(request.url.query(), Some("states=Public&states=Private"));
}

#[tokio::test]
async fn a_static_token_is_used_as_is() {
    let fake = Fake::bare().await;
    Mock::given(method("GET"))
        .and(path("/v5/values/tags"))
        .and(header("authorization", "Bearer my-jwt"))
        .respond_with(ResponseTemplate::new(200).set_body_json(json!([])))
        .mount(&fake.server)
        .await;
    let client = HelloAsso::new(config(&[
        ("HELLOASSO_ACCESS_TOKEN".into(), "my-jwt".into()),
        ("HELLOASSO_BASE_URL".into(), fake.base_url()),
    ]))
    .unwrap();
    let _: Value = client.call(Request::get("/values/tags")).await.unwrap();
    assert!(fake.requests("/oauth2/token").await.is_empty());
}
