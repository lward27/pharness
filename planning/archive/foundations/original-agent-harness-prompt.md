You are Codex acting as a principal engineer and implementation planner.

I want to build my own lightweight agent harness, primarily targeting Fireworks AI as the LLM service provider.

Goal:
Build a minimal, fast, memory-efficient, highly customizable local-first agent runtime inspired by the core patterns of Claude Code, but without building a plugin marketplace, integration ecosystem, or heavy framework. Focus on making the core agent loop, context handling, tool execution, safety model, and developer workflow excellent.

Background:
Claude Code’s useful core patterns include: operating from the terminal/project root, reading and editing files across the repo, running shell commands and tests, iterating based on command output, understanding project structure, and treating the agent as an autonomous task executor rather than autocomplete. Claude Code is described as reading codebases, making changes, running tests, and delivering committed code; it also uses existing CLI tools like git, Kubernetes, and project tooling. Use those principles, but do not clone the product. Build a clean, small, open harness.

Hard constraints:
- Fireworks AI should be the primary model provider.
- V1 runs locally on my machine.
- V2 runs in my homelab Kubernetes cluster.
- Minimal UI only.
- Avoid integrations/plugins in early versions.
- Prefer fast and memory-performant languages.
- Different layers may use different languages:
- Keep architecture modular but not over-engineered.
- Design for local filesystem work, shell command execution, git-aware workflows, and later Kubernetes sandboxing.

Deliverables:
Create a multi-phase implementation plan with concrete steps, file/module structure, interfaces, milestones, and acceptance criteria. Write the plan down in a planning directory.

Architecture preference:
1. Core Runtime: Rust
   - Agent loop
   - State machine
   - Tool registry
   - File read/write tools
   - Shell command executor
   - Permission/safety policy
   - Context packer
   - Fireworks provider client
   - Event stream
   - Session persistence

2. API Layer: Rust HTTP server
   - API should expose sessions, messages, runs, events, tool approvals, and artifacts.
   - Prefer Server-Sent Events or WebSockets for streaming.

3. UI: TypeScript
   - Minimal local web UI.
   - Chat/task prompt.
   - Live event log.
   - Tool call approval cards.
   - Diff viewer.
   - Run status.
   - Session list.
   - No plugin marketplace, no integrations.

4. Storage:
   - V1: local SQLite or file-based session store.
   - Track messages, tool calls, approvals, file diffs, shell output summaries, and run metadata.
   - V2: Postgres optional for Kubernetes deployment.

Core principles:
- Small surface area.
- Explicit permissions.
- Human approval for destructive actions.
- Observable agent loop.
- Reproducible sessions.
- Local-first.
- Provider abstraction, but Fireworks-first.
- No hidden magic.
- Easy to reason about.

Agent loop requirements:
- Accept user task.
- Build initial context from working directory.
- Ask model for next action.
- Support actions:
  - respond
  - read_file
  - write_file
  - patch_file
  - list_dir
  - search_files
  - run_shell
  - git_diff
  - git_status
  - request_approval
  - finish
- Validate action against policy.
- Execute tool or pause for approval.
- Append result to context.
- Continue until finish, failure, or max turns.
- Stream every event to UI/API.

Safety model:
- Commands are classified before execution:
  - safe read-only
  - write local project
  - destructive local
  - network
  - privileged
  - secret-accessing
- Default:
  - read-only allowed.
  - file writes require approval until trusted mode.
  - destructive commands require explicit approval.
  - network commands require approval.
  - sudo/privileged commands denied by default.
- Add dry-run capability where possible.
- Add command timeout and output truncation.
- Never expose secrets in logs.
- Detect .env, keys, tokens, kubeconfigs, SSH keys.

V1 scope:
- Local CLI and minimal local web UI.
- Fireworks chat/completions integration.
- Local repo/task execution.
- File tools.
- Shell tool.
- Patch/diff workflow.
- Human approval.
- Session persistence.
- Basic context packing.
- No remote execution.
- No Kubernetes yet.
- No plugin architecture.
- No MCP initially unless explicitly justified as future-only.

V2 scope:
- Containerized runtime.
- Kubernetes deployment.
- Per-run workspace sandbox.
- Persistent volume or object storage for artifacts.
- Optional Postgres.
- API service.
- Web UI service.
- Worker pods for agent runs.
- Network and filesystem isolation.
- Resource limits.
- Secrets management.
- Ingress for web UI.
- Homelab-friendly Helm chart or Kustomize manifests.

Research tasks:
1. Study Claude Code’s publicly documented workflow patterns:
   - terminal/project-root operation
   - full-codebase context discovery
   - file editing
   - command/test iteration
   - permissioned tool use
   - git-aware workflow
2. Study Cursor SDK as a comparison:
   - local vs cloud runs
   - streaming event model
   - durable run/session model
   - cancellation
   - self-hosted workers
   - why we are or are not using it
3. Study Fireworks AI API:
   - chat/completions API shape
   - streaming
   - function/tool calling support if available
   - model configuration
   - error handling
   - rate limits
4. Recommend the best model interaction pattern:
   - native tool calling if Fireworks supports it well
   - otherwise structured JSON action protocol
   - include robust JSON repair/validation strategy

Implementation plan format:
Break the work into phases:

Phase 0: Architecture decisions and repo setup
Phase 1: Fireworks provider client
Phase 2: Rust core agent loop
Phase 3: Local filesystem and shell tools
Phase 4: Safety and approval system
Phase 5: Session/event persistence
Phase 6: CLI
Phase 7: Minimal API
Phase 8: Minimal TypeScript UI
Phase 9: Local dogfooding on real tasks
Phase 10: V2 Kubernetes/homelab deployment
Phase 11: hardening, observability, and future extensions

For each phase include:
- goal
- implementation steps
- suggested files/modules
- acceptance criteria
- risks/tradeoffs

Also include:
- Proposed repo structure.
- Rust crate/module structure.
- API endpoint sketch.
- Event schema.
- Tool call schema.
- Session database schema.
- Fireworks provider interface.
- Example system prompt for the agent.
- Example local config file.
- Example user flow.
- Testing strategy.
- Security checklist.
- Explicit non-goals.

Important:
Keep the plan practical. Avoid vague “build agent framework” language. Give me a sequence that Codex can execute step-by-step in a real repo.