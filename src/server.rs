//! The MCP server: state, capabilities, and the glue between the routers.

use std::sync::Arc;

use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler,
    handler::server::router::{prompt::PromptRouter, tool::ToolRouter},
    model::{
        Implementation, InitializeResult, ListResourceTemplatesResult, ListResourcesResult,
        PaginatedRequestParams, ReadResourceRequestParams, ReadResourceResponse,
        ServerCapabilities, ServerConfig, Tool,
    },
    service::RequestContext,
};

use crate::{client::HelloAsso, config::Config, resolve::Resolver, resources};

/// What the server tells a client about itself, once, at initialization.
///
/// This is the only place a model reads before deciding which tool to call, so
/// it carries what it cannot guess: amounts are cents, a form is named by type
/// and slug (or by its URL), `to` is exclusive, and refunds are real money.
pub const INSTRUCTIONS: &str = "\
HelloAsso: the payment platform French associations use for memberships \
(adhésions), events (billetterie), donations, crowdfunding and shops.

Start with `whoami` to see which organization this server acts on and what the \
API client may do. Every tool taking `organization` defaults to that one.

Vocabulary: a **form** (formulaire) is a campaign — Membership, Event, Donation, \
CrowdFunding, PaymentForm, Shop or Checkout — named by its type and slug, or by \
its public URL (`https://www.helloasso.com/associations/{org}/evenements/{slug}`), \
which every form tool accepts as `form_url`. An **order** (commande) is one \
checkout by one payer; it holds **items** (one per ticket, membership or \
donation, carrying the participant and custom fields) and **payments** (one per \
instalment). A **cash-out** (versement) is HelloAsso paying the organization.

**All amounts are in cents**: 1500 means 15,00 €. Periods accept `this month`, \
`last month`, `2026-09`, `2026`, `last 30 days`; dates accept `YYYY-MM-DD`, \
`today`, `yesterday`. Upper bounds (`to`) are exclusive.

To find a person, pass `search` (name or email) to `list_orders`, `list_items` \
or `list_payments`. For totals, use `summarize_payments` instead of adding pages \
by hand.

`refund_payment` and `cancel_order` move real money and cannot be undone: they \
refuse to run without `confirm: true`. Show the payment and the amount, get the \
person's explicit agreement first. Refunds may additionally require strong \
authentication (MFA): the error then says which header to send.";

/// One MCP server over one HelloAsso API client.
pub struct HelloAssoServer {
    resolver: Arc<Resolver>,
    tool_router: ToolRouter<Self>,
    prompt_router: PromptRouter<Self>,
}

impl std::fmt::Debug for HelloAssoServer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("HelloAssoServer")
            .field("tools", &self.tool_router.list_all().len())
            .finish_non_exhaustive()
    }
}

impl HelloAssoServer {
    /// Build a server from a configuration, wiring only the enabled toolsets.
    #[expect(
        clippy::needless_pass_by_value,
        reason = "one server owns one configuration; taking it by value says so"
    )]
    pub fn new(config: Config) -> crate::error::Result<Self> {
        let client = Arc::new(HelloAsso::new(config.clone())?);
        Ok(Self::with_client(client, &config))
    }

    /// Build a server around an existing client, so HTTP sessions share one
    /// token instead of each asking HelloAsso for its own.
    pub fn with_client(client: Arc<HelloAsso>, config: &Config) -> Self {
        Self {
            resolver: Arc::new(Resolver::new(client)),
            tool_router: crate::tools::router(config),
            prompt_router: crate::prompts::router(),
        }
    }

    /// The organization and form resolver.
    pub fn resolver(&self) -> &Arc<Resolver> {
        &self.resolver
    }

    /// The HTTP client.
    pub fn client(&self) -> &Arc<HelloAsso> {
        self.resolver.client()
    }

    /// The configuration in force.
    pub fn config(&self) -> &Config {
        self.client().config()
    }

    /// Every tool this server exposes, which is what `--list-tools` prints.
    pub fn tools(&self) -> Vec<Tool> {
        let mut tools = self.tool_router.list_all();
        tools.sort_by(|a, b| a.name.cmp(&b.name));
        tools
    }
}

#[rmcp::tool_handler(router = self.tool_router)]
#[rmcp::prompt_handler(router = self.prompt_router)]
impl ServerHandler for HelloAssoServer {
    fn get_info(&self) -> ServerConfig {
        InitializeResult::new(
            ServerCapabilities::builder()
                .enable_tools()
                .enable_prompts()
                .enable_resources()
                .build(),
        )
        .with_server_info(
            Implementation::new(crate::SERVER_NAME, crate::SERVER_VERSION)
                .with_title("HelloAsso")
                .with_website_url("https://github.com/edouard-claude/helloasso-mcp"),
        )
        .with_instructions(INSTRUCTIONS)
    }

    fn list_resources(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<ListResourcesResult, McpError>> + Send + '_ {
        std::future::ready(Ok(ListResourcesResult::with_all_items(
            resources::catalogue(),
        )))
    }

    fn list_resource_templates(
        &self,
        _request: Option<PaginatedRequestParams>,
        _context: RequestContext<RoleServer>,
    ) -> impl Future<Output = std::result::Result<ListResourceTemplatesResult, McpError>> + Send + '_
    {
        std::future::ready(Ok(ListResourceTemplatesResult::with_all_items(
            resources::templates(),
        )))
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _context: RequestContext<RoleServer>,
    ) -> std::result::Result<ReadResourceResponse, McpError> {
        resources::read(self, &request.uri)
            .await
            .map(Into::into)
            .map_err(Into::into)
    }
}
