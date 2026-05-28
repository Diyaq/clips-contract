# Security Policy — ClipCash NFT Contract

## Supported Versions

| Version | Status            |
|---------|-------------------|
| 1.x     | Actively supported |

## Reporting a Vulnerability

**Do not open a public GitHub issue for security vulnerabilities.**

Email **security@clipcash.app** with:
- Description and impact
- Steps to reproduce
- Contract address (testnet or mainnet) if applicable
- Suggested mitigation (optional)

You will receive an acknowledgement within 48 hours. Confirmed vulnerabilities
are patched within 7 days for critical issues and 30 days for others.

---

## Security Architecture

### Access Control

| Operation | Authorized caller |
|-----------|------------------|
| `init` | One-time; any address (idempotent guard via `panic!`) |
| Admin functions (pause, set_signer, set_platform_fee, blacklist, freeze, upgrade) | Stored `Admin` address only via `require_admin` |
| `mint` | Token recipient (`to.require_auth()`) + valid backend signature |
| `mint_with_signature` | Token recipient (`to.require_auth()`) + strictly-increasing nonce + backend signature |
| `transfer` | Token owner (`from.require_auth()`); buyer also signs when `sale_price > 0` |
| `burn` | Token owner only |
| `approve` | Token owner or operator approved for all |
| `update_royalty_recipient` | Current primary royalty recipient only |

### Signature Scheme

Mint operations require an Ed25519 signature from the registered backend signer over:

```
message = SHA-256(
    clip_id_le4_bytes
    || SHA-256(XDR(owner_address))
    || SHA-256(UTF-8(metadata_uri))
)
```

`mint_with_signature` adds a domain separator and nonce:

```
message = SHA-256(
    "mint_with_signature"
    || clip_id_le4_bytes
    || SHA-256(XDR(owner_address))
    || SHA-256(UTF-8(metadata_uri))
    || nonce_le8_bytes
)
```

The `"mint_with_signature"` prefix prevents cross-function replay. Nonces are
strictly increasing per recipient; any nonce ≤ the stored `LastMintNonce` is
rejected (`Error::InvalidSignature`).

### Reentrancy Protection

All functions that invoke external SEP-0041 token contracts (`pay_royalty`,
`claim_royalties`, `transfer` with `sale_price > 0`, `withdraw_asset`) acquire
an instance-storage reentrancy lock before the external call and release it
after. Re-entrant calls return `Error::Reentrancy`.

State changes follow the **check-effects-interactions** pattern: all storage
writes are committed before any external token transfer call.

### Integer Overflow

Royalty amounts are calculated via `safe_math::safe_royalty_amount`:

```
royalty = sale_price × basis_points / 10_000
```

The multiplication is guarded with a checked-multiply check. Inputs where
`sale_price > i128::MAX / 10_000` return `Error::RoyaltyOverflow` rather than
wrapping. All counters use `saturating_add` / `saturating_sub`.

### Pause Mechanism

The contract supports two pause modes:

1. **Full pause** (`pause(admin, reason)`) — blocks `mint` and `transfer`.
   Enforced by a 24-hour timelock before the pause takes effect.
   Stores an optional human-readable reason queryable via `pause_reason()`.
2. **Minting-only pause** (`pause_minting`) — blocks new mints while allowing
   existing token transfers to proceed.

The circuit breaker auto-pauses the contract when mint activity exceeds the
configured threshold within a time window, emitting `CircuitBreakerTriggeredEvent`.

### Asset Withdrawal

Admin fund withdrawals use a **48-hour timelock**:
1. `request_withdraw_asset` stores the request with `unlock_time = now + 172_800`.
2. `withdraw_asset` fails with `Error::WithdrawalStillLocked` until the timelock expires.

### URL Validation

All optional `image` and `animation_url` fields are validated on-chain before
any state changes. Only `https://` and `ipfs://` schemes are accepted;
empty strings and other schemes return `Error::UnsupportedProtocol` or
`Error::MalformedUrl`.

### Metadata Immutability

Token owners may permanently lock their token's metadata with `lock_metadata`.
Once locked (`is_locked = true`), metadata updates return `Error::MetadataLocked`.
The lock is irreversible.

### Soulbound Tokens

Tokens minted with `is_soulbound = true` cannot be transferred (returns
`Error::SoulboundTransferBlocked`). Admin soulbound recovery requires a
separate backend-signed payload with a `"recover"` domain separator.

### Deduplication

Each `clip_id` maps to exactly one token via `DataKey::ClipIdMinted`. Attempts
to mint the same `clip_id` a second time return `Error::ClipAlreadyMinted`.
Blacklisted clips are permanently blocked from minting via `DataKey::BlacklistedClip`.

### Storage TTL Hardening

All persistent storage entries are extended on every read via
`extend_ttl(PERSISTENT_BUMP_THRESHOLD, PERSISTENT_BUMP_AMOUNT)` to minimize
archival risk for hot token data.

---

## Known Limitations

- **XLM royalties**: When `asset_address` is `None` (XLM), the `pay_royalty`
  function returns `Error::InvalidRecipient`. Marketplaces must handle native
  XLM royalty transfers directly, outside the contract.
- **Enumeration gas**: `index_remove_owner` and `index_remove_global` iterate
  linearly over the owner's index; cost scales with the number of tokens held.
  Production integrations should prefer paginated `get_user_tokens`.
- **Upgrade authority**: Contract upgrades require the admin key. If the admin
  key is lost, the contract cannot be upgraded. Consider a multi-sig admin.

---

## Audit History

| Date       | Scope                         | Auditor        | Report  |
|------------|-------------------------------|----------------|---------|
| 2026-05-28 | Internal pre-mainnet review   | ClipCash team  | (this document) |

No external audit has been completed. A third-party audit is recommended before
significant TVL accumulates on mainnet.
