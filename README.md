<div align="center">

# mcp-helloasso

**Your association's HelloAsso, answered by asking.**

A single Rust binary that puts the whole [HelloAsso](https://www.helloasso.com) v5 API —
memberships, tickets, donations, payments, refunds, checkouts — in front of any MCP client.

[![Rust](https://img.shields.io/badge/Rust-1.88%2B-CE422B?logo=rust&logoColor=white)](https://www.rust-lang.org)
[![MCP](https://img.shields.io/badge/MCP-2025--11--25-8A63D2)](https://modelcontextprotocol.io)
[![HelloAsso API](https://img.shields.io/badge/HelloAsso%20API%20v5-36%2F36%20routes-49D38A)](https://api.helloasso.com/v5/swagger/ui/index)
[![Transports](https://img.shields.io/badge/transports-stdio%20%2B%20Streamable%20HTTP-0b7285)](#run-it-over-http-instead)
[![CI](https://github.com/edouard-claude/helloasso-mcp/actions/workflows/ci.yml/badge.svg)](https://github.com/edouard-claude/helloasso-mcp/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/edouard-claude/helloasso-mcp?color=49D38A)](https://github.com/edouard-claude/helloasso-mcp/releases/latest)
[![License](https://img.shields.io/badge/License-MIT-green.svg)](LICENSE)
[![Stars](https://img.shields.io/github/stars/edouard-claude/helloasso-mcp?style=flat&logo=github&color=f5c518)](https://github.com/edouard-claude/helloasso-mcp/stargazers)

🇫🇷 *Serveur MCP pour HelloAsso : adhérents, billetterie, dons, paiements et remboursements,
interrogés en langage naturel depuis Claude, Cursor ou tout client MCP.*

</div>

---

Ask your assistant *"how much did we collect last month?"*, *"is Camille Martin's membership
paid?"*, *"give me the attendee list of the gala"* — and get the answer from the same
HelloAsso your treasurer reconciles against.

```console
> how much did we collect in September, and on what?

  summarize_payments   2026-09 · 214 payments read
                       collected   8 342,00 €   (196 authorized)
                       Membership/adhesion-2026     5 160,00 €   61.9%
                       Event/gala-2026              2 730,00 €   32.7%
                       Donation/dons                  452,00 €    5.4%
                       by means: Card 7 912,00 € · Sepa 430,00 €
                       refunded 60,00 € · 3 refused · 1 contested

> who registered for the gala, with their dietary answers?

  list_form_items      form_url = helloasso.com/associations/les-amis-du-velo/evenements/gala-2026
                       item_states = Processed, Registered · with_details = true
                       87 participants, 3 tiers, custom field "Régime alimentaire"
```

Paste a HelloAsso URL, say *last month*, name a person. The server resolves the
organization, the form, the period and the pagination.

## Why

HelloAsso's API is a clean REST surface, but handing it raw to a language model goes
wrong in predictable ways: amounts are **cents**, a form is a *(type, slug)* pair nobody
remembers, date ranges are half-open, tokens expire every 30 minutes, and the one call
that matters most — a refund — sends real money back with no undo.

- **The whole API, nothing hidden.** All 36 routes of the public v5 spec, one tool each,
  plus `whoami` and `summarize_payments`, which exist because the API has no "who am I"
  for a client and no aggregate. A test fails if that count ever drops.
- **URLs, words and periods, not identifiers.** Every form tool takes the form's public
  URL. Form types read in English, French or URL spelling (`evenements`, `adhésions`).
  Periods read `last month`, `2026-09`, `last 30 days`. Persons are found with `search`.
- **Money that adds up.** `summarize_payments` walks every page and splits what was
  *actually* collected (`Authorized`) per form, payment means and month, in cents and
  euros — no model adding up page one and calling it a total.
- **Fail-closed where money moves.** `refund_payment` and `cancel_order` refuse to run
  without `confirm: true`, and the refusal states the amount and the payer so the
  confirmation is an informed one. `HELLOASSO_READ_ONLY=1` withholds every write tool.
- **OAuth handled the way HelloAsso asks.** `client_credentials` once, then
  `refresh_token` — never a new grant while a refresh works, as the API requires. A token
  revoked server-side is renewed transparently.
- **Written for a model to read.** Structured output with schemas on all 38 tools,
  annotations on every one, four prompts that stop and ask before touching money, and
  server instructions that say up front: amounts are cents.

## Install

<details open>
<summary><b>Prebuilt binary (macOS, Linux, Windows)</b></summary>

Download the archive for your platform from the
[latest release](https://github.com/edouard-claude/helloasso-mcp/releases/latest),
check it against its `.sha256`, and put `mcp-helloasso` on your `PATH`:

```bash
# macOS on Apple silicon; swap the target for x86_64-apple-darwin,
# x86_64-unknown-linux-gnu or aarch64-unknown-linux-gnu
V=v0.1.0 T=aarch64-apple-darwin
curl -LO https://github.com/edouard-claude/helloasso-mcp/releases/download/$V/mcp-helloasso-$V-$T.tar.gz
tar xzf mcp-helloasso-$V-$T.tar.gz
sudo mv mcp-helloasso-$V-$T/mcp-helloasso /usr/local/bin/
```

Windows: take the `x86_64-pc-windows-msvc.zip` archive.
</details>

<details>
<summary><b>From source (any platform with Rust 1.88+)</b></summary>

```bash
git clone https://github.com/edouard-claude/helloasso-mcp
cd helloasso-mcp
cargo build --release
# the binary is at target/release/mcp-helloasso
```
</details>

<details>
<summary><b>With cargo install</b></summary>

```bash
cargo install --git https://github.com/edouard-claude/helloasso-mcp
```
</details>

<details>
<summary><b>With Docker</b></summary>

```bash
docker run --rm -i \
  -e HELLOASSO_CLIENT_ID=… -e HELLOASSO_CLIENT_SECRET=… \
  ghcr.io/edouard-claude/helloasso-mcp:latest
```

Or build it yourself with `docker build -t mcp-helloasso .`.

The image runs unprivileged and speaks stdio by default; add `--http` for the HTTP
transport. See [Run it over HTTP instead](#run-it-over-http-instead).
</details>

### Get API credentials

In the HelloAsso back office of your association: **My account → Integrations and API**
(*Mon compte → Intégrations et API*). You get a **client id** and a **client secret**,
with the `AccessPublicData`, `AccessTransactions` and `Checkout` privileges, scoped to
your organization. Partners get theirs from HelloAsso's partnership team.

Then check the wiring before pointing a client at it:

```bash
HELLOASSO_CLIENT_ID=… HELLOASSO_CLIENT_SECRET=… mcp-helloasso --check
```

```console
environment  production
base url     https://api.helloasso.com/v5
toolsets     organization, forms, sales, checkout, directory, partner
read only    false
tools        38
token        ok
reachable    1 organization(s)
             Les Amis du Vélo (les-amis-du-velo) as OrganizationAdmin
organization Les Amis du Vélo (les-amis-du-velo)

ready.
```

## Configure

| Variable | Required | Default | What it is |
|---|:-:|---|---|
| `HELLOASSO_CLIENT_ID` | yes | | The API client id |
| `HELLOASSO_CLIENT_SECRET` | yes | | Its secret |
| `HELLOASSO_CLIENT_SECRET_FILE` | | | A file holding the secret instead, for Docker secrets |
| `HELLOASSO_ACCESS_TOKEN` | | | Instead of a client: a ready-made bearer token, used as is |
| `HELLOASSO_ENVIRONMENT` | | `production` | `sandbox` targets `api.helloasso-sandbox.com` |
| `HELLOASSO_ORGANIZATION` | | | The organization slug tools default to |
| `HELLOASSO_TOOLSETS` | | `all` | `organization,forms,sales,checkout,directory,partner` |
| `HELLOASSO_READ_ONLY` | | `0` | `1` withholds every write tool |
| `HELLOASSO_TIMEOUT_SECS` | | `30` | Per-request timeout |
| `HELLOASSO_MAX_RETRIES` | | `3` | Retries on 429 and transient 5xx |
| `HELLOASSO_BASE_URL` | | per environment | API root override |
| `HELLOASSO_TOKEN_URL` | | per environment | Token endpoint override |
| `HELLOASSO_HTTP_BIND` | | | Listen address for the HTTP transport |
| `HELLOASSO_HTTP_TOKEN` | | | Bearer token the HTTP transport requires |
| `HELLOASSO_LOG` | | `info` | `warn`, `debug`, `mcp_helloasso=debug`… |

**Which organization.** An association's client reaches exactly one organization, and the
server finds it on its own. A partner client reaching several gets the list back as
candidates; set `HELLOASSO_ORGANIZATION` or pass `organization` per call. The binary
refuses to start on a malformed variable and names the one at fault.

## Connect a client

<details open>
<summary><b>Claude Code</b></summary>

```bash
claude mcp add helloasso --scope user \
  --env HELLOASSO_CLIENT_ID=… \
  --env HELLOASSO_CLIENT_SECRET=… \
  -- /absolute/path/to/mcp-helloasso
```
</details>

<details>
<summary><b>Claude Desktop</b></summary>

`claude_desktop_config.json`:

```json
{
  "mcpServers": {
    "helloasso": {
      "command": "/absolute/path/to/mcp-helloasso",
      "env": {
        "HELLOASSO_CLIENT_ID": "…",
        "HELLOASSO_CLIENT_SECRET": "…",
        "HELLOASSO_READ_ONLY": "1"
      }
    }
  }
}
```

Start read-only; lift it once you trust the setup.
</details>

<details>
<summary><b>Cursor, Windsurf, Zed, and other stdio clients</b></summary>

Same shape: one `command` pointing at the binary, the credentials in `env`. The server
speaks the stdio transport and prints nothing but protocol on stdout — logs go to stderr.
</details>

<details>
<summary><b>Any client, over HTTP</b></summary>

Point it at `http://127.0.0.1:8080/mcp` after starting the server with
`--http 127.0.0.1:8080`. See [below](#run-it-over-http-instead).
</details>

Then ask *"which HelloAsso organization are you connected to?"*. `whoami` should answer
with your association's name.

## Tools

38 tools in six toolsets, covering the 36 routes of the
[public v5 spec](https://api.helloasso.com/v5/swagger/public/swagger.json).
`HELLOASSO_TOOLSETS` picks which ones are registered; `HELLOASSO_READ_ONLY=1` keeps only
the ones that declare themselves read-only.

### sales — orders, items, payments, the treasurer's view

| Tool | Route | |
|---|---|---|
| `summarize_payments` | *composite* | Totals over a period: collected, per form, means, month, refunds |
| `list_orders` | `GET /organizations/{org}/orders` | Orders, by period, person or form type |
| `list_form_orders` | `GET …/forms/{type}/{slug}/orders` | The orders of one form |
| `get_order` | `GET /orders/{id}` | One order: payer, items, payments, refund eligibility |
| `cancel_order` | `POST /orders/{id}/cancel` | Stop future instalments — `confirm: true` |
| `list_items` | `GET /organizations/{org}/items` | Tickets, memberships, donations: the people |
| `list_form_items` | `GET …/forms/{type}/{slug}/items` | An event's attendees, a campaign's members |
| `get_item` | `GET /items/{id}` | One item, with custom fields and ticket URL |
| `list_payments` | `GET /organizations/{org}/payments` | Payments, by period, person or state |
| `list_form_payments` | `GET …/forms/{type}/{slug}/payments` | The payments of one form |
| `get_payment` | `GET /payments/{id}` | One payment, with refund operations and receipts |
| `refund_payment` | `POST /payments/{id}/refund` | Full or partial refund, MFA-aware — `confirm: true` |
| `export_cash_out` | `GET /organizations/{org}/cash-out/{id}/export` | A payout's detail, JSON, CSV or Excel |

### forms — the campaigns

| Tool | Route | |
|---|---|---|
| `list_forms` | `GET /organizations/{org}/forms` | Every form, by state and type |
| `list_form_types` | `GET /organizations/{org}/formTypes` | The types in use |
| `get_form` | `GET …/forms/{type}/{slug}/public` | A form in full: tiers, prices, custom fields |
| `get_form_stats` | `GET …/forms/{type}/{slug}/stats` | Participants and sales per tier |
| `quick_create_event` | `POST …/forms/Event/action/quick-create` | An event with tiers, in one call |
| `update_form_state` | `PUT …/forms/{type}/{slug}/state` | Publish, hide, disable — deleting needs `confirm: true` |
| `list_activity_types` | `GET /values/form/{type}/types` | Subtypes for a form type |

### checkout — payment links

| Tool | Route | |
|---|---|---|
| `create_checkout_intent` | `POST /organizations/{org}/checkout-intents` | A payment link for an amount, in one go or instalments |
| `get_checkout_intent` | `GET /organizations/{org}/checkout-intents/{id}` | Its outcome, and the order once paid |

### organization — who and what

| Tool | Route | |
|---|---|---|
| `whoami` | *composite* | Environment, client, default organization and its profile |
| `get_organization` | `GET /organizations/{org}` | An organization's public profile |
| `list_my_organizations` | `GET /users/me/organizations` | The organizations this client holds a role on |
| `list_organization_categories` | `GET /values/organization/categories` | Reference list |
| `list_company_legal_statuses` | `GET /values/company-legal-status` | Reference list |

### directory — HelloAsso's public directory

| Tool | Route | |
|---|---|---|
| `search_organizations` | `POST /directory/organizations` | By name, place, category, tag |
| `search_forms` | `POST /directory/forms` | By text, place, dates, type, price, tag |
| `get_tag` | `GET /tags/{name}` | One tag, with usage count and amount |
| `list_public_tags` | `GET /values/tags` | Every public tag |

### partner — for HelloAsso partners

| Tool | Route | |
|---|---|---|
| `get_partner` | `GET /partners/me` | The partner account |
| `list_partner_organizations` | `GET /partners/me/organizations` | Organizations that authorized the partner |
| `update_partner_domain` | `PUT /partners/me/api-clients` | The OAuth redirect domain |
| `set_notification_url` | `PUT /partners/me/api-notifications` | The webhook URL, main or per event type |
| `delete_notification_url` | `DELETE /partners/me/api-notifications` | Remove it — `confirm: true` |
| `set_organization_notification_url` | `PUT /partners/me/api-notifications/organizations/{org}` | Per-organization webhook |
| `delete_organization_notification_url` | `DELETE /partners/me/api-notifications/organizations/{org}` | Remove it — `confirm: true` |

## Resources and prompts

Three resources and two templates, for clients that prefer attaching context to calling
a function:

| URI | |
|---|---|
| `helloasso://organization` | The default organization's profile |
| `helloasso://forms` | Its forms |
| `helloasso://my-organizations` | Every organization the client reaches |
| `helloasso://forms/{form_type}/{form_slug}` | One form in full |
| `helloasso://orders/{order_id}` | One order in full |

Four prompts drive the workflows associations actually run:

- **`treasury_report`** — totals, per-campaign split, means, refunds and what stands
  out, in French number formatting. Writes nothing.
- **`find_person`** — everything one person did: memberships, tickets, payments,
  what is pending. Writes nothing.
- **`attendee_list`** — every participant of a form with their answers, walked to the
  last page and checked against the form's statistics. Writes nothing.
- **`refund_request`** — find the payment, check eligibility, propose, **stop and
  wait**, then refund and read it back.

## HelloAsso, as it actually behaves

Everything below was checked against the live API, is handled for you, and is written down because the next person
integrating HelloAsso will hit the same walls.

| What happens | What this server does |
|---|---|
| Access tokens last 30 minutes, and HelloAsso **requires** renewing them with the `refresh_token` rather than a new grant | Renews a minute early, refresh first, new grant only if the refresh is refused |
| A token can be revoked before it expires | A 401 drops the token and retries once with a fresh one |
| Amounts are integers in **cents** everywhere | Said in the server instructions and every tool description; `summarize_payments` adds euros |
| A form is addressed by `(organization, type, slug)` | Every form tool also takes the public URL, and French type names |
| Date filters are half-open: `to` is **exclusive** | Periods (`2026-09`, `last month`) are converted to the right `[from, to)` |
| Arrays in query strings are repeated keys (`states=A&states=B`) | Built that way, never comma-joined |
| Errors come in three envelopes: HelloAsso's `errors[]`, ASP.NET's validation map, OAuth's `error` | All three read into one message with the code in the error's `data` |
| A 403 says only `insufficient Privileges` | Names the privilege the spec requires for that route, and who grants it |
| An association's own client cannot call the directory, tag, activity-type or partner routes (`FormOpenDirectory`, `OrganizationOpenDirectory`, `FormAdministration`, partner client) | Those tools stay registered for partners; with an association client they answer which privilege is missing |
| Most listings (orders, items, payments, directory) report `-1` for totals and pages, and paginate by continuation token | `-1` is dropped rather than shown as a count; the token is surfaced, and `summarize_payments` walks it to the end |
| Refunds are protected by strong authentication | The MFA headers are exposed as optional arguments; the error says which one is needed |
| `quick-create` only supports events | Exposed as `quick_create_event`, which says so |
| The cash-out export picks JSON, CSV or Excel from the `Accept` header, and its CSV starts with a byte-order mark | `export_cash_out` takes a `format` and strips the BOM |
| Its CDN refuses requests without a `User-Agent` | Every request carries `mcp-helloasso/<version>` |
| 429 and transient 5xx | Retried with backoff, honouring `Retry-After` |

## Safety

Reading a treasury is sensitive; changing it is more so. The defaults lean careful.

- **Money is fail-closed.** `refund_payment`, `cancel_order`, deleting a form and
  removing a webhook refuse without `confirm: true`. A refused refund reads the payment
  first and says *"payment 67890 (25.00 € by Camille Martin, Authorized) would be
  refunded in full"*, so the person confirms what they actually see. All four are
  annotated `destructiveHint`.
- **Read-only is structural.** `HELLOASSO_READ_ONLY=1` drops every route that does not
  annotate itself `readOnlyHint` — not a list of names to keep in sync, so a new write
  tool is excluded by construction. A test asserts it.
- **Inputs are checked before HTTP.** Slugs cannot escape their path segment, checkout
  terms must add up to the total, URLs must be `https://`, ids must be positive.
- **The secret never leaks.** It is redacted in `Debug`, sent only to the token
  endpoint, and never serialized into a tool answer or a log line; `whoami` shows the
  first eight characters of the client id and nothing of the secret.
- **No disk writes over HTTP.** `export_cash_out`'s `save_to` is refused when the server
  is reached over HTTP.
- **The prompts ask first.** The only prompt that can move money stops for explicit
  approval, and asks for the MFA code rather than guessing.

## Run it over HTTP instead

```bash
mcp-helloasso --http 127.0.0.1:8080
# MCP at http://127.0.0.1:8080/mcp, health at /healthz
```

Off loopback, the server **refuses to start** without `HELLOASSO_HTTP_TOKEN`: one API
client means one identity, and an open endpoint is an open door to an association's
payments. With the token set, every request to `/mcp` needs
`Authorization: Bearer <token>`; `/healthz` stays open for probes. All sessions share one
OAuth token.

```bash
docker run -d --name mcp-helloasso -p 8080:8080 \
  -e HELLOASSO_CLIENT_ID=… \
  -e HELLOASSO_CLIENT_SECRET=… \
  -e HELLOASSO_HTTP_BIND=0.0.0.0:8080 \
  -e HELLOASSO_HTTP_TOKEN="$(openssl rand -hex 32)" \
  mcp-helloasso
```

Put TLS in front of it. The process speaks plain HTTP and does not pretend otherwise.

## Verify

```bash
cargo test                                   # 79 tests
cargo clippy --all-targets -- -D warnings
cargo fmt --all --check
mcp-helloasso --list-tools                   # the surface, as registered
mcp-helloasso --check                        # the configuration, against the live API
```

The suite includes the protocol itself: `tests/mcp_stdio.rs` spawns the real binary,
speaks JSON-RPC to its stdin and asserts on what comes back — the handshake, all 38 tools
with their schemas, a form named by URL, a period turned into dates, a summary that only
counts collected money, a refund refused without confirmation and sent with its MFA
header with it, read-only mode. `tests/api_contract.rs` holds the token lifecycle, the
retries and the error envelopes against a fake HelloAsso, and `tests/mcp_http.rs` checks
the HTTP transport and its bearer guard.

```
src/
├── client.rs      REST over HTTP: OAuth2 lifecycle, retries, the three error envelopes
├── config.rs      the environment, validated once, with the secret redacted
├── resolve.rs     URLs, slugs and form types, and the refusal to guess
├── dates.rs       today, last month, 2026-09, last 30 days
├── tools/         one module per toolset, one function per route
├── resources.rs   three URIs and two templates
├── prompts.rs     four workflows that ask before touching money
└── server.rs      capabilities, instructions, and the glue
```

## Releasing

Bump `version` in `Cargo.toml`, add its section to [CHANGELOG.md](CHANGELOG.md), then
push a tag:

```bash
git tag v0.1.1 && git push origin v0.1.1
```

The release workflow checks the tag against `Cargo.toml` and the changelog, builds the
five binaries with their checksums, pushes `ghcr.io/edouard-claude/helloasso-mcp`, and
publishes the GitHub release with the changelog section as notes.

## Contributing

Issues and pull requests welcome. A change is ready when `cargo test`,
`cargo clippy --all-targets -- -D warnings` and `cargo fmt --all --check` pass, and when a
new tool carries a description, an annotation and an output schema — three tests will
tell you if it does not.

## License

MIT — see [LICENSE](LICENSE). Not affiliated with HelloAsso; HelloAsso is a trademark of
its owner.
