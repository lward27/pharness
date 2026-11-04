# PHarness Operator Tutorial Site

A small static tutorial site for operating PHarness: build and test the
codebase, move a WorkItem through the coding harness, and take a verified
source change to a verified release.

## View it

No build step. Open `index.html` directly, or serve it:

```sh
cd tutorial-site
python3 -m http.server 8080
# then open http://127.0.0.1:8080
```

## Contents

| File | Page |
| --- | --- |
| `index.html` | Overview, prerequisites, build & test locally, local configuration |
| `workitems.html` | WorkItem lifecycle, create → plan → execute → verify → delivery |
| `release.html` | Build the seven release images, GitOps pinning, release verification, production approval |
| `reference.html` | CLI cheat sheet, config reference, troubleshooting, safety rules |
| `css/site.css` | Styles |
| `js/site.js` | Copy buttons on code blocks, sidebar active-link highlighting |

## Provenance

Content is generated from the repository itself: `README.md`,
`config/pharness.example.toml`, the `planning/` documentation (product model,
stage outcomes, operations runbooks), `scripts/`, and the CLI source
(`crates/pharness-cli/src/main.rs`). The WorkItem state machine on the
lifecycle page mirrors
`crates/pharness-api/src/app/work_items/lifecycle.rs`. If the code changes,
the source and runbooks win over this site.

This directory is an additive asset: it does not modify any PHarness code,
config, or deployment files.
