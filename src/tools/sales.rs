//! Sales: orders, items, payments, refunds, cancellations and cash-outs — the
//! part of HelloAsso a treasurer lives in.

use std::collections::BTreeMap;

use rmcp::{ErrorData as McpError, Json, handler::server::wrapper::Parameters};
use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::{
    client::Request,
    dates,
    error::Error,
    resolve::FormType,
    server::HelloAssoServer,
    tools::{
        ApiObject, Done, Listing, PageArgs, QueryBuilder, SortField, SortOrder, api_name,
        forms::FormArgs, ok, positive, require_confirmation,
    },
};

/// The state of an item (a ticket, a membership, a donation line).
#[derive(Debug, Clone, Copy, Deserialize, Serialize, schemars::JsonSchema)]
pub enum ItemState {
    /// Paid and valid.
    Processed,
    /// Registered by hand by the organization, paid offline.
    Registered,
    /// Waiting for its payment.
    Waiting,
    /// Deleted.
    Deleted,
    /// Unknown.
    Unknown,
    /// Canceled.
    Canceled,
    /// Its payment was refused.
    Refused,
    /// The payer gave up before paying.
    Abandoned,
}

/// The state of a payment.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Deserialize, Serialize, schemars::JsonSchema)]
pub enum PaymentState {
    /// Scheduled for later, not processed yet (a future instalment).
    Pending,
    /// Authorized: the money is collected. This is what "paid" means.
    Authorized,
    /// Refused by the bank.
    Refused,
    /// Unknown.
    Unknown,
    /// Registered offline by the organization.
    Registered,
    /// Errored.
    Error,
    /// Refunded.
    Refunded,
    /// Being refunded.
    Refunding,
    /// Waiting.
    Waiting,
    /// Canceled.
    Canceled,
    /// Contested by the payer (chargeback).
    Contested,
    /// SEPA: waiting for the bank's validation.
    WaitingBankValidation,
    /// SEPA: waiting for the withdrawal.
    WaitingBankWithdraw,
    /// Abandoned before completion.
    Abandoned,
    /// Waiting for 3-D Secure authentication.
    WaitingAuthentication,
    /// Authorized in pre-production.
    AuthorizedPreprod,
    /// Corrected.
    Corrected,
    /// Deleted.
    Deleted,
    /// Inconsistent.
    Inconsistent,
    /// No donation.
    NoDonation,
    /// Initialized.
    Init,
}

/// The kind of tier an item was bought from.
#[derive(Debug, Clone, Copy, Deserialize, Serialize, schemars::JsonSchema)]
pub enum TierType {
    /// A one-off donation.
    Donation,
    /// A payment.
    Payment,
    /// An event registration (a ticket).
    Registration,
    /// A membership.
    Membership,
    /// A monthly donation.
    MonthlyDonation,
    /// A monthly payment.
    MonthlyPayment,
    /// A donation registered offline.
    OfflineDonation,
    /// A contribution.
    Contribution,
    /// A bonus.
    Bonus,
    /// A shop product.
    Product,
    /// An insurance option.
    Insurance,
}

/// The filters every sales listing shares.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct SalesFilter {
    /// A period: `this month`, `last month`, `2026-09`, `2026`, `last 30 days`,
    /// `today`… Overridden by `from` / `to` when they are given.
    pub period: Option<String>,
    /// First date, inclusive: `YYYY-MM-DD`, `today`, or an ISO datetime.
    pub from: Option<String>,
    /// Last date, **exclusive**.
    pub to: Option<String>,
    /// Find a person: first name, last name or email of the payer or the user.
    pub search: Option<String>,
    /// `Desc` (newest first, the default) or `Asc`.
    pub sort_order: Option<SortOrder>,
    /// Pagination.
    #[serde(flatten)]
    pub page: PageArgs,
}

impl SalesFilter {
    fn query(&self) -> Result<QueryBuilder, Error> {
        let (from, to) = dates::range(
            self.period.as_deref(),
            self.from.as_deref(),
            self.to.as_deref(),
        )?;
        Ok(QueryBuilder::new()
            .opt("from", from)
            .opt("to", to)
            .opt(
                "userSearchKey",
                self.search
                    .as_deref()
                    .map(str::trim)
                    .filter(|s| !s.is_empty()),
            )
            .opt_enum("sortOrder", self.sort_order)
            .paged(&self.page))
    }
}

/// Arguments of `list_orders`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListOrdersArgs {
    /// The organization slug. Defaults to the configured organization.
    pub organization: Option<String>,
    /// Keep only orders from these form types.
    pub form_types: Option<Vec<FormType>>,
    /// Include the custom fields answered by the payer.
    pub with_details: Option<bool>,
    /// Filters and pagination.
    #[serde(flatten)]
    pub filter: SalesFilter,
}

/// Arguments of `list_form_orders`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListFormOrdersArgs {
    /// The form.
    #[serde(flatten)]
    pub form: FormArgs,
    /// Include the custom fields answered by the payer.
    pub with_details: Option<bool>,
    /// Filters and pagination.
    #[serde(flatten)]
    pub filter: SalesFilter,
}

/// The filters specific to item listings.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ItemFilter {
    /// Keep only items of these tier types (Membership, Registration, Donation…).
    pub tier_types: Option<Vec<TierType>>,
    /// Keep only items in these states. `Processed` is "paid and valid".
    pub item_states: Option<Vec<ItemState>>,
    /// Keep only items of the tier with this exact name.
    pub tier_name: Option<String>,
    /// Include custom fields (the answers to the form's questions) and options.
    pub with_details: Option<bool>,
    /// Sort on `Date` (the default) or `UpdateDate`.
    pub sort_field: Option<SortField>,
}

impl ItemFilter {
    fn apply(&self, query: QueryBuilder) -> QueryBuilder {
        query
            .list("tierTypes", self.tier_types.as_deref())
            .list("itemStates", self.item_states.as_deref())
            .opt("tierName", self.tier_name.as_deref())
            .opt("withDetails", self.with_details)
            .opt_enum("sortField", self.sort_field)
    }
}

/// Arguments of `list_items`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListItemsArgs {
    /// The organization slug. Defaults to the configured organization.
    pub organization: Option<String>,
    /// Item filters.
    #[serde(flatten)]
    pub items: ItemFilter,
    /// Filters and pagination.
    #[serde(flatten)]
    pub filter: SalesFilter,
}

/// Arguments of `list_form_items`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListFormItemsArgs {
    /// The form.
    #[serde(flatten)]
    pub form: FormArgs,
    /// Item filters.
    #[serde(flatten)]
    pub items: ItemFilter,
    /// Filters and pagination.
    #[serde(flatten)]
    pub filter: SalesFilter,
}

/// Arguments of `list_payments`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListPaymentsArgs {
    /// The organization slug. Defaults to the configured organization.
    pub organization: Option<String>,
    /// Keep only payments in these states. `Authorized` means collected.
    pub states: Option<Vec<PaymentState>>,
    /// Sort on `Date` (the default) or `UpdateDate`.
    pub sort_field: Option<SortField>,
    /// Filters and pagination.
    #[serde(flatten)]
    pub filter: SalesFilter,
}

/// Arguments of `list_form_payments`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct ListFormPaymentsArgs {
    /// The form.
    #[serde(flatten)]
    pub form: FormArgs,
    /// Keep only payments in these states. `Authorized` means collected.
    pub states: Option<Vec<PaymentState>>,
    /// Sort on `Date` (the default) or `UpdateDate`.
    pub sort_field: Option<SortField>,
    /// Filters and pagination.
    #[serde(flatten)]
    pub filter: SalesFilter,
}

/// Arguments of `get_order`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct GetOrderArgs {
    /// The order id.
    pub order_id: i64,
    /// Include the form's data.
    pub with_form_data: Option<bool>,
    /// Say, for each payment, whether it can still be refunded.
    pub check_refund_eligibility: Option<bool>,
}

/// Arguments of `cancel_order`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct CancelOrderArgs {
    /// The order id.
    pub order_id: i64,
    /// Must be `true`: future instalments are canceled for good.
    pub confirm: Option<bool>,
}

/// Arguments of `get_item`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct GetItemArgs {
    /// The item id.
    pub item_id: i64,
    /// Include custom fields and options.
    pub with_details: Option<bool>,
}

/// Arguments of `get_payment`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct GetPaymentArgs {
    /// The payment id.
    pub payment_id: i64,
    /// Also list refund attempts that failed (aborted, canceled, error, refused).
    pub with_failed_refund_operation: Option<bool>,
}

/// Arguments of `refund_payment`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct RefundPaymentArgs {
    /// The payment id.
    pub payment_id: i64,
    /// For a partial refund, the amount in **cents**. The whole payment when
    /// absent.
    pub amount_cents: Option<i64>,
    /// A note about the refund, kept by HelloAsso.
    pub comment: Option<String>,
    /// Also cancel the order's future payments and items. Only possible when
    /// the payment is fully refunded.
    pub cancel_order: Option<bool>,
    /// Send the payer HelloAsso's refund email. Default true.
    pub send_refund_mail: Option<bool>,
    /// The MFA access token, only when a previous call answered
    /// `AuthorizationErrors.MFA.AccessTokenRequired`.
    pub mfa_access_token: Option<String>,
    /// The SMS one-time code, only when a previous call answered
    /// `AuthorizationErrors.MFA.AccessOtpSmsRequired`.
    pub mfa_sms_code: Option<String>,
    /// The password token, only when a previous call answered
    /// `AuthorizationErrors.MFA.AccessPasswordTokenRequired`.
    pub mfa_password_token: Option<String>,
    /// Must be `true`: a refund sends real money back and cannot be undone.
    pub confirm: Option<bool>,
}

/// The formats a cash-out export comes in.
#[derive(Debug, Clone, Copy, Default, Deserialize, Serialize, schemars::JsonSchema)]
#[serde(rename_all = "lowercase")]
pub enum ExportFormat {
    /// Rows as JSON, returned inline. The default.
    #[default]
    Json,
    /// Comma-separated values.
    Csv,
    /// An Excel workbook.
    Xlsx,
}

/// Arguments of `export_cash_out`.
#[derive(Debug, Clone, Deserialize, schemars::JsonSchema)]
pub struct ExportCashOutArgs {
    /// The organization slug. Defaults to the configured organization.
    pub organization: Option<String>,
    /// The cash-out id, as found in a payment's `idCashOut`.
    pub cash_out_id: i64,
    /// `json` (default, inline rows), `csv` or `xlsx`.
    pub format: Option<ExportFormat>,
    /// A path on the machine running this server to write the file to. Refused
    /// when the server is reached over HTTP.
    pub save_to: Option<String>,
}

/// What `export_cash_out` answers with.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct CashOutExport {
    /// The cash-out id.
    pub cash_out_id: i64,
    /// The media type HelloAsso labelled the file with.
    pub content_type: String,
    /// How many bytes came back.
    pub bytes: usize,
    /// The rows, for the JSON format. Amounts in cents.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub rows: Option<Vec<Value>>,
    /// The file itself, when it is text and small enough to inline.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub text: Option<String>,
    /// Where the file was written, when `save_to` was given.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub saved_to: Option<String>,
    /// Set when the file could not be inlined.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub note: Option<String>,
}

/// How much of a text export is returned inline.
const MAX_INLINE_BYTES: usize = 200_000;

/// Arguments of `summarize_payments`.
#[derive(Debug, Clone, Default, Deserialize, schemars::JsonSchema)]
pub struct SummarizeArgs {
    /// Restrict to one form; the whole organization when absent.
    #[serde(flatten)]
    pub form: FormArgs,
    /// A period: `this month`, `last month`, `2026-09`, `2026`, `last 30 days`.
    /// Defaults to this month.
    pub period: Option<String>,
    /// First date, inclusive. Overrides the period's start.
    pub from: Option<String>,
    /// Last date, exclusive. Overrides the period's end.
    pub to: Option<String>,
    /// Stop after reading this many payments. Default 5000.
    pub max_payments: Option<u32>,
}

/// One line of a breakdown.
#[derive(Debug, Clone, Default, Serialize, schemars::JsonSchema)]
pub struct Bucket {
    /// How many payments.
    pub count: u64,
    /// Their total, in cents.
    pub amount_cents: i64,
    /// Their total, in euros.
    pub amount_euros: f64,
}

impl Bucket {
    fn add(&mut self, cents: i64) {
        self.count += 1;
        self.amount_cents += cents;
        self.amount_euros = euros(self.amount_cents);
    }
}

/// What `summarize_payments` answers with.
#[derive(Debug, Clone, Serialize, schemars::JsonSchema)]
pub struct PaymentSummary {
    /// The organization.
    pub organization: String,
    /// The form, when the summary is restricted to one.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub form: Option<String>,
    /// First date, inclusive.
    pub from: Option<String>,
    /// Last date, exclusive.
    pub to: Option<String>,
    /// How many payments were read.
    pub payments_read: u64,
    /// True when `max_payments` stopped the walk before the end.
    pub truncated: bool,
    /// Authorized payments: the money actually collected.
    pub collected: Bucket,
    /// Every state, with its count and total.
    pub by_state: BTreeMap<String, Bucket>,
    /// Authorized payments, per form (`Type/slug`).
    pub by_form: BTreeMap<String, Bucket>,
    /// Authorized payments, per means (Card, Sepa, Check, Cash…).
    pub by_means: BTreeMap<String, Bucket>,
    /// Authorized payments, per month (`YYYY-MM`).
    pub by_month: BTreeMap<String, Bucket>,
    /// Refunded amount reported by refund operations, in cents.
    pub refunded_cents: i64,
}

#[allow(
    clippy::cast_precision_loss,
    reason = "amounts in cents stay far below 2^53"
)]
fn euros(cents: i64) -> f64 {
    cents as f64 / 100.0
}

fn order_query(with_details: Option<bool>, filter: &SalesFilter) -> Result<QueryBuilder, Error> {
    Ok(filter.query()?.opt("withDetails", with_details))
}

#[allow(missing_docs, reason = "the router function is generated by the macro")]
#[rmcp::tool_router(router = sales_router, vis = "pub")]
impl HelloAssoServer {
    /// List an organization's orders (one per checkout: payer, items,
    /// payments, amount in cents), newest first. Filter by period, by person
    /// with `search`, or by form type.
    #[rmcp::tool(annotations(
        title = "List orders",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_orders(
        &self,
        Parameters(args): Parameters<ListOrdersArgs>,
    ) -> std::result::Result<Json<Listing>, McpError> {
        let org = self
            .resolver()
            .organization(args.organization.as_deref())
            .await?;
        let query = order_query(args.with_details, &args.filter)?
            .list("formTypes", args.form_types.as_deref())
            .build();
        let page: Value = self
            .client()
            .call(Request::get(format!("/organizations/{org}/orders")).query(query))
            .await?;
        ok(Listing::from_value(page))
    }

    /// List the orders of one form (an event, a membership campaign…), newest
    /// first, with the same filters as `list_orders`.
    #[rmcp::tool(annotations(
        title = "List a form's orders",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_form_orders(
        &self,
        Parameters(args): Parameters<ListFormOrdersArgs>,
    ) -> std::result::Result<Json<Listing>, McpError> {
        let form = self.form_ref(&args.form).await?;
        let query = order_query(args.with_details, &args.filter)?.build();
        let page: Value = self
            .client()
            .call(Request::get(format!("{}/orders", form.path())).query(query))
            .await?;
        ok(Listing::from_value(page))
    }

    /// Read one order in full: payer, every item with its participant and
    /// custom fields, every payment and instalment, amounts in cents.
    #[rmcp::tool(annotations(
        title = "Get an order",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_order(
        &self,
        Parameters(args): Parameters<GetOrderArgs>,
    ) -> std::result::Result<Json<ApiObject>, McpError> {
        let id = positive("order_id", args.order_id)?;
        let query = QueryBuilder::new()
            .opt("withFormData", args.with_form_data)
            .opt(
                "checkPaymentsRefundEligibility",
                args.check_refund_eligibility,
            )
            .build();
        let order: Value = self
            .client()
            .call(Request::get(format!("/orders/{id}")).query(query))
            .await?;
        ok(ApiObject::from_value(order))
    }

    /// Cancel an order's future payments (remaining instalments or a monthly
    /// donation). Nothing already paid is refunded — use `refund_payment` for
    /// that. Requires `confirm: true`.
    #[rmcp::tool(annotations(
        title = "Cancel an order",
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = true,
        open_world_hint = true
    ))]
    pub async fn cancel_order(
        &self,
        Parameters(args): Parameters<CancelOrderArgs>,
    ) -> std::result::Result<Json<Done>, McpError> {
        let id = positive("order_id", args.order_id)?;
        require_confirmation(
            args.confirm,
            &format!("every future payment of order {id} would be canceled"),
        )?;
        let answer: Value = self
            .client()
            .call(Request::post(format!("/orders/{id}/cancel")))
            .await?;
        ok(Done::new(
            format!("future payments of order {id} canceled"),
            answer,
        ))
    }

    /// List an organization's items — one per ticket, membership, donation or
    /// product sold, with the participant's name and, with `with_details`, the
    /// answers to the form's questions. This is the attendee or member list.
    #[rmcp::tool(annotations(title = "List items", read_only_hint = true, open_world_hint = true))]
    pub async fn list_items(
        &self,
        Parameters(args): Parameters<ListItemsArgs>,
    ) -> std::result::Result<Json<Listing>, McpError> {
        let org = self
            .resolver()
            .organization(args.organization.as_deref())
            .await?;
        let query = args.items.apply(args.filter.query()?).build();
        let page: Value = self
            .client()
            .call(Request::get(format!("/organizations/{org}/items")).query(query))
            .await?;
        ok(Listing::from_value(page))
    }

    /// List the items of one form: the attendees of an event, the members of a
    /// membership campaign, the donors of a donation form.
    #[rmcp::tool(annotations(
        title = "List a form's items",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_form_items(
        &self,
        Parameters(args): Parameters<ListFormItemsArgs>,
    ) -> std::result::Result<Json<Listing>, McpError> {
        let form = self.form_ref(&args.form).await?;
        let query = args.items.apply(args.filter.query()?).build();
        let page: Value = self
            .client()
            .call(Request::get(format!("{}/items", form.path())).query(query))
            .await?;
        ok(Listing::from_value(page))
    }

    /// Read one item: tier, amount in cents, state, participant, custom fields,
    /// options, and the ticket or membership card URL.
    #[rmcp::tool(annotations(
        title = "Get an item",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_item(
        &self,
        Parameters(args): Parameters<GetItemArgs>,
    ) -> std::result::Result<Json<ApiObject>, McpError> {
        let id = positive("item_id", args.item_id)?;
        let query = QueryBuilder::new()
            .opt("withDetails", args.with_details)
            .build();
        let item: Value = self
            .client()
            .call(Request::get(format!("/items/{id}")).query(query))
            .await?;
        ok(ApiObject::from_value(item))
    }

    /// List an organization's payments, newest first: amount in cents, state,
    /// means, payer, order, cash-out, receipt URLs. `Authorized` is collected
    /// money; `Pending` is a future instalment.
    #[rmcp::tool(annotations(
        title = "List payments",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_payments(
        &self,
        Parameters(args): Parameters<ListPaymentsArgs>,
    ) -> std::result::Result<Json<Listing>, McpError> {
        let org = self
            .resolver()
            .organization(args.organization.as_deref())
            .await?;
        let query = args
            .filter
            .query()?
            .list("states", args.states.as_deref())
            .opt_enum("sortField", args.sort_field)
            .build();
        let page: Value = self
            .client()
            .call(Request::get(format!("/organizations/{org}/payments")).query(query))
            .await?;
        ok(Listing::from_value(page))
    }

    /// List the payments of one form, with the same filters as `list_payments`.
    #[rmcp::tool(annotations(
        title = "List a form's payments",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn list_form_payments(
        &self,
        Parameters(args): Parameters<ListFormPaymentsArgs>,
    ) -> std::result::Result<Json<Listing>, McpError> {
        let form = self.form_ref(&args.form).await?;
        let query = args
            .filter
            .query()?
            .list("states", args.states.as_deref())
            .opt_enum("sortField", args.sort_field)
            .build();
        let page: Value = self
            .client()
            .call(Request::get(format!("{}/payments", form.path())).query(query))
            .await?;
        ok(Listing::from_value(page))
    }

    /// Read one payment: amount in cents, state, means, payer, items, order,
    /// cash-out, refund operations, receipt and fiscal receipt URLs.
    #[rmcp::tool(annotations(
        title = "Get a payment",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn get_payment(
        &self,
        Parameters(args): Parameters<GetPaymentArgs>,
    ) -> std::result::Result<Json<ApiObject>, McpError> {
        let id = positive("payment_id", args.payment_id)?;
        let query = QueryBuilder::new()
            .opt(
                "withFailedRefundOperation",
                args.with_failed_refund_operation,
            )
            .build();
        let payment: Value = self
            .client()
            .call(Request::get(format!("/payments/{id}")).query(query))
            .await?;
        ok(ApiObject::from_value(payment))
    }

    /// Refund a payment, fully or partially (amount in cents), optionally
    /// canceling the rest of the order. Real money goes back to the payer:
    /// requires `confirm: true`, and HelloAsso may demand strong
    /// authentication, in which case the error names the MFA field to fill.
    #[rmcp::tool(annotations(
        title = "Refund a payment",
        read_only_hint = false,
        destructive_hint = true,
        idempotent_hint = false,
        open_world_hint = true
    ))]
    pub async fn refund_payment(
        &self,
        Parameters(args): Parameters<RefundPaymentArgs>,
    ) -> std::result::Result<Json<ApiObject>, McpError> {
        let id = positive("payment_id", args.payment_id)?;
        if let Some(amount) = args.amount_cents {
            positive("amount_cents", amount)?;
        }
        if args.confirm != Some(true) {
            // Say what would go, so the confirmation is an informed one.
            let what = match self
                .client()
                .call::<Value>(Request::get(format!("/payments/{id}")))
                .await
            {
                Ok(payment) => {
                    let total = payment["amount"].as_i64().unwrap_or_default();
                    let payer = [
                        payment["payer"]["firstName"].as_str(),
                        payment["payer"]["lastName"].as_str(),
                    ]
                    .into_iter()
                    .flatten()
                    .collect::<Vec<_>>()
                    .join(" ");
                    format!(
                        "payment {id} ({:.2} € by {}, {}) would be refunded {}",
                        euros(total),
                        if payer.is_empty() {
                            "an unnamed payer"
                        } else {
                            &payer
                        },
                        payment["state"].as_str().unwrap_or("unknown state"),
                        match args.amount_cents {
                            Some(cents) => format!("for {:.2} €", euros(cents)),
                            None => "in full".to_owned(),
                        }
                    )
                }
                Err(_) => format!("payment {id} would be refunded"),
            };
            require_confirmation(None, &what)?;
        }
        let query = QueryBuilder::new()
            .opt("amount", args.amount_cents)
            .opt("comment", args.comment.as_deref())
            .opt("cancelOrder", args.cancel_order)
            .opt(
                "sendRefundMail",
                Some(args.send_refund_mail.unwrap_or(true)),
            )
            .build();
        let refund: Value = self
            .client()
            .call(
                Request::post(format!("/payments/{id}/refund"))
                    .query(query)
                    .header_opt("x-mfa-access-authorization", args.mfa_access_token)
                    .header_opt("x-mfa-sms-access-authorization", args.mfa_sms_code)
                    .header_opt("x-mfa-password-authorization", args.mfa_password_token),
            )
            .await?;
        ok(ApiObject::from_value(refund))
    }

    /// Export the detail of a cash-out (versement): every payment HelloAsso
    /// paid out in it, with fees and amounts in cents. JSON rows inline, or a
    /// CSV or Excel file.
    #[rmcp::tool(annotations(
        title = "Export a cash-out",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn export_cash_out(
        &self,
        Parameters(args): Parameters<ExportCashOutArgs>,
    ) -> std::result::Result<Json<CashOutExport>, McpError> {
        let id = positive("cash_out_id", args.cash_out_id)?;
        if args.save_to.is_some() && self.config().http_bind.is_some() {
            return Err(Error::invalid(
                "save_to writes on the server's disk, which is refused when it is reached over HTTP. Ask for the json format instead.",
            )
            .into());
        }
        let org = self
            .resolver()
            .organization(args.organization.as_deref())
            .await?;
        let format = args.format.unwrap_or_default();
        let accept = match format {
            ExportFormat::Json => "application/json",
            ExportFormat::Csv => "text/csv",
            ExportFormat::Xlsx => {
                "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            }
        };
        let response = self
            .client()
            .send(Request::get(format!("/organizations/{org}/cash-out/{id}/export")).accept(accept))
            .await?;
        let size = response.bytes.len();
        let saved_to = match args.save_to {
            Some(path) => {
                tokio::fs::write(&path, &response.bytes)
                    .await
                    .map_err(|source| Error::Io {
                        path: path.clone(),
                        source,
                    })?;
                Some(path)
            }
            None => None,
        };
        let is_json = response.content_type.contains("json");
        let textual = is_json
            || response.content_type.starts_with("text/")
            || response.content_type.contains("csv");
        let rows = if is_json {
            serde_json::from_slice::<Value>(&response.bytes)
                .ok()
                .map(|v| match v {
                    Value::Array(rows) => rows,
                    other => vec![other],
                })
        } else {
            None
        };
        let (text, note) = if rows.is_some() {
            (None, None)
        } else if textual && size <= MAX_INLINE_BYTES {
            (
                // HelloAsso's CSV starts with a byte-order mark for Excel.
                Some(
                    String::from_utf8_lossy(&response.bytes)
                        .trim_start_matches('\u{feff}')
                        .to_owned(),
                ),
                None,
            )
        } else if saved_to.is_some() {
            (None, None)
        } else {
            (
                None,
                Some(format!(
                    "{size} bytes of {}: pass save_to to write it to a file",
                    response.content_type
                )),
            )
        };
        ok(CashOutExport {
            cash_out_id: id,
            content_type: response.content_type,
            bytes: size,
            rows,
            text,
            saved_to,
            note,
        })
    }

    /// Total the payments of an organization or a form over a period: what was
    /// collected, and the split per state, form, payment means and month,
    /// in cents and euros. Walks every page so the numbers are not a sample.
    #[rmcp::tool(annotations(
        title = "Summarize payments",
        read_only_hint = true,
        open_world_hint = true
    ))]
    pub async fn summarize_payments(
        &self,
        Parameters(args): Parameters<SummarizeArgs>,
    ) -> std::result::Result<Json<PaymentSummary>, McpError> {
        let scoped = args.form.form_url.is_some() || args.form.form_slug.is_some();
        let (organization, form, path) = if scoped {
            let form = self.form_ref(&args.form).await?;
            let label = format!("{}/{}", form.form_type.as_str(), form.form_slug);
            (
                form.organization.clone(),
                Some(label),
                format!("{}/payments", form.path()),
            )
        } else {
            let org = self
                .resolver()
                .organization(args.form.organization.as_deref())
                .await?;
            let path = format!("/organizations/{org}/payments");
            (org, None, path)
        };
        let period = match (&args.period, &args.from, &args.to) {
            (None, None, None) => Some("this month"),
            (period, _, _) => period.as_deref(),
        };
        let (from, to) = dates::range(period, args.from.as_deref(), args.to.as_deref())?;
        let max = u64::from(args.max_payments.unwrap_or(5000).max(1));

        let mut summary = PaymentSummary {
            organization,
            form,
            from: from.clone(),
            to: to.clone(),
            payments_read: 0,
            truncated: false,
            collected: Bucket::default(),
            by_state: BTreeMap::new(),
            by_form: BTreeMap::new(),
            by_means: BTreeMap::new(),
            by_month: BTreeMap::new(),
            refunded_cents: 0,
        };
        let mut token: Option<String> = None;
        let mut page_index = 1_u32;
        loop {
            let mut query = QueryBuilder::new()
                .opt("from", from.as_deref())
                .opt("to", to.as_deref())
                .opt_enum("sortOrder", Some(SortOrder::Asc))
                .paged(&PageArgs {
                    page_size: Some(100),
                    page_index: Some(page_index),
                    continuation_token: token.clone(),
                });
            query.push("withDetails", false);
            let page = Listing::from_value(
                self.client()
                    .call(Request::get(path.clone()).query(query.build()))
                    .await?,
            );
            let received = page.items.len();
            for payment in &page.items {
                if summary.payments_read >= max {
                    summary.truncated = true;
                    break;
                }
                summary.payments_read += 1;
                tally(&mut summary, payment);
            }
            if summary.truncated || received < 100 {
                break;
            }
            if let Some(total) = page.total_pages
                && i64::from(page_index) >= total
            {
                break;
            }
            match page.continuation_token {
                Some(next) if Some(&next) != token.as_ref() => token = Some(next),
                _ => {
                    token = None;
                    page_index += 1;
                }
            }
        }
        ok(summary)
    }
}

/// Add one payment to the summary.
fn tally(summary: &mut PaymentSummary, payment: &Value) {
    let cents = payment["amount"].as_i64().unwrap_or_default();
    let state = payment["state"].as_str().unwrap_or("Unknown").to_owned();
    summary
        .by_state
        .entry(state.clone())
        .or_default()
        .add(cents);
    for refund in payment["refundOperations"].as_array().into_iter().flatten() {
        let processed = refund["state"].as_str().is_none_or(|s| s == "Processed");
        if processed {
            summary.refunded_cents += refund["amount"].as_i64().unwrap_or_default();
        }
    }
    if state != api_name(&PaymentState::Authorized) {
        return;
    }
    summary.collected.add(cents);
    let order = &payment["order"];
    let form = match (order["formType"].as_str(), order["formSlug"].as_str()) {
        (Some(kind), Some(slug)) => format!("{kind}/{slug}"),
        (None, Some(slug)) => slug.to_owned(),
        _ => "unknown".to_owned(),
    };
    summary.by_form.entry(form).or_default().add(cents);
    let means = payment["paymentMeans"].as_str().unwrap_or("Unknown");
    summary
        .by_means
        .entry(means.to_owned())
        .or_default()
        .add(cents);
    let month = payment["date"]
        .as_str()
        .and_then(|d| d.get(..7))
        .unwrap_or("unknown");
    summary
        .by_month
        .entry(month.to_owned())
        .or_default()
        .add(cents);
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
    use serde_json::json;

    fn empty() -> PaymentSummary {
        PaymentSummary {
            organization: "x".into(),
            form: None,
            from: None,
            to: None,
            payments_read: 0,
            truncated: false,
            collected: Bucket::default(),
            by_state: BTreeMap::new(),
            by_form: BTreeMap::new(),
            by_means: BTreeMap::new(),
            by_month: BTreeMap::new(),
            refunded_cents: 0,
        }
    }

    #[test]
    fn only_authorized_payments_count_as_collected() {
        let mut summary = empty();
        tally(
            &mut summary,
            &json!({"amount": 1500, "state": "Authorized", "paymentMeans": "Card",
                    "date": "2026-09-14T10:00:00+02:00",
                    "order": {"formType": "Event", "formSlug": "gala"}}),
        );
        tally(
            &mut summary,
            &json!({"amount": 3000, "state": "Refused", "paymentMeans": "Card"}),
        );
        tally(
            &mut summary,
            &json!({"amount": 2000, "state": "Authorized", "paymentMeans": "Sepa",
                    "date": "2026-10-01T10:00:00+02:00",
                    "order": {"formType": "Membership", "formSlug": "adhesion-2026"},
                    "refundOperations": [{"amount": 500, "state": "Processed"}]}),
        );
        assert_eq!(summary.collected.amount_cents, 3500);
        assert!((summary.collected.amount_euros - 35.0).abs() < f64::EPSILON);
        assert_eq!(summary.by_state["Refused"].count, 1);
        assert_eq!(summary.by_form["Event/gala"].amount_cents, 1500);
        assert_eq!(summary.by_means["Sepa"].amount_cents, 2000);
        assert_eq!(summary.by_month["2026-09"].count, 1);
        assert_eq!(summary.refunded_cents, 500);
    }
}
