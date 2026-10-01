//! The protocol, end to end.
//!
//! These tests spawn the real binary, speak JSON-RPC to its stdin and read its
//! stdout, with a fake HelloAsso behind it. If the handshake, the tool list, the
//! resources, the prompts or a tool call ever stop being what the MCP
//! specification asks for, this file fails.

#![allow(
    clippy::unwrap_used,
    clippy::expect_used,
    clippy::panic,
    reason = "a test that cannot fail loudly is not a test"
)]

mod common;

use std::process::Stdio;

use common::{FORM, Fake, ORG, PAYMENT_ID, env};
use serde_json::{Value, json};
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader, Lines},
    process::{Child, ChildStdin, ChildStdout, Command},
};

/// A connected MCP client, over the binary's stdio.
struct Mcp {
    child: Child,
    stdin: ChildStdin,
    lines: Lines<BufReader<ChildStdout>>,
    next_id: i64,
}

impl Mcp {
    /// Spawn the server and complete the handshake.
    async fn start(vars: &[(String, String)]) -> (Self, Value) {
        let mut command = Command::new(env!("CARGO_BIN_EXE_mcp-helloasso"));
        command
            .env_clear()
            .env("HELLOASSO_LOG", "warn")
            .env("PATH", std::env::var("PATH").unwrap_or_default())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::inherit());
        for (key, value) in vars {
            command.env(key, value);
        }
        let mut child = command.spawn().expect("the binary runs");
        let stdin = child.stdin.take().expect("stdin");
        let stdout = child.stdout.take().expect("stdout");
        let mut mcp = Self {
            child,
            stdin,
            lines: BufReader::new(stdout).lines(),
            next_id: 0,
        };
        let initialized = mcp
            .request(
                "initialize",
                json!({
                    "protocolVersion": "2025-11-25",
                    "capabilities": {},
                    "clientInfo": { "name": "mcp-helloasso-tests", "version": "0" }
                }),
            )
            .await;
        mcp.notify("notifications/initialized", json!({})).await;
        (mcp, initialized)
    }

    /// Send a request and wait for the answer carrying its id.
    async fn request(&mut self, method: &str, params: Value) -> Value {
        self.next_id += 1;
        let id = self.next_id;
        self.write(json!({
            "jsonrpc": "2.0",
            "id": id,
            "method": method,
            "params": params,
        }))
        .await;
        loop {
            let message = self.read().await;
            if message.get("id").and_then(Value::as_i64) == Some(id) {
                return message;
            }
        }
    }

    /// Send a notification, which expects no answer.
    async fn notify(&mut self, method: &str, params: Value) {
        self.write(json!({ "jsonrpc": "2.0", "method": method, "params": params }))
            .await;
    }

    /// Call a tool and return the whole JSON-RPC message, error or result.
    async fn call_tool(&mut self, name: &str, arguments: Value) -> Value {
        self.request(
            "tools/call",
            json!({ "name": name, "arguments": arguments }),
        )
        .await
    }

    /// Call a tool that is expected to succeed, and return its structured
    /// content.
    async fn structured(&mut self, name: &str, arguments: Value) -> Value {
        let message = self.call_tool(name, arguments).await;
        let result = message
            .get("result")
            .unwrap_or_else(|| panic!("{name} failed: {message}"));
        assert_ne!(
            result.get("isError").and_then(Value::as_bool),
            Some(true),
            "{name} answered an error: {result}"
        );
        result
            .get("structuredContent")
            .cloned()
            .unwrap_or_else(|| panic!("{name} answered without structured content: {result}"))
    }

    async fn write(&mut self, value: Value) {
        let mut line = serde_json::to_vec(&value).expect("serialize");
        line.push(b'\n');
        self.stdin.write_all(&line).await.expect("write");
        self.stdin.flush().await.expect("flush");
    }

    async fn read(&mut self) -> Value {
        let line = tokio::time::timeout(std::time::Duration::from_secs(20), self.lines.next_line())
            .await
            .expect("the server answers within twenty seconds")
            .expect("stdout is readable")
            .expect("the server did not close stdout");
        serde_json::from_str(&line).unwrap_or_else(|e| panic!("not JSON: {line:?} ({e})"))
    }

    /// Close the connection and reap the process.
    async fn shutdown(mut self) {
        drop(self.stdin);
        let _ = tokio::time::timeout(std::time::Duration::from_secs(5), self.child.wait()).await;
        let _ = self.child.kill().await;
    }
}

#[tokio::test]
async fn the_handshake_advertises_tools_resources_prompts_and_instructions() {
    let fake = Fake::start().await;
    let (mcp, initialized) = Mcp::start(&env(&fake, &[])).await;
    let result = &initialized["result"];

    assert_eq!(result["protocolVersion"], "2025-11-25");
    assert_eq!(result["serverInfo"]["name"], "helloasso");
    assert!(result["capabilities"]["tools"].is_object());
    assert!(result["capabilities"]["prompts"].is_object());
    assert!(result["capabilities"]["resources"].is_object());
    let instructions = result["instructions"].as_str().expect("instructions");
    assert!(
        instructions.contains("cents"),
        "the instructions must say amounts are cents: {instructions}"
    );
    mcp.shutdown().await;
}

#[tokio::test]
async fn every_tool_is_listed_with_a_schema_and_an_annotation() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let listed = mcp.request("tools/list", json!({})).await;
    let tools = listed["result"]["tools"]
        .as_array()
        .expect("a list of tools");
    assert_eq!(
        tools.len(),
        38,
        "36 routes plus whoami and summarize_payments"
    );
    for tool in tools {
        let name = tool["name"].as_str().expect("a name");
        assert!(
            tool["description"].as_str().is_some_and(|d| d.len() > 40),
            "{name} has no usable description"
        );
        assert_eq!(tool["inputSchema"]["type"], "object", "{name}");
        assert!(
            tool["outputSchema"].is_object(),
            "{name} has no output schema"
        );
        assert!(tool["annotations"]["title"].as_str().is_some(), "{name}");
    }
    mcp.shutdown().await;
}

#[tokio::test]
async fn whoami_finds_the_only_reachable_organization() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let me = mcp.structured("whoami", json!({})).await;
    assert_eq!(me["default_organization"], ORG);
    assert_eq!(me["organization"]["name"], "Les Amis du Vélo");
    assert_eq!(me["authentication"], "client_credentials");
    assert!(
        !me.to_string().contains("test-secret"),
        "the secret must never leave the process: {me}"
    );
    mcp.shutdown().await;
}

#[tokio::test]
async fn a_form_url_is_enough_to_name_a_form() {
    let fake = Fake::start().await;
    let route = format!("/v5/organizations/{ORG}/forms/Event/{FORM}/items");
    fake.mount(
        "GET",
        &route,
        common::page(vec![json!({"id": 1, "name": "Adult"})]),
    )
    .await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let items = mcp
        .structured(
            "list_form_items",
            json!({
                "form_url": format!("https://www.helloasso.com/associations/{ORG}/evenements/{FORM}"),
                "item_states": ["Processed"],
                "with_details": true
            }),
        )
        .await;
    assert_eq!(items["returned"], 1);
    assert_eq!(items["continuation_token"], "next-token");
    let query = fake.requests(&route).await[0]
        .url
        .query()
        .unwrap_or_default()
        .to_owned();
    assert!(query.contains("itemStates=Processed"), "{query}");
    assert!(query.contains("withDetails=true"), "{query}");
    mcp.shutdown().await;
}

#[tokio::test]
async fn a_period_becomes_a_half_open_date_range() {
    let fake = Fake::start().await;
    let route = format!("/v5/organizations/{ORG}/orders");
    fake.mount("GET", &route, common::page(vec![])).await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    mcp.structured(
        "list_orders",
        json!({ "period": "2026-09", "search": "camille" }),
    )
    .await;
    let query = fake.requests(&route).await[0]
        .url
        .query()
        .unwrap_or_default()
        .to_owned();
    assert!(query.contains("from=2026-09-01"), "{query}");
    assert!(query.contains("to=2026-10-01"), "{query}");
    assert!(query.contains("userSearchKey=camille"), "{query}");
    mcp.shutdown().await;
}

#[tokio::test]
async fn summarize_payments_counts_only_collected_money() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let summary = mcp
        .structured("summarize_payments", json!({ "period": "2026-09" }))
        .await;
    assert_eq!(summary["payments_read"], 3);
    assert_eq!(summary["collected"]["amount_cents"], 4500);
    assert_eq!(summary["collected"]["amount_euros"], 45.0);
    assert_eq!(summary["by_state"]["Refused"]["count"], 1);
    assert_eq!(
        summary["by_form"][format!("Event/{FORM}")]["amount_cents"],
        1500
    );
    mcp.shutdown().await;
}

#[tokio::test]
async fn a_refund_without_confirmation_says_what_would_happen_and_sends_nothing() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let message = mcp
        .call_tool("refund_payment", json!({ "payment_id": PAYMENT_ID }))
        .await;
    let error = message["error"]["message"]
        .as_str()
        .or_else(|| message["result"]["content"][0]["text"].as_str())
        .unwrap_or_else(|| panic!("expected a refusal: {message}"))
        .to_owned();
    assert!(error.contains("confirm: true"), "{error}");
    assert!(error.contains("25.00 €"), "the amount is shown: {error}");
    assert!(
        error.contains("Camille Martin"),
        "the payer is shown: {error}"
    );
    assert!(
        fake.requests(&format!("/v5/payments/{PAYMENT_ID}/refund"))
            .await
            .is_empty(),
        "nothing may reach HelloAsso without confirmation"
    );
    mcp.shutdown().await;
}

#[tokio::test]
async fn a_confirmed_partial_refund_sends_the_amount_and_the_mfa_header() {
    let fake = Fake::start().await;
    let route = format!("/v5/payments/{PAYMENT_ID}/refund");
    fake.mount(
        "POST",
        &route,
        json!({ "id": 1, "amount": 1000, "state": "Processing" }),
    )
    .await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let refund = mcp
        .structured(
            "refund_payment",
            json!({
                "payment_id": PAYMENT_ID,
                "amount_cents": 1000,
                "comment": "seat cancelled",
                "mfa_sms_code": "123456",
                "confirm": true
            }),
        )
        .await;
    assert_eq!(refund["amount"], 1000);
    let request = &fake.requests(&route).await[0];
    let query = request.url.query().unwrap_or_default();
    assert!(query.contains("amount=1000"), "{query}");
    assert!(query.contains("sendRefundMail=true"), "{query}");
    assert_eq!(
        request
            .headers
            .get("x-mfa-sms-access-authorization")
            .unwrap(),
        "123456"
    );
    mcp.shutdown().await;
}

#[tokio::test]
async fn read_only_mode_withholds_every_write_tool() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[("HELLOASSO_READ_ONLY", "1")])).await;
    let listed = mcp.request("tools/list", json!({})).await;
    let names: Vec<String> = listed["result"]["tools"]
        .as_array()
        .expect("tools")
        .iter()
        .filter_map(|t| t["name"].as_str().map(str::to_owned))
        .collect();
    assert_eq!(names.len(), 28, "{names:?}");
    for write in [
        "refund_payment",
        "cancel_order",
        "create_checkout_intent",
        "quick_create_event",
    ] {
        assert!(!names.contains(&write.to_owned()), "{write} survived");
    }
    mcp.shutdown().await;
}

#[tokio::test]
async fn a_checkout_whose_terms_do_not_add_up_is_refused_before_any_call() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let message = mcp
        .call_tool(
            "create_checkout_intent",
            json!({
                "item_name": "Adhésion 2026",
                "total_amount_cents": 6000,
                "initial_amount_cents": 2000,
                "terms": [{ "amount_cents": 2000, "date": "2026-11-01" }],
                "back_url": "https://example.org/back",
                "error_url": "https://example.org/error",
                "return_url": "https://example.org/ok",
                "contains_donation": false
            }),
        )
        .await;
    assert!(message.to_string().contains("must equal"), "{message}");
    assert!(
        fake.requests(&format!("/v5/organizations/{ORG}/checkout-intents"))
            .await
            .is_empty()
    );
    mcp.shutdown().await;
}

#[tokio::test]
async fn a_checkout_is_sent_in_helloasso_s_shape() {
    let fake = Fake::start().await;
    let route = format!("/v5/organizations/{ORG}/checkout-intents");
    fake.mount(
        "POST",
        &route,
        json!({ "id": 42, "redirectUrl": "https://checkout.example/42" }),
    )
    .await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let created = mcp
        .structured(
            "create_checkout_intent",
            json!({
                "item_name": "Adhésion 2026",
                "total_amount_cents": 6000,
                "terms": [{ "amount_cents": 2000, "date": "2026-11-01" }],
                "back_url": "https://example.org/back",
                "error_url": "https://example.org/error",
                "return_url": "https://example.org/ok",
                "contains_donation": false,
                "payer": { "first_name": "Camille", "email": "camille@example.test" },
                "metadata": { "member": 7 }
            }),
        )
        .await;
    assert_eq!(created["redirectUrl"], "https://checkout.example/42");
    let body: Value = serde_json::from_slice(&fake.requests(&route).await[0].body).unwrap();
    assert_eq!(body["totalAmount"], 6000);
    assert_eq!(body["initialAmount"], 4000);
    assert_eq!(body["terms"][0]["amount"], 2000);
    assert_eq!(body["payer"]["firstName"], "Camille");
    assert_eq!(body["metadata"]["member"], 7);
    mcp.shutdown().await;
}

#[tokio::test]
async fn resources_and_templates_are_listed_and_readable() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let listed = mcp.request("resources/list", json!({})).await;
    assert_eq!(
        listed["result"]["resources"].as_array().map(Vec::len),
        Some(3)
    );
    let templates = mcp.request("resources/templates/list", json!({})).await;
    assert_eq!(
        templates["result"]["resourceTemplates"]
            .as_array()
            .map(Vec::len),
        Some(2)
    );
    let read = mcp
        .request(
            "resources/read",
            json!({ "uri": "helloasso://organization" }),
        )
        .await;
    let text = read["result"]["contents"][0]["text"]
        .as_str()
        .expect("text");
    assert!(text.contains("Les Amis du Vélo"), "{text}");
    let order = mcp
        .request(
            "resources/read",
            json!({ "uri": "helloasso://orders/12345" }),
        )
        .await;
    assert!(
        order["result"]["contents"][0]["text"]
            .as_str()
            .is_some_and(|t| t.contains("12345")),
        "{order}"
    );
    mcp.shutdown().await;
}

#[tokio::test]
async fn prompts_are_listed_and_rendered() {
    let fake = Fake::start().await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let listed = mcp.request("prompts/list", json!({})).await;
    assert_eq!(
        listed["result"]["prompts"].as_array().map(Vec::len),
        Some(4)
    );
    let prompt = mcp
        .request(
            "prompts/get",
            json!({ "name": "refund_request", "arguments": { "who": "camille@example.test" } }),
        )
        .await;
    let text = prompt["result"]["messages"][0]["content"]["text"]
        .as_str()
        .expect("text");
    assert!(text.contains("Stop and wait"), "{text}");
    mcp.shutdown().await;
}

#[tokio::test]
async fn an_ambiguous_organization_comes_back_with_candidates() {
    let fake = Fake::bare().await;
    fake.mount(
        "GET",
        "/v5/users/me/organizations",
        json!([
            { "name": "Les Amis du Vélo", "organizationSlug": "les-amis-du-velo" },
            { "name": "Vélo Club", "organizationSlug": "velo-club" }
        ]),
    )
    .await;
    let (mut mcp, _) = Mcp::start(&env(&fake, &[])).await;
    let message = mcp.call_tool("list_forms", json!({})).await;
    let text = message.to_string();
    assert!(text.contains("velo-club"), "{text}");
    assert!(text.contains("HELLOASSO_ORGANIZATION"), "{text}");
    mcp.shutdown().await;
}
