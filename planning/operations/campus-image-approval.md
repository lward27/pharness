# Campus image, dependency and installer gates

This playbook covers how a PHarness release image becomes an approved Campus
lab template image, how that approval is enforced at admission, how
vulnerability exceptions work, and how package installs inside a lab session
are kept to reviewed indexes and per-session caches.

Current state: two lab runner images are approved, both from release
`9761707332da8f6558d5c35e50af73bbe731734b` with 0 blocking findings and no
exceptions:

- `registry.lucas.engineering/pharness-python-runner@sha256:bb5cc07a648ba4803a87aff4129cd21434b4364d098a2723b96d4952c0bb7400`
  (Research profile)
- `registry.lucas.engineering/pharness-node-runner@sha256:3cc3cd8dc39fc89e2cd443156a96b401d4da38caf5e0762d17f2fa2ec7345acd`
  (Coding profile)

The runner images from release `8f7338c792fc88d4b2b700b45b2161108b39e0bf`
were blocked by the vulnerability gate and remain recorded as blocked (details
below). Any other digest is rejected in namespaces labelled
`pharness.dev/campus-lab=true`. No exception has been recorded; deciding
whether to accept findings belongs to the owner, not to the tooling.

## Contract

| Piece | Location |
| --- | --- |
| Reviewed evidence for every assessed digest | `deploy/campus/approved-images.json` |
| Reviewed vulnerability exceptions | `deploy/campus/vulnerability-exceptions.json` |
| Approved Python and npm indexes | `deploy/campus/package-sources.json` |
| Signing public key (ECDSA P-256) | `deploy/campus/cosign.pub` |
| Key qualification records | `deploy/campus/key-qualification/` |
| Generated admission policy and binding | `deploy/campus/admission/campus-lab-image-allowlist.yaml` |
| Signing private key | Secret `tekton-pipelines/campus-cosign-signing` (never in Git) |
| Policy code (also embedded in the TaskRun) | `scripts/campus_image_policy.py` |
| Release check, TaskRun rendering, evidence, admission rendering | `scripts/campus_image_gate.py` |

Public key fingerprint (SHA-256 of the DER SubjectPublicKeyInfo):
`sha256:44c11d20f4503df3598c031f21b87b6f9cd62efa9675ff1a5e065d13c0d9a0e4`.

An image is approved only when all of these hold:

1. The digest is the one recorded for its component in the immutable release
   record (`dist/releases/<revision>/BUILD.json` and `ARTIFACTS.json`), and
   `registry.lucas.engineering` serves exactly that digest.
2. A TaskRun in `tekton-pipelines` produced an SPDX SBOM with syft and a grype
   scan of that SBOM, using the digest-pinned tool images recorded in the
   evidence entry.
3. No Critical or High finding with a fix available remains, other than ones
   covered by an unexpired exception.
4. The SBOM and the vulnerability disposition are attested to the digest, the
   digest is signed with the Campus key, and all three verify against
   `deploy/campus/cosign.pub` from the workstation before evidence is written.
5. The entry in `approved-images.json` has `status: approved`, and the
   regenerated admission policy containing the digest is reviewed, merged and
   applied.

Blocked digests are still recorded, with their SBOM and vulnerability
attestations, so the reason they cannot run stays reviewable. A blocked digest
never receives an approval signature; online verification fails if one appears.

## Tools

Install the pinned CLIs (checksum-verified, user-local):

```bash
scripts/campus-install-supply-chain-tools.sh            # installs into ~/.local/bin
scripts/campus-install-supply-chain-tools.sh --print-versions
```

Pinned versions: cosign 3.1.4, syft 1.56.0, grype 0.121.1. The TaskRun uses the
same versions as digest-pinned images (`docker.io/anchore/syft:v1.56.0`,
`docker.io/anchore/grype:v0.121.1`, `ghcr.io/sigstore/cosign/cosign:v3.1.4`)
plus `docker.io/library/python:3.11-slim` for the policy step. Changing a tool
version means updating both the installer and the image pins in
`scripts/campus_image_gate.py`, then rerunning approval for every approved
digest.

## Approving or updating an image

Prerequisites: kubectl context `lucas_engineering`, read access to
`tekton-pipelines` object names, and the release record for the revision.
The script reads only Secret names, never Secret data.

```bash
scripts/campus-image-approve.sh \
  --image registry.lucas.engineering/pharness-python-runner@sha256:<digest> \
  --revision <40-character source revision> \
  [--release-dir dist/releases/<revision>]
```

The script:

1. checks the digest against the release record and the registry;
2. runs `campus_image_gate.py check` so a malformed or expired exception file
   stops the run before anything is created;
3. creates a TaskRun `campus-approve-<component>-<digest prefix>-<time>` with
   steps `sbom`, `scan`, `evaluate`, `attest-sbom`,
   `attest-vulnerabilities`, `enforce` and `sign`. `enforce` exits 3 when the
   decision is blocked, so `sign` never runs for a blocked image. The private
   key is mounted only into the attest and sign steps;
4. verifies the attestations (and the signature, if any) locally against the
   committed public key, records the evidence entry, and regenerates the
   admission policy;
5. deletes the TaskRun unless `--keep-run` is given.

If workstation-side verification fails after a TaskRun has finished, rerun with
`--from-taskrun <name>` (and `--keep-run` on the first attempt) to collect the
evidence again without rescanning.

Then review the diff to `deploy/campus/`, commit, and after merge apply the
policy:

```bash
kubectl --context lucas_engineering diff -f deploy/campus/admission/campus-lab-image-allowlist.yaml
kubectl --context lucas_engineering apply -f deploy/campus/admission/campus-lab-image-allowlist.yaml
```

Updating an image is the same procedure for the new digest. A new digest has no
approval until it is assessed; there is no tag-following.

### Revoking an image

Set the entry's `status` to `revoked`, regenerate the policy with
`python3 scripts/campus_image_gate.py render-policy --evidence deploy/campus/approved-images.json > deploy/campus/admission/campus-lab-image-allowlist.yaml`,
review, merge and apply. The policy only checks images that are new in a
request, so Pods that are already running are not evicted; delete the affected
session Pods explicitly if the revocation is urgent. The cosign signature stays
in the registry and still verifies cryptographically. It no longer counts
because admission and approval both follow the reviewed evidence file, not the
signature.

## Vulnerability gate and exceptions

The policy blocks any finding whose severity is Critical or High and whose
grype fix state is `fixed`. Findings at those severities with no fix are
recorded as `contain-no-fix` and do not block; Medium and lower are
`below-threshold`. All findings, including the non-blocking ones, are in the
attested vulnerability predicate
(`https://pharness.dev/attestations/campus-vulnerability-disposition/v1`).

An exception covers exactly one vulnerability id, package name and image
repository:

```json
{
  "id": "CVE-2026-0000",
  "package": "openssl",
  "repository": "registry.lucas.engineering/pharness-python-runner",
  "owner": "Named person who accepts the risk",
  "reviewer": "A different named person",
  "reason": "Why the finding is not reachable or is contained, in at least 20 characters",
  "created": "2026-11-04",
  "expires": "2027-01-31"
}
```

The rules, enforced by `campus_image_policy.py` and by CI:

- every field is required and no other fields are accepted;
- owner and reviewer must differ;
- `expires` must be after `created` and no more than 90 days later;
- duplicate id/package/repository entries are rejected;
- an expired exception no longer covers its finding. The finding becomes
  `blocked-exception-expired`, which blocks approval, and `check` fails CI for
  any approved digest that depended on it;
- the SHA-256 of the exceptions file used is recorded in each evaluation.

Exceptions are a reviewed decision by the named owner. Do not add them to make
an approval pass.

## Verification

Offline, in CI on every change (`.github/workflows/ci.yml` runs the tests
below; the shell test calls the offline check):

```bash
scripts/campus-image-verify.sh --offline
python3 scripts/tests/campus-image-gate-test.py
bash scripts/tests/campus-image-gates-test.sh
```

The offline check confirms that the evidence, exceptions, public key
fingerprint, key qualification records and generated admission policy are
mutually consistent, that the committed policy matches what `render-policy`
produces, and that no approved digest depends on a missing or expired
exception.

Online, before relying on the allowlist for a release (needs registry access
and the pinned cosign):

```bash
scripts/campus-image-verify.sh
```

This verifies, for every recorded digest, that the SBOM and vulnerability
attestations in the registry verify with the committed key and match the
recorded digests; that every approved digest has a verifying signature bound to
its repository and source revision; that no blocked digest has one; and that
each key qualification signature verifies.

To check the in-cluster behaviour of the policy and the package cache controls:

```bash
scripts/campus-lab-cluster-test.sh [--remove-policy]
```

It runs in `campus-lab-spike` only, labels that namespace for the run, uses a
separate test-only policy (`pharness-campus-lab-image-allowlist-test`, built
from `scripts/tests/fixtures/campus/admission-test-evidence.json`) for the
cases that need an allowlisted digest, deletes every Pod it creates, removes
the labels, and leaves the committed policy applied unless `--remove-policy` is
given. The fixture marks the two runner digests as allowed for this test only;
that is not an approval.

## Admission policy

`pharness-campus-lab-image-allowlist` is a built-in ValidatingAdmissionPolicy
with `failurePolicy: Fail` and a `Deny` binding. It matches only namespaces
labelled `pharness.dev/campus-lab=true`, and covers Pods (including the
`ephemeralcontainers` subresource), Jobs, CronJobs, Deployments, ReplicaSets,
StatefulSets and DaemonSets on create and update. Every container,
initContainer and ephemeral container image that is new in the request must:

- be a fully qualified `repository@sha256:<64 lowercase hex>` reference with no
  tag, and
- appear in the generated allowlist.

Rejections name the offending images, for example:

```text
Campus lab images must be fully qualified immutable references (repository@sha256:<64 lowercase hex>, no tag); rejected: registry.lucas.engineering/pharness-python-runner:latest
Campus lab image digest is not approved in deploy/campus/approved-images.json; rejected: registry.lucas.engineering/pharness-runtime@sha256:95a4...
```

The in-cluster run on 2026-11-04 passed for: allowlisted Pod and Job admitted;
tag-only, `:latest`, tag-plus-digest, unknown digest and single-character
tampered digest rejected; the same for initContainers and ephemeral containers;
Job, CronJob and Deployment templates rejected; a Pod image update to a
tampered digest rejected; a metadata-only update of a running Pod admitted; and
a tag-only Pod in the unlabelled `tekton-pipelines` namespace still admitted
(scope check, dry-run).

No namespace currently carries `pharness.dev/campus-lab=true`. Labelling a
namespace while the allowlist is empty blocks all workloads in it.

## Package indexes and caches

`deploy/campus/package-sources.json` lists the indexes a lab session may use:
`https://pypi.org/simple` and `https://registry.npmjs.org/`.
`campus_image_gate.py render-lab-pod` produces a session Pod with:

- `PIP_INDEX_URL`, `UV_INDEX_URL` and `NPM_CONFIG_REGISTRY` set to the first
  approved entry, `PIP_CONFIG_FILE=/dev/null` and
  `NPM_CONFIG_USERCONFIG=/dev/null` so image or user configuration cannot
  redirect them;
- `PIP_CACHE_DIR`, `UV_CACHE_DIR`, `NPM_CONFIG_CACHE` and `XDG_CACHE_HOME` under
  `/var/cache/campus`, which is a per-Pod `emptyDir`;
- gVisor, non-root, read-only root filesystem, no service account token.

`check-lab-pod` rejects Pods that use non-`emptyDir` volumes, unapproved or
indirect (`valueFrom`) index settings, extra index or trusted-host settings, or
caches outside the per-session volume. The cluster test planted poisoned pip
and npm cache entries in one session Pod and confirmed that a second session
Pod on the same image saw an empty cache.

These settings are configuration, not a network control. A student can still
override the environment inside the session. Restricting egress to the
approved index hosts is LUC-130; until it lands, treat package installs as
unrestricted network access.

## Signing key custody and rotation

The private key exists only in Secret `tekton-pipelines/campus-cosign-signing`
(keys `cosign.key`, `cosign.password`, `cosign.pub`, labelled
`pharness.dev/story=LUC-121`). It was generated in-cluster and was never
written to a workstation or to Git. Do not read, copy or print it.

After creating or rotating the Secret, prove it matches the committed public
key:

```bash
scripts/campus-image-approve.sh --key-qualification
```

This signs a payload that states it carries no approval, verifies it locally
with `deploy/campus/cosign.pub`, and appends a record under
`deploy/campus/key-qualification/` and to `approved-images.json`.

To rotate: generate a new key pair into a new Secret in-cluster, replace
`deploy/campus/cosign.pub` and the `signing.public_key_sha256` fingerprint, run
key qualification, then re-approve every approved digest so its signature and
attestations are made with the new key. Online verification fails until that
is done, which is intended. Delete the old Secret only after the new evidence
is merged.

## Current assessment (2026-11-04, grype database built 2026-11-04T06:20:59Z)

| Image | Status | Findings | Blocking (Critical/High with fix) |
| --- | --- | --- | --- |
| `pharness-python-runner@sha256:0dcdb9e7…9756` (Debian 13.6) | blocked | 459 (C18 H180 M133 L28 N100) | 91 (C10 H81) |
| `pharness-node-runner@sha256:c478a31d…a7bd` (Debian 12.15) | blocked | 352 (C11 H98 M119 L15 N109) | 18 (C1 H17) |

Python runner blockers: OpenSSL 3.5.6 (`openssl`, `libssl3t64`,
`openssl-provider-legacy`; fixed in 3.5.7), CPython 3.11.16, glibc
(CVE-2026-5450 Critical, CVE-2026-5928), the util-linux 2.41-5 family
(CVE-2026-53612 to 53615), `libpcre2-8-0`, `gzip`, `libsqlite3-0`, and the
Python packages `jaraco-context` 5.3.0 and `wheel` 0.45.1.

Node runner blockers: Node.js 24.20.0 (CVE-2026-111497 Critical and three High;
fixed in 24.22.1), `libpcre2-8-0` 10.42-1, and the npm packages `undici`
6.27.0, `brace-expansion` 5.0.7, `tar` 7.5.19 and `ip-address` 10.2.0.

Both images have SBOM and vulnerability attestations in the registry and no
approval signature. The full finding lists are in `approved-images.json` and in
the attested predicates. The path forward is to rebuild the runner images on
patched bases and package versions through the normal release process, then
run approval on the new digests. The other images from the same release
(runtime, ui, model-gateway, eval-runner, codex-host) also have Critical/High
findings with fixes, but they are not Campus lab images.

## Residual risks

- Admission checks a digest allowlist, not a live signature. Security rests on
  review of `approved-images.json` and on who can change the cluster policy.
  Anyone with permission to edit ValidatingAdmissionPolicies or namespace
  labels can bypass it.
- Revocation is removal from the allowlist and does not stop Pods that are
  already running.
- An approved image can become vulnerable after approval. Rescan approved
  digests regularly; containment for signed-but-vulnerable images still
  depends on the runtime controls (threats T12 and T15 in the Campus project threat model).
- The private key is in a cluster Secret. Anyone who can read Secrets in
  `tekton-pipelines` can sign. There is no hardware or KMS backing.
- No transparency log is used. Signatures live only in
  `registry.lucas.engineering`, so there is no public record of signing
  events.
- cosign 3.1.4 is used with the legacy flags `--tlog-upload=false`,
  `--new-bundle-format=false` and `--use-signing-config=false`. They are
  deprecated, and a later cosign will require a signing-config file instead.
- Package index pins are configuration only until LUC-130 enforces egress.
- grype's view of language runtimes installed outside a package manager (the
  `UnknownPackage` entries for `python` and `node`) depends on its binary
  classifiers. A missed runtime would not be gated.
