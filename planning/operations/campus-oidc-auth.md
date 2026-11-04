# Campus OIDC sessions and course authorization

Status: implemented in source, disabled by default, not deployed. The Helm
chart does not set any Campus variable. This page covers configuration, the
HTTP contract, and residual risks for the `/api/campus/*` boundary.

## Boundary

Campus is a separate student-facing surface with its own router, middleware
and error type (`crates/pharness-api/src/app/campus/`).

- Only the `__Host-pharness-campus` session cookie authenticates Campus
  routes. Operator (`PHARNESS_OPERATOR_TOKENS`) and worker
  (`PHARNESS_WORKER_TOKEN`) bearer tokens are never read there.
- Operator, SDLC and `/api/internal/*` routes never read cookies, so a Campus
  session cannot authorize them. Tests send a tenant-admin Campus session to
  every route in `crates/pharness-api/tests/fixtures/app-route-inventory.tsv`
  and expect 401.
- IdP tokens never authorize anything. The id_token is validated once in the
  callback and then dropped. The access token is never read. Neither is
  stored, logged, returned or forwarded, including to LiteLLM or any model
  gateway. Only the server-side session authorizes Campus requests.
- Tenant and actor come only from the session. Email and name claims are
  display-only. The identity key is `(tenant_id, issuer, subject)`.

## Fail-closed startup

- Campus is off unless `PHARNESS_CAMPUS_ENABLED` is `true`, `1` or `yes`.
  Any other non-empty value is a startup error. While it is off, every
  `/api/campus` path returns 404 and no cookie is set.
- When Campus is on, a missing or invalid value stops startup with an error
  that names the variable. The client secret value is never printed.
- When Campus is on, `PHARNESS_OPERATOR_TOKENS` must contain at least one
  token, even on a loopback bind. This keeps SDLC routes from being
  loopback-open next to a student-facing surface.
- In `PHARNESS_OPERATIONAL_MODE=read_only`, Campus answers 503 because it
  cannot record logins or sessions.

## Configuration

All endpoints are explicit. No discovery document is fetched.

| Variable | Required | Default and bounds |
| --- | --- | --- |
| `PHARNESS_CAMPUS_ENABLED` | no | `false` |
| `PHARNESS_CAMPUS_TENANT_ID` | yes | 1-64 of `[A-Za-z0-9._-]`; one tenant per deployment |
| `PHARNESS_CAMPUS_PUBLIC_ORIGIN` | yes | Browser origin, e.g. `https://campus.example.edu`; no path or query |
| `PHARNESS_CAMPUS_OIDC_ISSUER` | yes | Exact `iss`, compared byte-for-byte |
| `PHARNESS_CAMPUS_OIDC_CLIENT_ID` | yes | Must appear in `aud` |
| `PHARNESS_CAMPUS_OIDC_CLIENT_SECRET_FILE` | yes | Path to a file holding only the secret; mount it from a Secret |
| `PHARNESS_CAMPUS_OIDC_AUTHORIZATION_ENDPOINT` | yes | https URL; existing query parameters are kept |
| `PHARNESS_CAMPUS_OIDC_TOKEN_ENDPOINT` | yes | https URL; uses `client_secret_basic` |
| `PHARNESS_CAMPUS_OIDC_JWKS_URI` | yes | https URL |
| `PHARNESS_CAMPUS_OIDC_REDIRECT_URI` | yes | Must equal `<public origin>/api/campus/auth/callback` |
| `PHARNESS_CAMPUS_OIDC_SCOPES` | no | `openid email profile`; must include `openid` |
| `PHARNESS_CAMPUS_OIDC_ALLOWED_ALGS` | no | `RS256,ES256`; also allows RS384/RS512/PS256/PS384/PS512/ES384; `none` and `HS*` are rejected |
| `PHARNESS_CAMPUS_OIDC_CLOCK_SKEW_SECONDS` | no | `60`; 0-300 |
| `PHARNESS_CAMPUS_SESSION_ABSOLUTE_TTL_SECONDS` | no | `28800` (8 h); 900-86400 |
| `PHARNESS_CAMPUS_SESSION_IDLE_TTL_SECONDS` | no | `1800` (30 min); 300 up to the absolute TTL |
| `PHARNESS_CAMPUS_BOOTSTRAP_ADMINS` | no | Comma-separated `issuer\|subject` pairs, at most 32; the issuer must equal the configured issuer |
| `PHARNESS_CAMPUS_POST_LOGIN_PATH` | no | `/`; same-origin absolute path |
| `PHARNESS_CAMPUS_DEV_ALLOW_INSECURE_LOOPBACK_HTTP` | no | `false`; development only, see below |

Every URL must use https and must not contain credentials or a fragment.
Plain http is accepted only when the development flag is `true`, and then
only for `localhost`, `127.0.0.0/8` or `::1`. The IdP HTTP client follows no
redirects, enforces https unless that flag is set, uses rustls, and bounds
time (10 s) and response size.

Bootstrap administrators are synchronized at every normal-mode startup, so
removing a pair revokes it on the next start. Course memberships are managed
through the API below.

### Swapping the identity provider

To move from the test IdP to the institution's issuer, change only the
`PHARNESS_CAMPUS_OIDC_*` values, the secret file, and the
`PHARNESS_CAMPUS_BOOTSTRAP_ADMINS` issuer. Register the redirect URI with the
institution as a confidential client that supports PKCE S256. Sessions,
memberships and admin grants are bound to the issuer. Existing sessions from
the previous issuer stop working (401). Memberships recorded under the old
issuer do not carry over and must be re-entered with the new issuer's
subjects.

## Login flow

1. `GET /api/campus/auth/login` creates a one-time login attempt that expires
   after 10 minutes. The server stores the SHA-256 of the state, the nonce,
   the PKCE verifier and the SHA-256 of a browser-binding value. The response
   sets `__Host-pharness-campus-login` (Max-Age 600) and redirects (302) to
   the authorization endpoint with `response_type=code`, `state`, `nonce` and
   `code_challenge_method=S256`. At most 10,000 attempts can be outstanding;
   beyond that, login returns 503.
2. `GET /api/campus/auth/callback?code&state` consumes the attempt
   atomically before any other check, so a replayed or failed state can
   never be reused. It then requires that the attempt is unexpired and that
   the login cookie matches the stored binding. If the IdP sends an `iss`
   parameter (RFC 9207), it must match the configured issuer. The callback
   exchanges the code using the PKCE verifier and validates the id_token.
3. The id_token must pass every check below. Otherwise the callback returns
   401 and sets no session cookie:
   - The `kid` header is present and the `alg` is in the allowlist.
   - The `alg` matches the JWK's key type and curve, and the JWK's `alg` if
     the JWK declares one. Symmetric (`oct`) and `use=enc` keys are never
     loaded.
   - `iss` is exact.
   - `aud` contains the client id. If `aud` lists several audiences, `azp`
     must equal the client id. If `azp` is present, it must always equal the
     client id.
   - `exp`, `nbf` and `iat` are checked with the configured skew.
   - The `nonce` matches the stored nonce.
   - `sub` is 1-255 characters with no control characters.
4. On success the server upserts the identity, revokes any session cookie
   the browser presented, creates a new session, clears the login cookie and
   answers 303 to `PHARNESS_CAMPUS_PUBLIC_ORIGIN` +
   `PHARNESS_CAMPUS_POST_LOGIN_PATH`.

The JWKS is cached. A token with an unknown `kid` triggers a refetch, at
most once every 60 s. A successful refetch replaces the whole key set, so a
key the IdP removed stops validating. The cache is also refetched on use
after one hour. If that refetch fails, nothing validates until the JWKS is
reachable again.

## Sessions, cookies, CSRF and logout

- The session id is 256 random bits, sent only in the cookie
  `__Host-pharness-campus=<id>; Path=/; Secure; HttpOnly; SameSite=Lax;
  Max-Age=<absolute TTL>`, with no `Domain`. The server stores only its
  SHA-256, the absolute expiry, a rolling idle expiry (capped at the absolute
  expiry) and `revoked_at`.
- Unknown, idle-expired, absolutely expired, revoked, other-tenant and
  other-issuer sessions get 401, and the response clears the cookie.
  Duplicate session cookies are rejected.
- Roles and administrator status are read from storage on every request, so
  a change applies to the next request.
- CSRF applies to every request that is not GET, HEAD or OPTIONS. Such a
  request must send `X-Campus-CSRF: <csrf_token>`. If an `Origin` header is
  present, it must equal the public origin. A `Sec-Fetch-Site: cross-site`
  header is rejected. A missing or mismatched token or origin gets 403.
  `csrf_token` comes from `GET /api/campus/me`. It is derived from the
  session id with domain separation, does not reveal the session id, stays
  stable for the life of the session, and is stored only as a hash.
- `POST /api/campus/auth/logout` (CSRF required) revokes the session on the
  server and clears the cookie. Replaying the old cookie afterwards gets
  401.
- Every Campus response sends `Cache-Control: no-store`. Request spans log
  only the method and path, never the callback query.

## API

| Method and path | Authorization |
| --- | --- |
| `GET /api/campus/auth/login` | Public; creates a login attempt |
| `GET /api/campus/auth/callback` | State, browser binding and id_token checks |
| `POST /api/campus/auth/logout` | Session and CSRF |
| `GET /api/campus/me` | Session. Returns tenant, identity, admin flag, memberships, `csrf_token` and session expiries |
| `GET /api/campus/courses/{course_id}` | ViewCourse: a student or instructor of the course, or a tenant admin |
| `GET /api/campus/courses/{course_id}/members` | ViewRoster: an instructor of the course or a tenant admin |
| `PUT /api/campus/courses/{course_id}/members` | ManageMembers: tenant admin only, plus CSRF. Body `{"issuer","subject","role":"student"\|"instructor"}` |
| `DELETE /api/campus/courses/{course_id}/members` | ManageMembers: tenant admin only, plus CSRF. Body `{"issuer","subject"}` |

The authorization matrix is in `campus/authz.rs`. It uses an exhaustive
match and denies by default. Administration is tenant-scoped, and a
cross-tenant decision is always denied. A denied course request gets 403
whether or not the course exists. Request bodies reject unknown fields, so a
body that names a tenant, actor or email is rejected (4xx) rather than
trusted. The member issuer must be the configured issuer. Course ids are
1-128 characters of `[A-Za-z0-9._-]`.

## Residual risks and open questions

- The institution's claims and groups are not mapped. Roles come only from
  PHarness memberships and bootstrap admins. Group or role claims, and
  `email_verified`, are ignored.
- `SameSite=Lax` is needed for the IdP's redirect back to the callback. CSRF
  protection for state changes relies on the CSRF header and the origin
  checks.
- An XSS on the Campus origin could read `csrf_token` from `/me` and act as
  the user within the session. The `HttpOnly` cookie still cannot be read.
- There is no per-client rate limit on `/auth/login`. The 10,000-attempt cap
  bounds storage but can deny logins during a flood.
- Draining mode does not block Campus membership writes. Read-only mode
  disables Campus entirely.
- There is no IdP-initiated or back-channel logout. Logging out of the IdP
  does not end a PHarness session before its idle or absolute expiry.
- Nonces and PKCE verifiers are kept in plaintext for up to 10 minutes. They
  are single-use and are included in database backups.
- A full RS256 round trip is tested with a 2048-bit key that the system
  `openssl` generates when the test runs. The test is skipped if `openssl` is
  missing. ES256 round trips use `ring`-generated keys. No private key
  material is committed.
