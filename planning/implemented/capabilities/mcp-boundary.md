# Decisions

- MCP is a future adapter path, not a V1 dependency. pharness should keep progressing with native typed capabilities until MCP clearly reduces integration cost.
- Keep the "no massive plugin library" stance. Supporting MCP later does not mean adding a plugin marketplace or letting arbitrary tool catalogs into the core runtime.
- Treat MCP servers as governed `ToolServer` implementations when they arrive. They must be registered, scoped, policy-evaluated, audited, and redacted like any other capability provider.
- Use MCP first for high-value external collaboration systems where native implementation would be wasteful: Jira, Slack, documentation systems, and other workflow tools around the SDLC.
- Do not use MCP as the primary interface for core cluster delivery capabilities unless it proves better than typed native adapters. Kubernetes, Argo, Tekton, registry, database, and LGTM paths still need first-class resource refs, policy decisions, and durable artifacts.
- Keep provider and tool execution boundaries explicit. A model should not discover arbitrary MCP tools at runtime without pharness turning them into approved capability definitions.

# Backlog

- Add a future `ToolServer` registry that can represent native adapters and MCP-backed adapters with the same policy/audit envelope.
- Add allowlisted MCP server config with server identity, capability names, environment scope, secret requirements, and policy class.
- Add MCP result redaction and artifact capture before exposing results to model context or event logs.
- Add MCP conformance tests with a fake server before wiring any real Jira or Slack integration.
- Add operator UX for approving an MCP server's capability set before enabling it for autonomous runs.
- Revisit MCP after V1 stable and after the first cluster-native typed capabilities are reliable enough to define the expected audit contract.
