# ADR 003: Control plane authentication — standard OIDC, server as sole relying party

- Status: accepted
- Date: 2026-07-28

## Context

The control plane (`fabric-control`) currently binds `0.0.0.0:47800` with zero
authentication. Any network-reachable caller can steal leases, inject context,
or squat sessions (review finding S1/S2, CRITICAL). The `holder_id` field is
caller-supplied — a string with no proof of possession.

The fabric is offline-first: endpoints go dark for hours or days. The auth
design must not break offline operation. The fabric is also standards-based and
IdP-agnostic: customers bring their own identity authority (Entra, Okta,
Authentik, or anything else that speaks OIDC). We do not operate an IdP and
must not create a dependency on one at request time.

### Constraints

- **Offline-first**: an endpoint that loses connectivity keeps working. Auth
  must not gate local operation.
- **Standards-based**: only common, universally-implemented RFCs. No
  proprietary flows, no IdP-specific extensions on the critical path.
- **IdP-agnostic**: the fabric validates tokens; it does not mint them. How
  the endpoint acquires a credential is the customer's business.
- **CA-compatible**: enterprises run Conditional Access (device compliance,
  location, risk). The auth design must not bypass or nerf CA.
- **No IdP in the request path**: validation is local (cached signing keys).
  The IdP is only touched at token issuance, never at API call time.
- **ZDR**: no third-party auth service as a runtime dependency.

### What was rejected and why

| Option | Rejection reason |
|---|---|
| mTLS | Creates a parallel trust kingdom alongside the admin's ZT stack. Cert rotation for field devices is an ops burden. Admin must debug two trust chains. |
| Device Code Flow (RFC 8628) as primary | Enterprises disable it because it nerfs Conditional Access — the requesting device is opaque, CA can't evaluate compliance/location/risk. Token lands on a box CA never saw. |
| RFC 8693 Token Exchange | Not portable: Entra has proprietary OBO, Okta has nothing, Authentik just merged it (Jul 2026, not stable). |
| RFC 9449 DPoP | Absent in Entra and Okta. Authentik only binds ID tokens, not access tokens. Aspirational, not v1. |
| SAML | Browser-SSO protocol. No M2M flow, no offline bearer model, no device enrollment, XML-DSig in Rust. Wrong protocol for API auth. |
| Fabric-operated IdP | Violates ZDR and BYO-IdP. We are a control plane, not an identity provider. |
| Opaque device tokens (Fabric-minted) | Reintroduces a Fabric-owned credential store. The customer's IdP already does this better. |

## Decision

### The server is the sole OIDC relying party

`fabric-control` validates RS256-signed JWTs against the customer's IdP.
It is the only component that speaks OIDC. Endpoints present tokens; they
do not validate, mint, or refresh them against the IdP directly (the OS
token broker or MDM handles that).

### Validation is local — no IdP in the request path

The server caches the IdP's JWKS (RFC 7517) discovered via OIDC Discovery
(RFC 8414 / OpenID Connect Discovery 1.0). All token validation is pure
local cryptography. The IdP is never contacted during an API call.

JWKS cache implementation (per auth0/node-jwks-rsa, the industry-standard
pattern):

- LRU cache keyed by `kid`, 5 entries max
- Freshness TTL: 10 minutes (configurable via `FABRIC_JWKS_TTL_SECS`)
- Unknown-`kid` miss: fetch JWKS once, rate-limited to 10 fetches/minute
- Stale-cache fallback: if the JWKS endpoint is unreachable and the cache
  has expired, serve last-known-good keys for an additional window
  (default 1 hour, configurable via `FABRIC_JWKS_STALE_SECS`), with a
  `warn!` log for alerting
- Bootstrap: pin issuer keys at startup so a cold start with no IdP
  connectivity still validates

### Token acquisition is the endpoint's business

Fabric defines a contract: *present a valid RS256 JWT with correct `aud`
and `iss`, validated against JWKS.* How the endpoint minted it is
out of scope.

Expected acquisition paths (all standard OIDC, all CA-compatible):

1. **OS token broker** (primary for managed devices): the daemon calls the
   platform broker — WAM on Windows, Platform SSO / `ASAuthorization` on
   macOS, broker on Linux/Intune. The broker performs auth with full CA +
   device compliance evaluation and hands the daemon a scoped token. Zero
   secrets in the Fabric binary. CA fully intact.

2. **Client credentials + X.509 cert** (headless/server endpoints): app
   registration / service principal with a certificate (not a shared
   secret). CA can still apply device/location conditions to the service
   principal. This is the Azure managed-identity / workload-identity shape.

3. **Authorization code + PKCE** (interactive enrollment): RFC 6749 §4.1
   with RFC 7636. Human authorizes the endpoint once via browser. CA fully
   evaluated. Suitable for developer workstations and BYO devices.

4. **Device Code Flow** (explicit opt-in fallback, off by default):
   RFC 8628. Only for genuinely unmanaged/BYO devices where the admin
   chose to allow it. Documented as "this bypasses device CA." Gated
   behind `FABRIC_ALLOW_DCF=true`.

### Identity derivation

`holder_id` is derived from the validated token's `sub` claim. It is never
taken from the request body. The request-body `holder_id` fields in
`lease.proto` are deprecated and ignored when auth is enabled.

A server-side **device registry** maps `sub → endpoint` for the
association between an identity and a physical device. On first
authenticated request from an unknown `sub`, the server creates an
endpoint record.

### Offline behavior

Auth interactions only happen while online. The endpoint's offline
operation is unaffected:

| Scenario | Behavior |
|---|---|
| Online, token valid | Normal operation. Server validates JWT locally. |
| Online, token expired | Endpoint refreshes via refresh token (RFC 6749 §6) or broker. |
| Offline, token valid | Endpoint works locally. Doesn't need the control plane. |
| Offline, token expired | Endpoint works locally. Cannot reach control plane until reconnect. This is correct — offline-first means the endpoint is sovereign. |
| Reconnect after token expiry | Endpoint refreshes (broker/refresh token), then reconciles. |
| Offline > refresh token lifetime | Full re-enrollment required. Correct — a device dark for months should re-authenticate. |

The fabric does not attempt to solve "renew while offline." No standard
does. GitHub CLI, Azure CLI, Kubernetes, and SPIFFE all have the same
property: credentials are valid until they expire, and renewal requires
connectivity.

### RFCs used (all universally implemented)

| RFC | Role |
|---|---|
| RFC 6749 §4.1 | Authorization code grant (interactive enrollment) |
| RFC 6749 §4.4 | Client credentials grant (headless M2M) |
| RFC 6749 §6 | Refresh tokens (renewal when online) |
| RFC 7517 | JWKS (local key cache for validation) |
| RFC 7519 | JWT (token format) |
| RFC 7636 | PKCE (mandatory for auth code flow) |
| RFC 8414 | AS metadata / OIDC Discovery |
| RFC 8628 | Device code flow (opt-in fallback only) |
| RFC 9700 | OAuth 2.0 Security BCP (refresh rotation, PKCE, no implicit) |

Not used on the critical path: RFC 8693 (token exchange), RFC 9449 (DPoP),
RFC 8705 (mTLS-bound tokens). These may be added as optional hardening
adapters in the future, feature-detected per IdP.

### Configuration

```toml
# fabric-control config
[auth]
enabled = true                    # false = dev mode, holder_id = "dev"
issuer = "https://login.example.com"  # OIDC issuer URL
audience = "fabric-control"       # expected aud claim
jwks_ttl_secs = 600              # JWKS cache freshness
jwks_stale_secs = 3600           # stale-cache fallback window
allow_dcf = false                # RFC 8628 opt-in
admin_header = ""                # trusted proxy header for admin routes (e.g. Cf-Access-Authenticated-User-Email)
```

Dev mode (`auth.enabled = false`): skip all validation, `holder_id = "dev"`,
log a loud warning at startup. Never bind `0.0.0.0` in dev mode.

### Authentik caveat

Authentik defaults to HS256 (symmetric, client-secret-signed) JWTs unless
a signing key is configured. The fabric requires RS256 asymmetric signing
for JWKS validation. Document this in the Authentik setup guide: configure
a signing key on the OAuth2 provider. Entra and Okta are RS256 by default.

### Future hardening (not v1)

- **DPoP** (RFC 9449): when IdPs support it broadly, bind tokens to
  device-held keys. Stolen token becomes useless without the private key.
- **Token exchange** (RFC 8693): for delegation scenarios (endpoint acts
  on behalf of a user). Per-IdP adapter (OBO for Entra, RFC 8693 for
  Authentik, skip for Okta).
- **mTLS-bound tokens** (RFC 8705): sender-constrained access tokens.
  Requires client certs, which we rejected as the primary auth but could
  layer on top of OIDC for high-security deployments.

## Consequences

- `fabric-control` gains an axum middleware layer: JWT validation →
  identity extraction → request extension injection.
- `holder_id` in `lease.proto` request messages becomes deprecated.
  Server derives it from the token. Proto fields remain for wire
  compatibility but are ignored when auth is enabled.
- A `devices` table is added to the control plane's SQLite store for the
  `sub → endpoint` registry.
- The endpoint daemon gains a token-provider trait with adapter
  implementations (broker, client-credentials, static-file for dev).
- All existing tests that pass `holder_id` in request bodies continue to
  work (dev mode). New tests validate the auth middleware with
  locally-signed test JWTs.
- The admin API (policy CRUD, audit) can sit behind a ZT proxy and read
  identity from a trusted header, separate from the device auth path.

## References

- Review findings S1 (unauthenticated control plane), S2 (non-atomic
  preempt + caller-supplied holder_id), S3 (plaintext lease traffic)
- auth0/node-jwks-rsa: industry-standard JWKS caching pattern
- RFC 9700 (BCP 240): OAuth 2.0 Security Best Current Practice
- Kubernetes service-account JWT validation: production-scale proof that
  locally-validated, JWKS-backed, audience-bound JWTs work
- GitHub CLI, Azure CLI/MSAL, kubelogin: device/broker enrollment patterns
