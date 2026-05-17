# Decisions

- Add a repo-level `.gitignore` before the first remote push.
- Keep `Cargo.lock` trackable because pharness is an application/control-plane workspace, not just a library.
- Ignore Rust build output, local pharness SQLite state, generated smoke-test files, local logs, env files, local config overrides, kubeconfigs, and common credential file shapes.
- Keep planning docs, ADRs, source, migrations, README, and `config/pharness.example.toml` trackable.

# Backlog

- Revisit ignore rules if V2 adds checked-in Kubernetes fixture files whose names overlap with the conservative kubeconfig/credential patterns.
- Add CI fixture directories explicitly if future smoke tests generate non-`target/` artifacts.
