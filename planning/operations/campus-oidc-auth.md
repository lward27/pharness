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
  that names the variable. The client secret and the login-cookie key are
  never printed.
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
| `PHARNESS_CAMPUS_LOGIN_COOKIE_KEY_FILE` | yes | Path to a file holding exactly 32 random bytes, raw or base64 (standard or URL-safe, padding optional). It seals the login cookie; mount it from a Secret. See below |
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

Generate the login-cookie key with `head -c 32 /dev/urandom > key` or
`openssl rand -base64 32 > key`. A file that does not decode to exactly 32
bytes, 32 printable characters (a password rather than a key), or 32
identical bytes is rejected at startup. All replicas must share the same
key. Rotating it invalidates only logins that are in progress (at most 10
minutes old); sessions are unaffected.

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

1. `GET /api/campus/auth/login` is stateless and writes nothing to the
   database. It generates a random state, nonce and PKCE verifier (256 bits
   each) and seals them, with the issue time and a 10-minute expiry, into
   the `__Host-pharness-campus-login` cookie (Max-Age 600). It then
   redirects (302) to the authorization endpoint with `response_type=code`,
   `state`, `nonce` and `code_challenge_method=S256`.
2. `GET /api/campus/auth/callback?code&state` opens the sealed cookie. A
   missing, duplicated, tampered or foreign cookie gets 400. The `state`
   parameter must equal the sealed state (constant-time comparison), and
   the cookie must be unexpired. Only then is the SHA-256 of the state
   inserted into `campus_consumed_login_states`. Its primary key makes this
   single-use: a duplicate is a replay and gets 400. Each record is kept
   until the cookie's own expiry; expired records are pruned on every
   insert. The state is consumed before the code is redeemed, so a failed
   exchange or id_token check still burns it. A request without the right
   cookie cannot burn a state, so it cannot cancel someone else's login. If
   the IdP sends an `iss` parameter (RFC 9207), it must match the
   configured issuer. The callback then exchanges the code using the PKCE
   verifier and validates the id_token.
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
   - The `nonce` matches the sealed nonce.
   - `sub` is 1-255 characters with no control characters.
4. On success the server upserts the identity, revokes any session cookie
   the browser presented, creates a new session, clears the login cookie and
   answers 303 to `PHARNESS_CAMPUS_PUBLIC_ORIGIN` +
   `PHARNESS_CAMPUS_POST_LOGIN_PATH`.

### Sealed login cookie

The cookie value is base64url (no padding) of
`version (1 byte) || nonce (12) || ciphertext || tag (16)`, encrypted with
ChaCha20-Poly1305 (`ring::aead`) under the login-cookie key, with a fresh
random nonce for each cookie. The associated data is a fixed
domain-separation label, the version byte, the tenant id and the issuer. A
cookie made under another key, version, tenant or issuer, or with any bit
changed, does not open. Holding the cookie is what binds the login to the
browser that started it: `__Host-` stops sibling domains from setting it,
and the state inside it must match the callback's `state`. The nonce and
PKCE verifier are never written to the database.

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
  Duplicate session cookies are rejected. A `Cookie` pair containing
  non-ASCII bytes, such as one set by a sibling domain, is ignored rather
  than making the whole header unreadable.
- Roles and administrator status are read from storage on every request, so
  a change applies to the next request.
- CSRF applies to every request that is not GET, HEAD or OPTIONS. Such a
  request must send `X-Campus-CSRF: <csrf_token>`. If an `Origin` header is
  present, it must equal the public origin. A `Sec-Fetch-Site: cross-site`
  header is rejected. A missing or mismatched token or origin gets 403.
  These checks run before the session's idle window is extended, so a
  rejected request does not keep a session alive.
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
| `GET /api/campus/auth/login` | Public; writes nothing and sets only the sealed login cookie |
| `GET /api/campus/auth/callback` | Sealed login cookie, single-use state and id_token checks |
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
- `/auth/login` writes nothing and has no global cap, so flooding it
  cannot block other users' logins. The callback still writes one small row
  per state that arrives with a valid sealed cookie. Anyone can get such a
  cookie from `/auth/login`, so rate-limiting `/api/campus/auth/callback`
  (and `/auth/login`) per client at the ingress is still recommended. The
  rows expire after 10 minutes.
- The login-cookie key is a long-lived secret. Anyone holding it can forge
  login cookies for chosen states. That alone does not create a session,
  because the IdP still has to issue a code and an id_token with the
  matching nonce, but the key should be protected like the client secret.
- Draining mode does not block Campus membership writes. Read-only mode
  disables Campus entirely.
- There is no IdP-initiated or back-channel logout. Logging out of the IdP
  does not end a PHarness session before its idle or absolute expiry.
- A full RS256 round trip is tested with a 2048-bit key that the `rsa`
  crate (a dev-dependency only) generates when the test runs, so the test
  always runs. ES256 round trips use `ring`-generated keys. No private key
  material is committed.
