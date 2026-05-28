# Soroban Storage Audit - ClipCash Clips NFT

## Current Storage Pattern
The contract utilizes Soroban's `storage()` interfaces: `instance`, `persistent`, and `temporary`. 

1. **Instance Storage**: Used for global singleton data like `Admin`, `Name`, `Symbol`, `TotalSupply`, `NextTokenId`, and `CircuitBreaker` states. This is efficient since instance storage is loaded once per contract invocation.
2. **Persistent Storage**: Used for individual token data, owner balances, approvals, and enumerability indices (`TokenIndex`, `OwnerTokenIndex`, `Balance`).
3. **Temporary Storage**: Used for gas tracking metrics (`CountMint`, `TotalGasMint`, `CountTransfer`, `TotalGasTransfer`).

## Efficient Keys Analysis
The acceptance criteria requires verifying the use of efficient keys (e.g., `u32` instead of `String`).
- The contract uses an enumerated `DataKey` type. Soroban `enum`s compile down to very efficient XDR representations (small integers/symbols), rather than raw byte strings.
- Token identifiers (`TokenId`) are typed as `u32` instead of string representations, which significantly optimizes the size of `DataKey::Token(TokenId)`, `DataKey::Approved(TokenId)`, and `DataKey::OwnerTokenIndex(Address, u32)`.
- **Conclusion**: The current `DataKey` enum design already perfectly adheres to the best practice of avoiding `String` or `Bytes` allocations for storage keys where possible. It employs lightweight `u32` and `Address` wrappers.

## Estimated Cost Per Mint
In Soroban, storage fees are calculated based on the size of the data and the ledger rent (CPU + RAM + State size).

During a mint operation (`mint` or `mint_with_signature`), the contract writes to:
- **Instance**: `NextTokenId`, `TotalSupply`, `CountMint`, `TotalGasMint`.
- **Persistent**: 
  - `DataKey::Token(TokenId)` (The largest write: contains the `TokenData` struct with `metadata_uri`, optional strings, and royalty array).
  - `DataKey::ClipIdMinted(u32)`
  - `DataKey::TokenIndex(u32)`
  - `DataKey::Balance(Address)`
  - `DataKey::OwnerTokenIndex(Address, u32)`
  - `DataKey::LastMintNonce(Address)` (for signatures)

**Cost Breakdown**:
- `TokenData` structure is dynamic. A typical NFT with an IPFS URI (`ipfs://...`), no optional strings, and one royalty recipient takes roughly **150 - 200 bytes** of XDR payload.
- Other integer/address indices consume about **30 - 60 bytes** each.
- **Estimated Total Write Size**: ~350 - 450 bytes per mint.
- **Estimated Fee**: Based on current Stellar Mainnet fees (which scale linearly with instructions and byte size), the average base cost of a mint operation is estimated around **~50,000 stroops (0.005 XLM)** per standard mint (tracked by `GAS_BASE_MINT` internally). For larger strings/metadata, it can scale to **0.01 XLM** per token.

### Recommendations for Future Optimization
While the current code cannot be changed for this audit, future optimizations could include:
1. Moving heavy on-chain optional metadata (`description`, `attributes`) entirely off-chain to the JSON resolved via `metadata_uri`.
2. Consolidating `TokenIndex` and `OwnerTokenIndex` if off-chain indexing (like Soroban RPC indexers/subgraphs) can handle enumeration, removing 2-3 persistent writes per mint.
