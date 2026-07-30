# ADR 005: Policy delivery via MDM-native formats, no application-layer signing

- Status: accepted
- Date: 2026-07-28
- Supersedes: signature placeholder in `endpoint/mdm/src/lib.rs`

## Context

The endpoint daemon enforces a device-level policy ceiling delivered by the
organization's MDM. The current implementation (`endpoint/mdm/src/lib.rs`)
defines a custom JSON wrapper format (`fabric-mdm/v1`) with a `signature`
field described as "placeholder for future code-signing verification."

Two questions needed resolution:

1. **Should Fabric sign policy packs?** (ed25519, customer-managed keys)
2. **Is MDM the right delivery mechanism**, given that agent policy is
   user-bound and first-login delivery is simpler?

## Decision

### MDM is the delivery mechanism

Policy packs are delivered via the organization's existing MDM (Jamf,
Intune, or equivalent), not via first-login API fetch.

Rationale:

- **The daemon is a system service, not a user app.** It runs under
  launchd/systemd, boots at device startup, and operates independently of
  any user session. Policy must be available at boot, not at login.
- **Offline-first requires pre-positioned policy.** If policy arrives at
  login, an offline device has no policy → daemon either fails open
  (unsafe) or fails closed (useless). MDM policy is on disk before the
  user ever authenticates.
- **Survivability.** MDM profiles persist across user sign-out, account
  changes, and profile resets. A login-fetched cache does not.
- **Zero new tooling.** IT admins already push WiFi, VPN, restrictions,
  and certificates via MDM. Agent policy is another payload in the same
  channel. No new firewall rules, no new API to monitor, no new trust
  boundary.

### User-bound policy is server-side, not MDM

The dual-policy model separates concerns:

| Layer | Bound to | Delivered by | Survives offline |
|---|---|---|---|
| Device ceiling (what this device *can* do) | Device | MDM | Yes — on disk at boot |
| User policy (what this user's agent *may* do) | User | Server (fetched when online) | Opportunistic cache, deny-wins on stale |

MDM delivers the device capability floor. User-specific policy
("this user's agent cannot access financial tools") is server-side
additive policy, fetched when online, cached opportunistically, and
deny-wins if the cache is stale.

### No application-layer signing

Fabric does not sign policy packs. The `signature` field in the current
wrapper format is removed.

Rationale:

- **MDM is the trust anchor.** Every other device setting (WiFi, VPN,
  restrictions, certificates) arrives via MDM with MDM's integrity
  guarantees. Adding ed25519 on top of one payload type builds a parallel
  trust kingdom — the same anti-pattern as mTLS alongside ZT.
- **Post-delivery integrity is the customer's problem.** The file sits on
  disk protected by FileVault, BitLocker, SELinux, SIP, and endpoint EDR.
  If an attacker can bypass all of those AND tamper with an MDM-managed
  file, they can also patch the daemon binary itself. A signature check
  doesn't save you.
- **No key management burden.** No customer KMS integration, no key
  rotation, no grace periods, no `FABRIC_PACK_PUBLIC_KEY` config. One
  fewer thing to break, document, and support.

### MDM-native formats

Fabric ships policy packs in the MDM platform's native format:

| Platform | Format | Delivery |
|---|---|---|
| Jamf (macOS) | plist (Configuration Profile) | Jamf Pro policy |
| Intune (Windows) | OMA-URI / XML CSP | Intune configuration profile |
| Generic / Linux | JSON (current format, minus signature) | Any file-delivery MDM |

The daemon parses whichever format the platform delivers. The
`fabric-mdm/v1` JSON wrapper remains as the generic/Linux format. The
`signature` field is removed from the schema.

### What Fabric ships

- **Pack generators**: `fabric-pack generate --format jamf|intune|generic`
  → platform-native MDM format
- **Pack parser** in `endpoint/mdm/`: reads platform-native format, maps
  to internal `EndpointPolicy` struct
- **Schema validation**: reject malformed packs (structural integrity,
  not origin integrity)
- **Docs**: "deliver via your MDM, secure your disk with your existing
  tools"

### What Fabric does NOT ship

- Signing keys, verification logic, key rotation, grace periods
- Any crypto for pack integrity
- Disk protection (FileVault/BitLocker/SELinux)
- MDM server software

## Consequences

- `endpoint/mdm/src/lib.rs`: remove `signature` field from `PolicyPack`
  struct and docs. Remove "future code-signing" language.
- `enterprise/mdm/`: implement Jamf plist and Intune OMA-URI generators.
- `endpoint/mdm/`: add plist parser (macOS) and OMA-URI/XML parser
  (Windows) alongside the existing JSON parser.
- The `fabric-mdm/v1` JSON format remains for generic/Linux deployments
  but loses the `signature` field.
- No `FABRIC_PACK_PUBLIC_KEY` config. No `ed25519-dalek` dependency for
  pack verification.

## References

- ADR 001 (monorepo, Rust, licensing)
- ADR 002 (conflict resolution, offline-first, dual policy)
- ADR 003 (control plane auth — same "don't build a parallel trust
  kingdom" principle)
- Current implementation: `endpoint/mdm/src/lib.rs` (193 lines)
