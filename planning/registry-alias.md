# Decisions

- Add registry image identity normalization to Tekton PipelineRun analysis.
- Configure aliases with `PHARNESS_REGISTRY_ALIASES` as comma-separated `left=right` pairs. Example: `docker-registry.registry.svc.cluster.local:5000=registry.lucas.engineering`.
- Keep unconfigured registry differences visible as `registry_mismatch`. The tool should not silently decide that two registries are equivalent.
- Keep exact image string matches as `exact_match`.
- Treat configured host-equivalent image matches as `registry_alias_match` only when repository and version identity also match.
- Include parsed image references in `image_alignment` so operators can see what pharness compared.
- Live smoke passed against `finance-frontend-run-6mwcl`: with `PHARNESS_REGISTRY_ALIASES=docker-registry.registry.svc.cluster.local:5000=registry.lucas.engineering`, the analysis returned `image_alignment.status: registry_alias_match`.
- Registry aliases can now also be set in parsed runtime config under `[cluster].registry_aliases`; env still wins for ad hoc dogfooding.

# Backlog

- Add digest-aware deployment correlation when the deployed image includes a digest instead of only a tag.
- Add registry read capability later to verify whether two tags resolve to the same digest, behind an explicit read-only registry policy.
