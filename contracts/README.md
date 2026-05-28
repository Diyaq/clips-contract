# Soroban Contract Documentation

This repository contains the `clips_nft` Soroban smart contract.

## Contract: `clips_nft`

### Public methods (entrypoints)

#### `init(admin: Address)`

- **Type**: initialization
- **Requires**: `admin.require_auth()`
- **Returns**: `()`
- **Effects**: stores the admin, sets defaults (paused=false, platform recipient/admin, fee defaults, etc.)

#### `version() -> u32`

- **Type**: view
- **Returns**: contract version constant

#### `mint(to: Address, clip_id: u32, metadata_uri: String, image: Option<String>, animation_url: Option<String>, royalty: Royalty, is_soulbound: bool, signature: BytesN<64>) -> Result<TokenId, Error>`

- **Requires**: `to.require_auth()`
- **Requires**: contract not paused and minting not paused
- **Requires**: valid backend `signature` over the canonical mint payload
- **Returns**: newly minted `token_id`
- **Errors** (subset):
  - `ContractPaused`, `MintingPaused`
  - `SignerNotSet`, `InvalidSignature`
  - `ClipAlreadyMinted`, `ClipBlacklisted`
  - `InvalidRoyaltySplit`, `RoyaltyTooHigh`
  - `SoulboundTransferBlocked` (indirectly via transfer restrictions)

**Backend signature payload** (canonical mint payload):

```text
owner_hash = SHA-256(XDR(to))
uri_hash   = SHA-256(metadata_uri bytes)
message    = SHA-256( clip_id_le_4_bytes || owner_hash || uri_hash )
```

#### `batch_mint(to: Address, clip_ids: Vec<u32>, metadata_uris: Vec<String>, images: Vec<Option<String>>, animation_urls: Vec<Option<String>>, royalty: Royalty, is_soulbound: bool, signatures: Vec<BytesN<64>>) -> Result<Vec<TokenId>, Error>`

- **Requires**: `to.require_auth()`
- **Returns**: list of minted `TokenId`s
- **Constraints**:
  - Enforces maximum batch size
  - Validates input vector lengths
  - Verifies a backend signature for each clip

#### `transfer(from: Address, to: Address, token_id: TokenId, sale_price: i128, payment_asset: Option<Address>) -> Result<(), Error>`

- **Requires**: `from.require_auth()`
- **Blocks**: if token is soulbound (non-transferable)
- **Royalty semantics**:
  - If `sale_price > 0`, royalties are computed and paid via the configured royalty `asset_address`
  - For XLM royalties (`asset_address=None`), the caller marketplace must handle the transfer directly
- **Returns**: `()`

#### `burn(owner: Address, token_id: TokenId) -> Result<(), Error>`

- **Requires**: `owner.require_auth()`
- **Blocks**: if token is frozen
- **Returns**: `()`

#### `approve(caller: Address, operator: Option<Address>, token_id: TokenId) -> Result<(), Error>`

- **Requires**: `caller.require_auth()`
- **Returns**: `()`

#### `set_approval_for_all(caller: Address, operator: Address, approved: bool) -> Result<(), Error>`

- **Requires**: `caller.require_auth()`
- **Returns**: `()`

#### `owner_of(token_id: TokenId) -> Result<Address, Error>`

- **Type**: view

#### `token_uri(token_id: TokenId) -> Result<String, Error>`

- **Type**: view

#### `get_metadata(token_id: TokenId) -> Result<String, Error>`

- **Type**: view (alias)

#### `is_soulbound(token_id: TokenId) -> bool`

- **Type**: view

#### `royalty_info(token_id: TokenId, sale_price: i128) -> Result<RoyaltyInfo, Error>`

- **Type**: view
- **Computes**: `royalty_amount = sale_price * total_basis_points / 10_000` (safe math)
- **Returns**: receiver (primary recipient), computed amount, and `asset_address`

#### `get_royalty(token_id: TokenId) -> Result<Royalty, Error>`

- **Type**: view

#### `set_royalty(admin: Address, token_id: TokenId, new_royalty: Royalty) -> Result<(), Error>`

- **Requires**: admin

#### `update_royalty_recipient(caller: Address, token_id: TokenId, new_recipient: Address) -> Result<(), Error>`

- **Requires**: current primary recipient

#### `pay_royalty(payer: Address, token_id: TokenId, sale_price: i128) -> Result<(), Error>`

- **Requires**: `payer.require_auth()`
- **Purpose**: pays SEP-0041 royalties using the royalty config `asset_address`
- **Returns**: `()`
- **For XLM royalties (`asset_address=None`)**: returns `Error::InvalidRecipient`

#### `claim_royalties(caller: Address, token_id: TokenId) -> Result<(), Error>`

- **Requires**: primary recipient (index 0)
- **Purpose**: claims accumulated royalties stored per token

#### `pause(admin: Address, reason: Option<String>) -> Result<(), Error>`

- **Requires**: admin
- **Effect**: sets paused flag, schedules a 24-hour timelock, and stores an optional human-readable reason
- **Query reason**: `pause_reason()` returns the stored reason (cleared on `unpause`)

#### `pause_reason() -> Option<String>`

- **Type**: view
- **Returns**: the reason string set during the last `pause` call, or `None`

#### `unpause(admin: Address) -> Result<(), Error>`

- **Requires**: admin
- **Effect**: clears the paused flag and removes the stored reason

### Events (high-level)

- `MintEvent` — emitted when a new NFT is minted
- `TransferEvent` — emitted on transfers (and sometimes burns as standard ERC-721 Transfer)
- `RoyaltyPaidEvent` — emitted when royalties are paid
- `RoyaltyRecipientUpdatedEvent` / `RoyaltyUpdatedEvent` — royalty configuration updates
- `RoyaltyClaimedEvent` — when royalties are claimed
- `PauseScheduledEvent` / unpause-related events

### Royalty struct

#### `Royalty`

- `recipients: Vec<RoyaltyRecipient>`
- `asset_address: Option<Address>`

#### `RoyaltyRecipient`

- `recipient: Address`
- `basis_points: u32`

> The contract normalizes royalties by ensuring the platform recipient is included with **1% = 100 bps** when missing.

### Usage example (mint + royalty payment)

Pseudo-flow for a frontend + backend integration:

1. Backend signs the mint payload for `to` + `clip_id` + `metadata_uri`.
2. Frontend wallet (Freighter) submits `mint(...)` with the backend signature.
3. On sale/secondary transfer, marketplace submits `pay_royalty(...)`.

```text
mint(to, clip_id, metadata_uri, image, animation_url, royalty, is_soulbound, signature)
royalty_info(token_id, sale_price)
pay_royalty(payer, token_id, sale_price)
```
