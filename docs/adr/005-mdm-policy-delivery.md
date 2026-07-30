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

### MDM key reference (customer documentation)

These are the exact keys IT admins configure in their MDM. The daemon
reads them from the platform-native location on disk.

#### macOS (Jamf Configuration Profile)

Payload domain: `tech.fabric.agent.policy`

| Key | Type | Required | Description |
|---|---|---|---|
| `PolicyID` | String | Yes | Unique policy identifier |
| `Version` | String | Yes | Policy version (semver) |
| `OrgID` | String | Yes | Organization identifier |
| `KillSwitch` | Boolean | No | Emergency stop. `true` = daemon halts all agent activity. Default: `false` |
| `MaxRetentionHours` | Integer | No | Max hours context entries are retained. `0` = unlimited. Default: `0` |
| `DataRules` | Array of Dict | No | Per-data-class rules (see below) |
| `ToolRules` | Array of Dict | No | Per-tool access rules (see below) |
| `ModelRules` | Array of Dict | No | Per-model access rules (see below) |
| `CuaEnabled` | Boolean | No | Allow computer-use actuator. Default: `false` |
| `CuaMaxScreenArea` | Real | No | Max fraction of screen CUA may interact with (0.0–1.0). Default: `1.0` |
| `CuaRequireConfirmation` | Boolean | No | Require user confirmation before CUA actions. Default: `true` |
| `CuaBlockedApps` | Array of String | No | Bundle IDs CUA must never touch (e.g. `com.apple.Terminal`) |
| `DlpPatterns` | Array of Dict | No | DLP regex patterns (see below) |
| `SafetyEnabled` | Boolean | No | Enable content safety classification. Default: `true` |
| `SafetyFailMode` | String | No | `open` or `closed` (behavior when safety model unreachable). Default: `closed` |
| `SafetyRules` | Array of Dict | No | Per-category safety rules (see below) |

**DataRules dict keys:**

| Key | Type | Description |
|---|---|---|
| `DataClass` | String | Data classification label (e.g. `pii`, `financial`, `public`) |
| `MayLeaveDevice` | Boolean | Whether this data class may be sent to server-side inference |
| `RequiresRedaction` | Boolean | Whether data must be redacted before leaving device |
| `AllowedDestinations` | Array of String | Explicit allowlist of destination hostnames (empty = any if `MayLeaveDevice` is true) |

**ToolRules dict keys:**

| Key | Type | Description |
|---|---|---|
| `ToolPattern` | String | Glob pattern matching tool names (e.g. `file.*`, `shell.exec`) |
| `Action` | String | `allow`, `deny`, or `confirm` (require user confirmation) |
| `MaxCallsPerSession` | Integer | Rate limit per session. `0` = unlimited |

**ModelRules dict keys:**

| Key | Type | Description |
|---|---|---|
| `ModelPattern` | String | Glob pattern matching model identifiers (e.g. `nvidia/*`, `local/*`) |
| `Action` | String | `allow` or `deny` |
| `MaxTokensPerCall` | Integer | Per-call token cap. `0` = unlimited |

**DlpPatterns dict keys:**

| Key | Type | Description |
|---|---|---|
| `Name` | String | Pattern identifier (e.g. `us-ssn`) |
| `Regex` | String | Regular expression to match |
| `Action` | String | `redact`, `block`, or `warn` |

**SafetyRules dict keys:**

| Key | Type | Description |
|---|---|---|
| `Category` | String | Safety category (e.g. `violence`, `sexual`, `pii`, `injection`) |
| `Action` | String | `block`, `warn`, or `allow` |

#### Windows (Intune OMA-URI)

CSP path: `./Vendor/MSFT/Policy/Config/Fabric~Agent~Policy/`

| OMA-URI suffix | Type | Required | Maps to |
|---|---|---|---|
| `PolicyID` | String | Yes | `policy_id` |
| `Version` | String | Yes | `version` |
| `OrgID` | String | Yes | `org_id` |
| `KillSwitch` | Integer (0/1) | No | `kill_switch` |
| `MaxRetentionHours` | Integer | No | `max_retention_hours` |
| `CuaEnabled` | Integer (0/1) | No | `cua.enabled` |
| `CuaMaxScreenArea` | String (decimal) | No | `cua.max_screen_area` |
| `CuaRequireConfirmation` | Integer (0/1) | No | `cua.require_confirmation` |
| `CuaBlockedApps` | String (semicolon-delimited) | No | `cua.blocked_apps` |
| `SafetyEnabled` | Integer (0/1) | No | `safety.enabled` |
| `SafetyFailMode` | String | No | `safety.fail_mode` |
| `DataRules` | String (JSON array) | No | `data_rules` |
| `ToolRules` | String (JSON array) | No | `tool_rules` |
| `ModelRules` | String (JSON array) | No | `model_rules` |
| `DlpPatterns` | String (JSON array) | No | `dlp_patterns` |
| `SafetyRules` | String (JSON array) | No | `safety.rules` |

Note: Intune OMA-URI does not support native nested arrays. Complex
rules (DataRules, ToolRules, etc.) are delivered as JSON-encoded strings
within a single OMA-URI value. The daemon parses the JSON string into
the corresponding proto structs.

#### Generic / Linux (JSON file)

File path: `/etc/fabric/policy.json` (or `FABRIC_POLICY_PATH` env)

Format: the existing `fabric-mdm/v1` JSON wrapper (minus `signature`).
Field names on the wire are camelCase per the protobuf JSON mapping
(pbjson); snake_case is still accepted on read for compatibility:

```json
{
  "format": "fabric-mdm/v1",
  "policy": {
    "policyId": "org-acme-2026-07",
    "version": "1.2.0",
    "orgId": "acme-corp",
    "killSwitch": false,
    "maxRetentionHours": 720,
    "dataRules": [
      {
        "dataClass": "pii",
        "mayLeaveDevice": false,
        "requiresRedaction": true,
        "allowedDestinations": []
      }
    ],
    "toolRules": [
      {
        "toolPattern": "shell.*",
        "action": "confirm",
        "maxCallsPerSession": 50
      }
    ],
    "modelRules": [
      {
        "modelPattern": "local/*",
        "action": "allow",
        "maxTokensPerCall": 0
      }
    ],
    "cua": {
      "enabled": true,
      "maxScreenArea": 0.8,
      "requireConfirmation": true,
      "blockedApps": ["com.apple.Terminal"]
    },
    "dlpPatterns": [
      {
        "name": "us-ssn",
        "regex": "\\b\\d{3}-\\d{2}-\\d{4}\\b",
        "action": "redact"
      }
    ],
    "safety": {
      "enabled": true,
      "failMode": "closed",
      "rules": [
        { "category": "violence", "action": "block" },
        { "category": "injection", "action": "block" }
      ]
    }
  }
}
```

#### Key naming convention

- macOS plist: PascalCase (Apple convention)
- Intune OMA-URI: PascalCase (Microsoft convention)
- JSON: camelCase (protobuf JSON mapping; snake_case accepted on read)

The daemon normalizes all three to the internal `EndpointPolicy` proto
struct. Admins configure in their platform's native convention; no
cross-platform key translation required.

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
