# Decisions

- `patch_file` is now a structured exact text replacement action, not a raw unified-diff string. The patch payload is `{ find, replace, replace_all }`.
- Default behavior requires exactly one match. Ambiguous edits fail unless `replace_all=true`; this keeps V1 patching predictable and reviewable.
- `patch_file` targets existing UTF-8 files inside the workspace and uses the same approval policy as `write_file`.
- `patch_file` is exposed in the default Fireworks worker schema now that execution, approval resume, and diff persistence are covered.
- Live smoke passed: the model proposed `patch_file`, policy created a `file_write` approval, approval resumed the exact reviewed action, the file was patched, the model finished, and run diff retrieval reported one stored change.
- Pending `patch_file` approvals now persist a best-effort generated preview diff on the approval row before the action executes.

# Backlog

- Do not accept arbitrary unified diffs until the harness has stronger patch parsing, conflict reporting, and fixture coverage.
- Consider richer patch forms later, such as line-range replacement or multi-hunk patches, after exact find/replace proves insufficient.
