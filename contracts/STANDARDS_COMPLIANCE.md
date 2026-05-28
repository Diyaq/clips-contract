# Soroban NFT Standards Compliance

**Contract**: `clips_nft`  
**Review date**: 2026-05-28  
**Difficulty**: advanced (Issue #298)

---

## SEP-0041 (Token Interface for Smart Contracts)

**Status: Compliant (consumer)**

The contract uses `soroban_sdk::token::TokenClient` to interact with
SEP-0041 assets for all royalty payment and fund-withdrawal flows.
The `asset_address` field on the `Royalty` struct accepts any SEP-0041
contract address. When `None`, payments default to native XLM and the
marketplace is responsible for the on-chain transfer.

---

## Soroban NFT SIP (Draft, pre-ratification)

The Stellar ecosystem does not yet have a ratified NFT standard (SIP).
The table below tracks alignment with the widely-discussed draft requirements.

| Interface point | Implemented | Function |
|-----------------|-------------|----------|
| Token existence check | ✅ | `exists(token_id)` |
| Owner query | ✅ | `owner_of(token_id)` |
| Single-token transfer | ✅ | `transfer(from, to, token_id, sale_price, payment_asset)` |
| Soulbound / non-transferable | ✅ | `is_soulbound` flag; `Error::SoulboundTransferBlocked` |
| Single-token approval | ✅ | `approve(caller, operator, token_id)` |
| Operator approval (all) | ✅ | `set_approval_for_all` / `is_approved_for_all` |
| Revoke single approval | ✅ | `revoke_approval` |
| Revoke operator approval | ✅ | `revoke_all_approvals` |
| Burn | ✅ | `burn(owner, token_id)` |
| Metadata URI | ✅ | `token_uri(token_id)` + `get_metadata` alias |
| Metadata JSON | ✅ | `get_metadata_json(token_id)` (OpenSea standard) |
| Balance query | ✅ | `balance_of(owner)` |
| Total supply | ✅ | `total_supply()` |
| Global enumeration | ✅ | `token_by_index(index)` — O(1) |
| Per-owner enumeration | ✅ | `token_of_owner_by_index(owner, index)` — O(1) |
| Royalty info (ERC-2981 style) | ✅ | `royalty_info(token_id, sale_price)` |
| Multi-recipient royalty split | ✅ | `Royalty.recipients: Vec<RoyaltyRecipient>` |
| Royalty payment | ✅ | `pay_royalty(payer, token_id, sale_price)` |
| Admin pause with reason | ✅ | `pause(admin, reason)` / `pause_reason()` — Issue #299 |
| Minting-only pause | ✅ | `pause_minting` / `unpause_minting` |
| Metadata lock | ✅ | `lock_metadata` / `is_metadata_locked` |
| Token freeze | ✅ | `freeze_token` / `is_frozen` |
| Clip deduplication | ✅ (custom) | `clip_token_id(clip_id)` |
| Batch mint | ✅ (custom) | `batch_mint(...)` — max 25 per tx |
| Batch burn | ✅ (custom) | `batch_burn(owner, token_ids)` |

---

## OpenSea Metadata Standard

`get_metadata_json` serialises:

```json
{
  "metadata_uri": "ipfs://...",
  "image": "https://...",
  "animation_url": "https://...",
  "description": "...",
  "external_url": "https://...",
  "attributes": [{ "trait_type": "...", "value": "..." }]
}
```

Fields that are `None` are omitted from the output.

---

## Adjustments Made

1. **`pause` now accepts an optional reason** (Issue #299) — stores the string
   in instance storage under `DataKey::PauseReason`; cleared on `unpause`.
   Emits `PauseWithReasonEvent { active_at, reason }`.
2. **`pause_reason()` view function** — lets users and indexers query why the
   contract was paused without parsing event history.
3. **`revoke_approval` / `revoke_all_approvals`** — explicit revoke paths per
   emerging Soroban NFT draft guidance.
4. **O(1) enumeration indexes** — `TokenIndex` and `OwnerTokenIndex` maintained
   by mint, burn, and transfer, matching ERC-721 Enumerable semantics.

---

## Outstanding Gaps

| Gap | Priority | Notes |
|-----|----------|-------|
| `payment_token()` convenience view | Low | Returns configured royalty asset |
| Formal SIP compliance reference | N/A | No ratified Soroban NFT SIP exists yet |
| Third-party security audit | **High** | Required before significant mainnet TVL |
| XLM royalty native handling | Medium | Currently returns `InvalidRecipient`; marketplace must handle |
| `safeTransfer` receiver hooks | Low | No Soroban equivalent of ERC-721 receiver callbacks yet |
