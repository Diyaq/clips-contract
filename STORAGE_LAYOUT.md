# ClipCash NFT — Storage Layout

This document maps every `DataKey` variant to the Soroban storage tier in which it lives,
the Rust type stored under that key, and the operations that read or write it.

---

## Storage Tiers

| Tier | Cost | Lifetime | Use |
|------|------|----------|-----|
| **Instance** | Cheap — loaded once per transaction, shared across all calls in the tx | Lives as long as the contract instance | Global contract config and counters |
| **Persistent** | Per-entry fee paid per ledger | Survives until its own TTL expires (admin must bump) | Per-token and per-user state |
| **Temporary** | Cheapest | Expires automatically after a short TTL | Short-lived metrics that tolerate reset |

---

## Instance Storage Keys

All instance keys share the contract instance's TTL and are loaded together in a single read.

| Key | Type | Default | Description | Written by |
|-----|------|---------|-------------|------------|
| `Admin` | `Address` | — (required at init) | Contract administrator | `init` |
| `NextTokenId` | `u32` | `1` | Monotonically increasing token ID counter. `total_supply = NextTokenId - 1` (before `TotalSupply` was added) | `init`, `mint`, `batch_mint` |
| `Paused` | `bool` | `false` | Global pause flag. Blocks `mint` and `transfer` when `true` | `init`, `pause`, `unpause` |
| `MintingPaused` | `bool` | `false` | Mint-only pause flag. Blocks `mint` and `batch_mint` independently of `Paused` | `pause_minting`, `unpause_minting` |
| `PauseUnlockTime` | `u64` | absent | Ledger timestamp at which a scheduled pause becomes active (24-hour timelock) | `pause` (with reason) |
| `PauseReason` | `String` | absent | Human-readable reason stored when the contract is paused | `pause` (with reason), removed by `unpause` |
| `Signer` | `BytesN<32>` | absent | 32-byte Ed25519 public key of the trusted backend signer. Must be set before minting | `set_signer` |
| `BackendAddress` | `Address` | absent | Alternative backend address (Stellar address form of the backend) | `set_backend_address` |
| `PlatformRecipient` | `Address` | admin at init | Address that receives the automatic 1% platform royalty cut | `init`, `set_platform_recipient` |
| `PlatformFeeBps` | `u32` | `100` (1%) | Platform fee in basis points (max 10 000) | `set_platform_fee` |
| `DefaultRoyaltyBps` | `u32` | `0` | Default royalty in basis points applied when no explicit royalty is supplied | `set_default_royalty` |
| `DefaultRoyaltyAsset` | `Option<Address>` | `None` (XLM) | Default SEP-0041 asset contract for royalty payments; `None` means native XLM | `set_default_royalty_asset` |
| `Name` | `String` | `"ClipCash Clips"` | Collection name returned by `name()` | `set_name` |
| `Symbol` | `String` | `"CLIP"` | Collection symbol returned by `symbol()` | `set_symbol` |
| `TotalSupply` | `u32` | `0` | Count of currently live (non-burned) tokens | `mint`, `batch_mint`, `burn`, `burn_with_refund` |
| `TotalGasMint` | `u64` | `0` | Cumulative synthetic gas for all mint operations (also mirrored to temporary) | `mint`, `mint_core`, `batch_mint` |
| `CountMint` | `u64` | `0` | Total number of mint operations (also mirrored to temporary) | `mint`, `mint_core`, `batch_mint` |
| `TotalGasTransfer` | `u64` | `0` | Cumulative synthetic gas for all transfer operations | `transfer` |
| `CountTransfer` | `u64` | `0` | Total number of transfer operations | `transfer` |
| `TotalPlatformFees` | `i128` | `0` | Total platform royalty revenue collected on-chain (asset smallest units) | `init`, `pay_royalty` |
| `MintCooldownSeconds` | `u64` | `DEFAULT_MINT_COOLDOWN_SECONDS` | Minimum seconds that must elapse between two mints from the same address | `set_mint_cooldown` |
| `CircuitBreakerEnabled` | `bool` | `DEFAULT_CIRCUIT_BREAKER_ENABLED` | Whether the mint-rate circuit breaker is active | `set_circuit_breaker_enabled` |
| `CircuitBreakerThreshold` | `u64` | `DEFAULT_CIRCUIT_BREAKER_THRESHOLD` | Maximum mints allowed within the current window before the breaker trips | `set_circuit_breaker_threshold` |
| `CircuitBreakerWindowSeconds` | `u64` | `DEFAULT_CIRCUIT_BREAKER_WINDOW_SECONDS` | Duration (seconds) of each circuit-breaker measurement window | `set_circuit_breaker_window` |
| `CircuitBreakerWindowStart` | `u64` | `0` | Ledger timestamp at which the current window began | `update_circuit_breaker_counter`, `reset_circuit_breaker` |
| `CircuitBreakerWindowCount` | `u64` | `0` | Number of mints recorded in the current window | `update_circuit_breaker_counter`, `reset_circuit_breaker` |
| `WithdrawXlmRequest` | `WithdrawRequest { amount: i128, unlock_time: u64 }` | absent | Pending withdrawal request placed by admin (48-hour timelock) | `request_withdraw_asset`, `withdraw_xlm`, removed by `withdraw_asset` |
| `LastWithdrawalTime` | `u64` | absent | Ledger timestamp of the last executed withdrawal | `withdraw_asset`, `withdraw_xlm` |

---

## Persistent Storage Keys

Each persistent key has its own TTL and incurs a per-entry ledger fee.

| Key | Type | Description | Written by | Removed by |
|-----|------|-------------|------------|------------|
| `Token(TokenId)` | `TokenData` | Full token record: owner, clip_id, metadata_uri, image, animation_url, description, external_url, attributes, royalty, is_soulbound, is_locked | `mint`, `mint_core`, `batch_mint`, `transfer`, `set_royalty`, `update_royalty_recipient` | `burn`, `burn_with_refund` |
| `ClipIdMinted(clip_id: u32)` | `TokenId` | Dedup guard mapping an off-chain clip ID to the on-chain token ID that was minted for it. Prevents the same clip from being minted twice | `mint`, `mint_core`, `batch_mint` (via `mark_clip_minted`) | `burn`, `burn_with_refund` |
| `BlacklistedClip(clip_id: u32)` | `bool` | Present and `true` when a clip ID has been blacklisted by the admin. Checked during mint | `blacklist_clip` | — (permanent until manually unset) |
| `Approved(token_id: TokenId)` | `Address` | Single-token operator approval: the address approved to transfer this specific token | `approve` | `revoke_approval`, cleared on transfer |
| `ApprovalForAll(owner: Address, operator: Address)` | `bool` | Operator approval for all tokens owned by `owner`. `true` = `operator` may transfer any of `owner`'s tokens | `set_approval_for_all` | `revoke_all_approvals` |
| `Balance(owner: Address)` | `u32` | Number of tokens currently owned by `owner` | `mint`, `mint_core`, `batch_mint`, `burn`, `burn_with_refund`, `transfer` | — |
| `TokenIndex(global_pos: u32)` | `TokenId` | Global enumeration index mapping a 0-based position to a token ID. Enables `token_by_index` in O(1) | `mint`, `batch_mint` | Updated/compacted by `burn` |
| `OwnerTokenIndex(owner: Address, position: u32)` | `TokenId` | Per-owner enumeration index mapping a 0-based position within the owner's holdings to a token ID. Enables `token_of_owner_by_index` in O(1) | `mint`, `mint_core`, `batch_mint`, `transfer` | Updated/compacted by `burn`, `transfer` |
| `RoyaltyBalance(token_id: TokenId)` | `i128` | Accrued royalty balance for a token (asset smallest units). Incremented by `pay_royalty`, cleared by `claim_royalties` or `burn_with_refund` | `pay_royalty` (internal) | `claim_royalties`, `burn_with_refund` |
| `Frozen(token_id: TokenId)` | `bool` | Present and `true` when an admin has frozen a token. Frozen tokens cannot be transferred or burned | `freeze_token` | `unfreeze_token` |
| `CustomTokenUri(token_id: TokenId)` | `String` | Owner-supplied URI override for a token. Takes precedence over the URI stored in `Token(token_id)` when set | `set_token_uri` | — |
| `MetadataRefreshTime(token_id: TokenId)` | `u64` | Ledger timestamp of the last metadata refresh for a token. Used to enforce the 30-day refresh cooldown | `refresh_metadata` (admin) | — |

---

## Temporary Storage Keys

Temporary entries expire automatically after a short TTL. Data may be lost between ledger closings.

> **Note:** `CountMint` and `TotalGasMint` are written to **both** instance storage and temporary storage by different code paths. This is an inconsistency in the current contract implementation — the view functions `average_gas_mint()` and `total_mints()` read from **temporary** storage, while `get_avg_gas_cost()` reads from **instance** storage.

| Key | Type | Description | Written by |
|-----|------|-------------|------------|
| `TotalGasMint` | `u64` | Short-lived mirror of cumulative synthetic gas for mint operations (read by `average_gas_mint`) | `mint` |
| `CountMint` | `u64` | Short-lived mirror of total mint count (read by `total_mints`, `average_gas_mint`) | `mint` |

---

## Key Sizing Notes

Soroban keys contribute to ledger entry size, which affects per-entry fees:

| Key shape | XDR words | Notes |
|-----------|-----------|-------|
| No-payload variants (`Admin`, `Paused`, etc.) | 1 word | Minimum possible size |
| Single `u32` payload (`Token(u32)`, `ClipIdMinted(u32)`) | 2 words | Smallest per-token key |
| Single `Address` payload (`Balance(Address)`) | ~8 words | Depends on address encoding |
| Two-address payload (`ApprovalForAll(Address, Address)`) | ~16 words | Largest variant — prefer clearing unused approvals |

---

## Storage Operations per Function

| Function | Instance reads | Instance writes | Persistent reads | Persistent writes | Persistent removes |
|----------|---------------|-----------------|-----------------|-------------------|--------------------|
| `init` | 0 | 9 | 0 | 0 | 0 |
| `mint` | 4 | 3 | 2 | 4 | 0 |
| `batch_mint` (n items) | 3 + n | 2 + 3n | 2n | 4n | 0 |
| `transfer` | 1 | 2 | 1 | 3 | 1 |
| `burn` | 1 | 1 | 1 | 1 | 2 |
| `burn_with_refund` | 1 | 1 | 1 | 1 | 3 |
| `approve` | 0 | 0 | 1 | 1 | 0 |
| `set_approval_for_all` | 0 | 0 | 0 | 1 | 0 |
| `pay_royalty` | 1 | 1 | 2 | 1 | 0 |
| `claim_royalties` | 0 | 0 | 2 | 0 | 1 |
| `freeze_token` | 0 | 0 | 1 | 1 | 0 |
| `set_royalty` | 1 | 0 | 1 | 1 | 0 |

---

## TokenData Structure

`Token(token_id)` stores the following packed struct:

```rust
pub struct TokenData {
    pub owner: Address,          // current owner
    pub clip_id: u32,            // off-chain clip identifier
    pub is_soulbound: bool,      // true = non-transferable
    pub metadata_uri: String,    // primary IPFS / Arweave URI
    pub image: Option<String>,   // static thumbnail URL
    pub animation_url: Option<String>, // animated preview URL
    pub description: Option<String>,
    pub external_url: Option<String>,
    pub attributes: Vec<Attribute>,
    pub royalty: Royalty,        // multi-recipient royalty config
    pub is_locked: bool,         // reserved for future use
}
```

Packing all token fields into a single entry reduces persistent writes from 4 (owner + clip_id + metadata + royalty as separate entries) down to **1** per mint.
