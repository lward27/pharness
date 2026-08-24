# Decisions

- Move typed runtime config into `pharness-config` now that API startup and CLI validation both need the same parser. This is the point where the shared crate earns its keep.
- Auto-load `config/pharness.toml` when present and support `PHARNESS_CONFIG` for explicit paths. Missing default config is not an error; missing explicit config is an operator mistake and should fail startup.
- Keep env overrides authoritative for bind address, storage path, Fireworks model/base URL, cluster tool settings, Prometheus URL, and registry aliases. This keeps smoke tests and one-off dogfooding cheap.
- Keep secrets out of TOML. Config stores `api_key_env`; the API resolves the key from the environment and never returns it through `/api/config/effective`.
- Inject resolved `ReadOnlyClusterTools` into both direct capabilities and local worker runs. Cluster behavior should not depend on scattered runtime env reads once the API has started.
- Keep V1 Fireworks-only. If a config file sets another provider, startup fails instead of pretending provider abstraction is ready.
- Live config smoke passed on a throwaway port: `PHARNESS_CONFIG` loaded a temp TOML file, `/api/config/effective` returned `cluster.registry_alias_count = 1`, and no secret value was exposed.
- Add `pharness-cli config validate --file ...` as an offline check. It validates TOML, env precedence, V1 provider support, and secret resolution state without starting the API or printing secret values.
- Offline validation smoke passed against a temp TOML file: output reported `status = ok`, `model.api_key_configured = true`, and `cluster.registry_alias_count = 1`.
- Move `[policy]` into typed runtime config now. The API uses it for direct capability decisions and as the default worker policy, while `POST /api/runs` may override only `policy_mode`.
- Persist the selected `SafetyPolicy` inside each run's execution target so resume behavior uses the same policy that created the approval gate.
- Config validation smoke against `config/pharness.example.toml` now reports a non-secret `policy` object with subject, environment, `mode = default`, write/network/destructive approval flags, and secret/privileged denials.
- Require non-blank policy subject and environment at config load time. A blank trust boundary should fail before the API starts.

# Backlog

- Add config-source metadata to `/api/config/effective`, excluding secrets, so operators can tell whether a value came from defaults, TOML, or env.
- Add Kubernetes deployment config mapping for V2 so the same TOML semantics can become ConfigMap and Secret inputs.
