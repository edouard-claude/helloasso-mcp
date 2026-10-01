# Changelog

All notable changes to this project are documented here. The format follows
[Keep a Changelog](https://keepachangelog.com/en/1.1.0/), and the project uses
[Semantic Versioning](https://semver.org/).

## [0.1.0] - 2026-10-01

First release: the whole HelloAsso v5 API as an MCP server.

### Added

- 38 tools: the 36 routes of the public v5 spec, one tool each, plus `whoami`
  and `summarize_payments`.
- Six toolsets (`organization`, `forms`, `sales`, `checkout`, `directory`,
  `partner`) selectable with `HELLOASSO_TOOLSETS`, and `HELLOASSO_READ_ONLY`.
- OAuth2 `client_credentials` with `refresh_token` renewal, as HelloAsso
  requires, and a transparent retry on a revoked token.
- Forms named by their public URL, form types in English or French, periods
  such as `last month` or `2026-09`.
- Refunds, cancellations and deletions refused without `confirm: true`, the
  refusal stating the amount and the payer; MFA headers for refunds.
- Cash-out export as JSON, CSV or Excel.
- Three resources, two resource templates and four prompts
  (`treasury_report`, `find_person`, `attendee_list`, `refund_request`).
- stdio and Streamable HTTP transports, the latter behind a bearer token.
- Prebuilt binaries for macOS, Linux and Windows, and a Docker image on GHCR.
