//! ClipCash NFT - Soroban Smart Contract
//!
//! This contract enables minting video clips as NFTs on the Stellar network
//! with built-in royalty support for content creators.
//! Royalties can be paid in XLM or any custom Stellar asset (SEP-0041 token).
//!
//! # Clip verification
//!
//! Before a clip can be minted the backend must sign a verification payload
//! with its Ed25519 private key. The contract verifies the signature on-chain
//! using `env.crypto().ed25519_verify()`.
//!
//! ## Payload format
//!
//! ```text
//! payload = SHA-256( clip_id_le_bytes || owner_address_bytes || metadata_uri_bytes )
//! ```
//!
//! - `clip_id` is encoded as 4 little-endian bytes.
//! - `owner_address_bytes` is the raw XDR encoding of the `Address` produced by
//!   `env.crypto().sha256(&owner.to_xdr(&env))` — i.e. the contract hashes the
//!   address XDR so the payload is always a fixed-size 32-byte digest.
//! - The final SHA-256 over the concatenation is what gets signed.
//!
//! The backend registers its Ed25519 public key once via `set_signer` (admin only).
//! The public key is stored in instance storage under `DataKey::Signer`.
//!
//! # Storage layout & gas cost notes
//!
//! ## Storage tiers used
//! - `instance`   – cheap, loaded once per tx, shared across all calls in the tx.
//!                  Used for: Admin, NextTokenId, Paused, Signer.
//! - `persistent` – per-entry fee, survives ledger expiry extension.
//!                  Used for: TokenData (owner+clip_id+metadata+royalty packed),
//!                  ClipIdMinted (dedup guard).
//!
//! ## Estimated storage operations per function
//!
//! ### `mint`
//! | Op              | Tier       | Count |
//! |-----------------|------------|-------|
//! | instance read   | instance   | 4     | (Admin, NextTokenId, Paused, Signer)
//! | instance write  | instance   | 1     | (NextTokenId++)
//! | persistent read | persistent | 1     | (ClipIdMinted dedup check)
//! | persistent write| persistent | 2     | (TokenData, ClipIdMinted)
//! Total persistent writes: **2** (Optimized from 4)
//!
//! ### `transfer`
//! | Op              | Tier       | Count |
//! |-----------------|------------|-------|
//! | instance read   | instance   | 1     | (Paused)
//! | persistent read | persistent | 1     | (TokenData — owner check)
//! | persistent write| persistent | 1     | (TokenData — new owner)
//! Total persistent writes: **1**
//!
//! ### `burn`
//! | Op              | Tier       | Count |
//! |-----------------|------------|-------|
//! | persistent read | persistent | 1     | (TokenData — owner check + clip_id)
//! | persistent remove| persistent| 2     | (TokenData, ClipIdMinted)
//! Total persistent removes: **2** (Optimized from 4)
//!
//! ## Removed counters / indexes (vs. earlier version)
//! - `Balance(Address)` — per-address token counter removed.
//! - `TokenCount` — replaced by `next_token_id - 1`.
//! - `TokenClipId(TokenId)` — clip_id packed into `TokenData`.

#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype,
    symbol_short, xdr::ToXdr, Address, Bytes, BytesN, Env, String, Vec,
};

/// Contract version
pub const VERSION: u32 = 1;

/// Synthetic gas weights for monitoring
const GAS_BASE_MINT: u64 = 55_000;
const GAS_BASE_TRANSFER: u64 = 1_500;
const GAS_PER_BYTE: u64 = 1;


/// Custom errors for the NFT contract
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum Error {
    /// Operation not authorized
    Unauthorized = 1,
    /// Invalid token ID
    InvalidTokenId = 2,
    /// Token already minted
    TokenAlreadyMinted = 3,
    /// Royalty too high (max 10000 basis points = 100%)
    RoyaltyTooHigh = 4,
    /// Invalid recipient
    InvalidRecipient = 5,
    /// Sale price must be greater than zero
    InvalidSalePrice = 6,
    /// Contract is paused — minting and transfers are blocked
    ContractPaused = 7,
    /// Backend signature over the mint payload is invalid
    InvalidSignature = 8,
    /// No backend signer public key has been registered yet
    SignerNotSet = 9,
    /// Royalty split is invalid
    InvalidRoyaltySplit = 10,
    /// Token is soulbound (non-transferable)
    SoulboundTransferBlocked = 11,
    /// Royalty calculation would overflow
    RoyaltyOverflow = 12,
    /// Clip is blacklisted
    ClipBlacklisted = 13,
    /// Caller is not authorized to approve
    NotAuthorizedToApprove = 14,
}

/// Token ID type
pub type TokenId = u32;

/// Packs owner address, originating clip_id, metadata, and royalty into a single persistent entry.
///
/// Combining these fields eliminates the separate `Metadata` and `Royalty`
/// entries that were previously written on every mint.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenData {
    pub owner: Address,
    /// The off-chain clip identifier this token was minted for.
    pub clip_id: u32,
    /// Whether this token is soulbound (non-transferable)
    pub is_soulbound: bool,
    /// Metadata URI for the token
    pub metadata_uri: String,
    /// Royalty configuration
    pub royalty: Royalty,
}

/// Royalty information stored per token.
/// `asset_address` is `None` for native XLM, or `Some(contract_address)`
/// for any SEP-0041 custom Stellar asset.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoyaltyRecipient {
    pub recipient: Address,
    pub basis_points: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Royalty {
    /// Multi-recipient split. Platform recipient is automatically added with 1%
    /// if not present.
    pub recipients: Vec<RoyaltyRecipient>,
    /// Optional SEP-0041 asset contract address.
    /// `None` → royalties expected in XLM (native).
    pub asset_address: Option<Address>,
}

/// Royalty payment info returned by `royalty_info()`.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoyaltyInfo {
    pub receiver: Address,
    /// Royalty amount in the same denomination as `sale_price`
    pub royalty_amount: i128,
    /// `None` → pay in XLM; `Some(addr)` → pay in that SEP-0041 token.
    pub asset_address: Option<Address>,
}

/// Storage keys
///
/// Key sizing notes:
/// - Enum variants with no payload (Admin, NextTokenId, Paused) are 1-word keys.
/// - Variants with a u32 payload (Token, ClipIdMinted) are
///   2-word keys — the smallest possible for per-token entries.
#[contracttype]
pub enum DataKey {
    /// Contract administrator address (instance storage)
    Admin,
    /// Monotonically increasing token ID counter (instance storage).
    /// `total_supply = NextTokenId - 1` — no separate TokenCount needed.
    NextTokenId,
    /// Pause flag (instance storage)
    Paused,
    /// Packed owner + clip_id + metadata + royalty for a token (persistent storage)
    Token(TokenId),
    /// Dedup guard: clip_id → token_id (persistent storage)
    ClipIdMinted(u32),
    /// Ed25519 public key of the trusted backend signer (instance storage)
    Signer,
    /// Platform recipient used for default 1% royalty cut
    PlatformRecipient,
    /// Total synthetic gas used in minting (instance storage)
    TotalGasMint,
    /// Total number of successful mints (instance storage)
    CountMint,
    /// Total synthetic gas used in transfers (instance storage)
    TotalGasTransfer,
    /// Total number of successful transfers (instance storage)
    CountTransfer,
    /// Collection name (instance storage)
    Name,
    /// Collection symbol (instance storage)
    Symbol,
    /// Blacklisted clip IDs (persistent storage)
    BlacklistedClip(u32),
    /// Per-token operator approval (persistent storage)
    Approved(TokenId),
    /// Operator approvals across all owner tokens (persistent storage)
    ApprovalForAll(Address, Address),
    /// Total platform royalty revenue collected (instance storage)
    TotalPlatformFees,
}

/// Event emitted when a new NFT is minted
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MintEvent {
    pub to: Address,
    pub clip_id: u32,
    pub token_id: TokenId,
    pub metadata_uri: String,
    pub gas_used: u64,
}

/// Event emitted when an NFT is burned.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BurnEvent {
    pub owner: Address,
    pub token_id: TokenId,
    pub clip_id: u32,
}

/// Event emitted when NFT ownership changes.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferEvent {
    pub token_id: TokenId,
    pub from: Address,
    pub to: Address,
    pub gas_used: u64,
}

/// Event emitted when a clip is blacklisted.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlacklistEvent {
    pub clip_id: u32,
}

/// Event emitted when token approval is updated.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalEvent {
    pub owner: Address,
    pub operator: Address,
    pub token_id: TokenId,
}

/// Event emitted when operator-for-all approval is updated.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalForAllEvent {
    pub owner: Address,
    pub operator: Address,
    pub approved: bool,
}

/// Event emitted when royalty is paid.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoyaltyPaidEvent {
    pub token_id: TokenId,
    pub from: Address,
    pub to: Address,
    pub amount: i128,
}

/// Event emitted when royalty recipient is updated.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoyaltyRecipientUpdatedEvent {
    pub token_id: TokenId,
    pub old_recipient: Address,
    pub new_recipient: Address,
}

/// Event emitted when the contract is upgraded.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeEvent {
    pub new_wasm_hash: BytesN<32>,
}

/// NFT Contract
#[contract]
pub struct ClipsNftContract;

#[contractimpl]
impl ClipsNftContract {
    /// Initialize the contract with an admin address.
    pub fn init(env: Env, admin: Address) {
        env.storage().instance().set(&DataKey::Admin, &admin);
        // NextTokenId starts at 1; total_supply = NextTokenId - 1
        env.storage().instance().set(&DataKey::NextTokenId, &1u32);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::PlatformRecipient, &admin);
        env.storage().instance().set(&DataKey::TotalGasMint, &0u64);
        env.storage().instance().set(&DataKey::CountMint, &0u64);
        env.storage().instance().set(&DataKey::TotalGasTransfer, &0u64);
        env.storage().instance().set(&DataKey::CountTransfer, &0u64);
        env.storage().instance().set(&DataKey::TotalPlatformFees, &0i128);
        // Signer is not set at init — call set_signer before minting.
    }

    /// Register (or rotate) the backend Ed25519 public key used to verify
    /// clip ownership before minting. Only callable by the admin.
    ///
    /// # Arguments
    /// * `admin`  - Must be the contract admin
    /// * `pubkey` - 32-byte Ed25519 public key of the trusted backend signer
    pub fn set_signer(env: Env, admin: Address, pubkey: BytesN<32>) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Signer, &pubkey);
        Ok(())
    }

    /// Return the currently registered backend signer public key, if any.
    pub fn get_signer(env: Env) -> Option<BytesN<32>> {
        env.storage().instance().get(&DataKey::Signer)
    }

    // -------------------------------------------------------------------------
    // Upgradeability
    // -------------------------------------------------------------------------

    /// Upgrade the contract to a new WASM implementation.
    /// Only callable by the admin.
    ///
    /// Uses Soroban's built-in `update_current_contract_wasm` which replaces
    /// the current contract code with the new WASM hash while preserving all
    /// instance and persistent storage.
    ///
    /// # Arguments
    /// * `admin`         - Must be the contract admin
    /// * `new_wasm_hash` - 32-byte SHA-256 hash of the new WASM blob
    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.deployer().update_current_contract_wasm(new_wasm_hash.clone());
        env.events().publish(
            (symbol_short!("upgrade"),),
            UpgradeEvent { new_wasm_hash },
        );
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Pausable
    // -------------------------------------------------------------------------

    /// Pause the contract. Blocks `mint` and `transfer` until unpaused.
    /// Only callable by the admin.
    pub fn pause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events().publish((symbol_short!("paused"),), ());
        Ok(())
    }

    /// Unpause the contract, re-enabling `mint` and `transfer`.
    /// Only callable by the admin.
    pub fn unpause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events().publish((symbol_short!("unpaused"),), ());
        Ok(())
    }

    /// Returns `true` if the contract is currently paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
    }

    /// Blacklist a clip ID, preventing it from being minted.
    /// Only callable by the admin.
    pub fn blacklist_clip(env: Env, admin: Address, clip_id: u32) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage()
            .persistent()
            .set(&DataKey::BlacklistedClip(clip_id), &true);
        env.events()
            .publish((symbol_short!("blacklist"),), BlacklistEvent { clip_id });
        Ok(())
    }

    fn emit_mint_event(
        env: &Env,
        token_id: TokenId,
        from: &Address,
        to: &Address,
        clip_id: u32,
    ) {
        env.events().publish(
            (symbol_short!("mint"), token_id, from.clone(), to.clone()),
            MintEvent {
                to: to.clone(),
                clip_id,
                token_id,
            },
        );
    }

    fn emit_transfer_event(env: &Env, token_id: TokenId, from: &Address, to: &Address) {
        env.events().publish(
            (symbol_short!("transfer"), token_id, from.clone(), to.clone()),
            TransferEvent {
                token_id,
                from: from.clone(),
                to: to.clone(),
            },
        );
    }

    fn emit_royalty_paid_event(
        env: &Env,
        token_id: TokenId,
        from: &Address,
        to: &Address,
        amount: i128,
    ) {
        env.events().publish(
            (
                symbol_short!("royalty"),
                token_id,
                from.clone(),
                to.clone(),
                amount,
            ),
            RoyaltyPaidEvent {
                token_id,
                from: from.clone(),
                to: to.clone(),
                amount,
            },
        );
    }

    fn require_clip_owner(owner: &Address) {
        owner.require_auth();
    }

    fn only_clip_owner(
        env: &Env,
        owner: &Address,
        clip_id: u32,
        metadata_uri: &String,
        signature: &BytesN<64>,
    ) -> Result<(), Error> {
        Self::verify_clip_signature(env, owner, clip_id, metadata_uri, signature)
    }

    fn only_clip_owner_with_nonce(
        env: &Env,
        owner: &Address,
        clip_id: u32,
        metadata_uri: &String,
        nonce: u64,
        signature: &BytesN<64>,
    ) -> Result<(), Error> {
        Self::verify_clip_signature_with_nonce(env, owner, clip_id, metadata_uri, nonce, signature)?;
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Core NFT operations
    // -------------------------------------------------------------------------

    /// Mint a new NFT for a video clip.
    ///
    /// Requires a valid Ed25519 `signature` from the registered backend signer
    /// over the canonical mint payload, proving the clip exists and belongs to
    /// `to`. The payload is:
    ///
    /// ```text
    /// payload = SHA-256(
    ///     clip_id_le_4_bytes
    ///     || SHA-256(owner_address_xdr)   // 32 bytes
    ///     || SHA-256(metadata_uri_bytes)  // 32 bytes
    /// )
    /// ```
    ///
    /// Storage writes (persistent): TokenData, Metadata, Royalty, ClipIdMinted = **4**
    /// Instance writes: NextTokenId = **1**
    ///
    /// # Arguments
    /// * `to`           - Address that will own the NFT (must match the signed payload)
    /// * `clip_id`      - Unique off-chain clip identifier (must match the signed payload)
    /// * `metadata_uri` - IPFS or Arweave URI (must match the signed payload)
    /// * `royalty`      - Royalty configuration
    /// * `is_soulbound` - Whether the token is soulbound (non-transferable)
    /// * `signature`    - 64-byte Ed25519 signature from the backend signer
    pub fn mint(
        env: Env,
        to: Address,
        clip_id: u32,
        metadata_uri: String,
        royalty: Royalty,
        is_soulbound: bool,
        signature: BytesN<64>,
    ) -> Result<TokenId, Error> {
        Self::require_clip_owner(&to);
        Self::require_not_paused(&env)?;
        if env.storage().instance().get::<DataKey, bool>(&DataKey::MintingPaused).unwrap_or(false) {
            return Err(Error::MintingPaused);
        }
        Self::enforce_mint_cooldown(&env, &to)?;
        Self::check_circuit_breaker(&env, 1)?;
        Self::validate_url(&env, &image)?;
        Self::validate_url(&env, &animation_url)?;
        Self::only_clip_owner(&env, &to, clip_id, &metadata_uri, &signature)?;

        // Dedup check — one persistent read
        if env
            .storage()
            .persistent()
            .has(&DataKey::ClipIdMinted(clip_id))
        {
            return Err(Error::TokenAlreadyMinted);
        }

        if env
            .storage()
            .persistent()
            .get(&DataKey::BlacklistedClip(clip_id))
            .unwrap_or(false)
        {
            return Err(Error::ClipBlacklisted);
        }

        let royalty = Self::normalize_royalty(&env, royalty)?;

        // One instance read
        let token_id: TokenId = env
            .storage()
            .instance()
            .get(&DataKey::NextTokenId)
            .unwrap_or(1);

        // 2 persistent writes (Optimized from 4)
        env.storage().persistent().set(
            &DataKey::Token(token_id),
            &TokenData {
                owner: to.clone(),
                clip_id,
                is_soulbound,
                metadata_uri: metadata_uri.clone(),
                royalty,
            },
        );
        env.storage()
            .persistent()
            .set(&DataKey::ClipIdMinted(clip_id), &token_id);

        // 1 instance write
        env.storage()
            .instance()
            .set(&DataKey::NextTokenId, &(token_id + 1));

        let gas_used = GAS_BASE_MINT
            .saturating_add((metadata_uri.len() as u64).saturating_mul(GAS_PER_BYTE));

        // Update totals
        let total_gas: u64 = env.storage().instance().get(&DataKey::TotalGasMint).unwrap_or(0);
        let count: u64 = env.storage().instance().get(&DataKey::CountMint).unwrap_or(0);
        env.storage().instance().set(&DataKey::TotalGasMint, &total_gas.saturating_add(gas_used));
        env.storage().instance().set(&DataKey::CountMint, &count.saturating_add(1));

        env.events().publish(
            (symbol_short!("mint"),),
            MintEvent { to, clip_id, token_id, metadata_uri, gas_used },
        );

        let data = TokenData {
            owner: to.clone(), clip_id, is_soulbound,
            metadata_uri: metadata_uri.clone(), image, animation_url,
            description: None, external_url: None,
            attributes: Vec::new(&env), royalty, is_locked: false,
        };
        env.storage().persistent().set(&DataKey::Token(token_id), &data);
        Self::bump_persistent_ttl(&env, &DataKey::Token(token_id));
        Self::mark_clip_minted(&env, clip_id, token_id);

        // Update supply + enumeration indexes
        let supply: u32 = env.storage().instance().get(&DataKey::TotalSupply).unwrap_or(0);
        env.storage().instance().set(&DataKey::TotalSupply, &(supply + 1));
        env.storage().persistent().set(&DataKey::TokenIndex(supply), &token_id);
        let bal: u32 = env.storage().persistent().get(&DataKey::Balance(to.clone())).unwrap_or(0);
        env.storage().persistent().set(&DataKey::Balance(to.clone()), &(bal + 1));
        env.storage().persistent().set(&DataKey::OwnerTokenIndex(to.clone(), bal), &token_id);

        Self::record_mint_timestamp(&env, &to);
        Self::update_circuit_breaker_counter(&env, 1);

        let count: u64 = env.storage().temporary().get(&DataKey::CountMint).unwrap_or(0);
        env.storage().temporary().set(&DataKey::CountMint, &(count + 1));
        let gas: u64 = env.storage().temporary().get(&DataKey::TotalGasMint).unwrap_or(0);
        env.storage().temporary().set(&DataKey::TotalGasMint, &gas.saturating_add(GAS_BASE_MINT));

        let mint_from = env.current_contract_address();
        Self::emit_mint_event(&env, token_id, &mint_from, &to, clip_id);
        Self::emit_transfer_event(&env, token_id, &mint_from, &to);
        Ok(token_id)
    }

    // -------------------------------------------------------------------------
    // Approvals
    // -------------------------------------------------------------------------

    /// Approve an operator to transfer a specific token.
    pub fn approve(
        env: Env,
        caller: Address,
        operator: Option<Address>,
        token_id: TokenId,
    ) -> Result<(), Error> {
        caller.require_auth();
        Self::require_not_paused(&env)?;
        if env.storage().persistent().get::<DataKey, bool>(&DataKey::Frozen(token_id)).unwrap_or(false) {
            return Err(Error::TokenFrozen);
        }
        let mut data: TokenData = Self::load_token(&env, token_id)?;
        if from != data.owner { return Err(Error::Unauthorized); }
        if data.is_soulbound { return Err(Error::SoulboundTransferBlocked); }

        if sale_price > 0 {
            let royalty = data.royalty.clone();
            let pay_asset = match royalty.asset_address.clone() {
                Some(a) => a,
                None => payment_asset.ok_or(Error::InvalidRecipient)?,
            };
            to.require_auth();
            Self::acquire_reentrancy_lock(&env)?;
            let token_client = soroban_sdk::token::TokenClient::new(&env, &pay_asset);
            let mut cum_bps: u32 = 0;
            let mut cum_amt: i128 = 0;
            for i in 0..royalty.recipients.len() {
                let split = royalty.recipients.get(i).ok_or(Error::InvalidRoyaltySplit)?;
                cum_bps = cum_bps.saturating_add(split.basis_points);
                let total = Self::calculate_royalty(sale_price, cum_bps)?;
                let amount = total.saturating_sub(cum_amt);
                cum_amt = total;
                if amount > 0 {
                    token_client.transfer(&to, &split.recipient, &amount);
                    Self::emit_royalty_paid_event(
                        &env,
                        token_id,
                        &to,
                        &split.recipient,
                        amount,
                    );
                }
            }
            Self::release_reentrancy_lock(&env);
        }

        env.storage().persistent().remove(&DataKey::Approved(token_id));
        data.owner = to.clone();
        env.storage().persistent().set(&DataKey::Token(token_id), &data);

        let owner = Self::owner_of(env.clone(), token_id)?;
        
        // Caller must be owner or approved for all
        if caller != owner && !Self::is_approved_for_all(env.clone(), owner.clone(), caller.clone()) {
            return Err(Error::NotAuthorizedToApprove);
        }

        if let Some(op) = operator.clone() {
            env.storage().persistent().set(&DataKey::Approved(token_id), &op);
            env.events().publish(
                (symbol_short!("approve"),),
                ApprovalEvent { owner, operator: op, token_id },
            );
        } else {
            env.storage().persistent().remove(&DataKey::Approved(token_id));
        }

        Self::emit_transfer_event(&env, token_id, &from, &to);
        Ok(())
    }

    /// Set or unset an operator to manage all of the caller's tokens.
    pub fn set_approval_for_all(
        env: Env,
        caller: Address,
        operator: Address,
        approved: bool,
    ) -> Result<(), Error> {
        caller.require_auth();
        Self::require_not_paused(&env)?;

        env.storage()
            .persistent()
            .set(&DataKey::ApprovalForAll(caller.clone(), operator.clone()), &approved);

        env.events().publish(
            (symbol_short!("appr_all"),),
            ApprovalForAllEvent { owner: caller, operator, approved },
        );

        Ok(())
    }

    pub fn approve(env: Env, caller: Address, operator: Option<Address>, token_id: TokenId) -> Result<(), Error> {
        caller.require_auth();
        let data: TokenData = Self::load_token(&env, token_id)?;
        if data.owner != caller {
            let approved_all = env.storage().persistent().get::<DataKey, bool>(&DataKey::ApprovalForAll(data.owner.clone(), caller.clone())).unwrap_or(false);
            if !approved_all { return Err(Error::Unauthorized); }
        }
        match operator {
            Some(op) => {
                env.storage().persistent().set(&DataKey::Approved(token_id), &op);
                env.events().publish((symbol_short!("approval"), token_id, data.owner.clone(), op.clone()), ApprovalEvent { owner: data.owner, operator: op, token_id });
            }
            None => { env.storage().persistent().remove(&DataKey::Approved(token_id)); }
        }
        Ok(())
    }

    pub fn set_approval_for_all(env: Env, caller: Address, operator: Address, approved: bool) -> Result<(), Error> {
        caller.require_auth();
        env.storage().persistent().set(&DataKey::ApprovalForAll(caller.clone(), operator.clone()), &approved);
        env.events().publish((symbol_short!("app_all"), caller.clone(), operator.clone()), ApprovalForAllEvent { owner: caller, operator, approved });
        Ok(())
    }

    pub fn get_approved(env: Env, token_id: TokenId) -> Option<Address> {
        env.storage().persistent().get(&DataKey::Approved(token_id))
    }

    pub fn is_approved_for_all(env: Env, owner: Address, operator: Address) -> bool {
        env.storage().persistent().get::<DataKey, bool>(&DataKey::ApprovalForAll(owner, operator)).unwrap_or(false)
    }

    // -------------------------------------------------------------------------
    // Task 2: Admin-only config — set_platform_fee, set_default_royalty
    // -------------------------------------------------------------------------

    /// Set platform fee in basis points. Emits ConfigUpdated.
    pub fn set_platform_fee(env: Env, admin: Address, bps: u32) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        if bps > 10_000 { return Err(Error::RoyaltyTooHigh); }
        env.storage().instance().set(&DataKey::PlatformFeeBps, &bps);
        env.events().publish((symbol_short!("cfg_upd"), symbol_short!("platform_fee")), ConfigUpdatedEvent { key: String::from_str(&env, "platform_fee"), new_value: bps as i128 });
        Ok(())
    }

    /// Set default royalty in basis points. Emits ConfigUpdated.
    pub fn set_default_royalty(env: Env, admin: Address, bps: u32) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        if bps > 10_000 { return Err(Error::RoyaltyTooHigh); }
        env.storage().instance().set(&DataKey::DefaultRoyaltyBps, &bps);
        env.events().publish((symbol_short!("cfg_upd"), symbol_short!("default_royalty")), ConfigUpdatedEvent { key: String::from_str(&env, "default_royalty"), new_value: bps as i128 });
        Ok(())
    }

    pub fn get_platform_fee(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::PlatformFeeBps).unwrap_or(100)
    }

    pub fn get_default_royalty_bps(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::DefaultRoyaltyBps).unwrap_or(0)
    }

    // -------------------------------------------------------------------------
    // Task 3: get_user_tokens — O(1) index-based pagination
    // -------------------------------------------------------------------------

    /// Return token IDs owned by `owner`. O(limit) — reads from OwnerTokenIndex.
    pub fn get_user_tokens(env: Env, owner: Address, limit: u32, offset: u32) -> Vec<TokenId> {
        const MAX_LIMIT: u32 = 100;
        let limit = if limit > MAX_LIMIT { MAX_LIMIT } else { limit };
        let balance: u32 = env.storage().persistent().get(&DataKey::Balance(owner.clone())).unwrap_or(0);
        let mut result: Vec<TokenId> = Vec::new(&env);
        let end = offset.saturating_add(limit).min(balance);
        for pos in offset..end {
            if let Some(tid) = env.storage().persistent().get::<DataKey, TokenId>(&DataKey::OwnerTokenIndex(owner.clone(), pos)) {
                result.push_back(tid);
            }
        }
        result
    }

    /// Alias with optional params for backwards compat.
    pub fn tokens_of_owner(env: Env, owner: Address, limit: Option<u32>, offset: Option<u32>) -> Vec<TokenId> {
        Self::get_user_tokens(env, owner, limit.unwrap_or(100), offset.unwrap_or(0))
    }

    // -------------------------------------------------------------------------
    // Task 4: Fee estimation helpers
    // -------------------------------------------------------------------------

    /// Returns approximate mint fee in stroops.
    pub fn estimate_mint_fee(_env: Env) -> i128 {
        GAS_BASE_MINT as i128
    }

    /// Returns approximate transfer fee in stroops.
    pub fn estimate_transfer_fee(_env: Env) -> i128 {
        GAS_BASE_TRANSFER as i128
    }

    // -------------------------------------------------------------------------
    // Royalty
    // -------------------------------------------------------------------------

    pub fn royalty_info(env: Env, token_id: TokenId, sale_price: i128) -> Result<RoyaltyInfo, Error> {
        if sale_price <= 0 { return Err(Error::InvalidSalePrice); }
        let data = Self::load_token(&env, token_id)?;
        let mut total_bps: u32 = 0;
        for i in 0..data.royalty.recipients.len() {
            let r = data.royalty.recipients.get(i).ok_or(Error::InvalidRoyaltySplit)?;
            total_bps = total_bps.saturating_add(r.basis_points);
        }
        let royalty_amount = Self::calculate_royalty(sale_price, total_bps)?;
        let first = data.royalty.recipients.get(0).ok_or(Error::InvalidRoyaltySplit)?;
        Ok(RoyaltyInfo { receiver: first.recipient, royalty_amount, asset_address: data.royalty.asset_address })
    }

    pub fn get_royalty(env: Env, token_id: TokenId) -> Result<Royalty, Error> {
        Ok(Self::load_token(&env, token_id)?.royalty)
    }

    pub fn set_royalty(env: Env, admin: Address, token_id: TokenId, new_royalty: Royalty) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        let mut data = Self::load_token(&env, token_id)?;
        let old = data.royalty.clone();
        if !old.recipients.is_empty() && !new_royalty.recipients.is_empty() {
            let or = old.recipients.get(0).ok_or(Error::InvalidRoyaltySplit)?;
            let nr = new_royalty.recipients.get(0).ok_or(Error::InvalidRoyaltySplit)?;
            if or.recipient != nr.recipient {
                env.events().publish((symbol_short!("royalty"), token_id, or.recipient.clone(), nr.recipient.clone()), RoyaltyRecipientUpdatedEvent { token_id, old_recipient: or.recipient, new_recipient: nr.recipient });
            }
        }
        let new_royalty = Self::normalize_royalty(&env, new_royalty)?;
        data.royalty = new_royalty;
        env.storage().persistent().set(&DataKey::Token(token_id), &data);
        env.events().publish((symbol_short!("roy_upd"), token_id), RoyaltyUpdatedEvent { token_id });
        Ok(())
    }

    pub fn update_royalty_recipient(env: Env, caller: Address, token_id: TokenId, new_recipient: Address) -> Result<(), Error> {
        caller.require_auth();
        let mut data = Self::load_token(&env, token_id)?;
        let old = data.royalty.recipients.get(0).ok_or(Error::InvalidRoyaltySplit)?.recipient.clone();
        if caller != old { return Err(Error::Unauthorized); }
        let bps = data.royalty.recipients.get(0).ok_or(Error::InvalidRoyaltySplit)?.basis_points;
        data.royalty.recipients.set(0, RoyaltyRecipient { recipient: new_recipient.clone(), basis_points: bps });
        env.storage().persistent().set(&DataKey::Token(token_id), &data);
        env.events().publish((symbol_short!("royalty"), token_id, old.clone(), new_recipient.clone()), RoyaltyRecipientUpdatedEvent { token_id, old_recipient: old, new_recipient });
        Ok(())
    }

    pub fn pay_royalty(env: Env, payer: Address, token_id: TokenId, sale_price: i128) -> Result<(), Error> {
        payer.require_auth();
        if sale_price <= 0 { return Err(Error::InvalidSalePrice); }
        let data = Self::load_token(&env, token_id)?;
        let asset = data.royalty.asset_address.clone().ok_or(Error::InvalidRecipient)?;
        Self::acquire_reentrancy_lock(&env)?;
        let client = soroban_sdk::token::TokenClient::new(&env, &asset);
        let mut cum_bps: u32 = 0;
        let mut cum_amt: i128 = 0;
        for i in 0..data.royalty.recipients.len() {
            let split = data.royalty.recipients.get(i).ok_or(Error::InvalidRoyaltySplit)?;
            cum_bps = cum_bps.saturating_add(split.basis_points);
            let total = Self::calculate_royalty(sale_price, cum_bps)?;
            let amount = total.saturating_sub(cum_amt);
            cum_amt = total;
            if amount > 0 {
                client.transfer(&payer, &split.recipient, &amount);
                Self::emit_royalty_paid_event(
                    &env,
                    token_id,
                    &payer,
                    &split.recipient,
                    amount,
                );
            }
        }
        Self::release_reentrancy_lock(&env);
        Ok(())
    }

    pub fn claim_royalties(env: Env, caller: Address, token_id: TokenId) -> Result<(), Error> {
        caller.require_auth();
        let data = Self::load_token(&env, token_id)?;
        let recipient = data.royalty.recipients.get(0).ok_or(Error::InvalidRoyaltySplit)?.recipient;
        if caller != recipient { return Err(Error::Unauthorized); }
        let asset = data.royalty.asset_address.ok_or(Error::InvalidRecipient)?;
        let balance: i128 = env.storage().persistent().get(&DataKey::RoyaltyBalance(token_id)).unwrap_or(0);
        if balance <= 0 { return Err(Error::InsufficientBalance); }
        Self::acquire_reentrancy_lock(&env)?;
        env.storage().persistent().remove(&DataKey::RoyaltyBalance(token_id));
        soroban_sdk::token::TokenClient::new(&env, &asset).transfer(&env.current_contract_address(), &recipient, &balance);
        Self::release_reentrancy_lock(&env);
        env.events().publish((symbol_short!("roy_clm"), token_id, recipient.clone()), RoyaltyClaimedEvent { token_id, recipient, amount: balance, asset });
        Ok(())
    }

    /// Task 1: public safe-math royalty calculation helper
    pub fn calculate_royalty_amount(env: Env, token_id: TokenId, sale_price: i128) -> Result<i128, Error> {
        let data = Self::load_token(&env, token_id)?;
        let mut total_bps: u32 = 0;
        for i in 0..data.royalty.recipients.len() {
            let r = data.royalty.recipients.get(i).ok_or(Error::InvalidRoyaltySplit)?;
            total_bps = total_bps.saturating_add(r.basis_points);
        }
        Self::calculate_royalty(sale_price, total_bps)
    }

    // -------------------------------------------------------------------------
    // Pause / Admin
    // -------------------------------------------------------------------------

    /// Pause the contract with an optional human-readable reason.
    ///
    /// The pause takes effect after a 24-hour timelock. Callers can query the
    /// stored reason via [`pause_reason`] until the contract is unpaused.
    pub fn pause(env: Env, admin: Address, reason: Option<String>) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        let active_at = env.ledger().timestamp().saturating_add(86_400);
        env.storage().instance().set(&DataKey::PauseUnlockTime, &active_at);
        env.storage().instance().set(&DataKey::Paused, &true);
        match reason.clone() {
            Some(ref r) => env.storage().instance().set(&DataKey::PauseReason, r),
            None => env.storage().instance().remove(&DataKey::PauseReason),
        }
        env.events().publish(
            (symbol_short!("pse_sched"), symbol_short!("pause")),
            PauseWithReasonEvent { active_at, reason },
        );
        Ok(())
    }

    /// Returns the reason stored when the contract was last paused, if any.
    pub fn pause_reason(env: Env) -> Option<String> {
        env.storage().instance().get(&DataKey::PauseReason)
    }

    pub fn unpause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().remove(&DataKey::PauseUnlockTime);
        env.storage().instance().remove(&DataKey::PauseReason);
        env.events().publish((symbol_short!("unpaused"), admin.clone()), UnpausedEvent {});
        Ok(())
    }

    pub fn is_paused(env: Env) -> bool {
        Self::check_paused(&env)
    }

    pub fn pause_minting(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::MintingPaused, &true);
        env.events().publish((symbol_short!("pause_mint"), admin.clone()), PauseMintingEvent {});
        Ok(())
    }

    pub fn unpause_minting(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::MintingPaused, &false);
        env.events().publish((symbol_short!("unpause_mint"), admin.clone()), UnpauseMintingEvent {});
        Ok(())
    }

    pub fn set_signer(env: Env, admin: Address, pubkey: BytesN<32>) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Signer, &pubkey);
        env.events().publish((symbol_short!("sgn_upd"), pubkey.clone()), SignerUpdatedEvent { new_pubkey: pubkey });
        Ok(())
    }

    pub fn get_signer(env: Env) -> Option<BytesN<32>> {
        env.storage().instance().get(&DataKey::Signer)
    }

    pub fn set_backend_address(env: Env, admin: Address, backend_address: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::BackendAddress, &backend_address);
        env.events().publish((symbol_short!("backend_upd"), backend_address.clone()), BackendAddressUpdatedEvent { new_backend_address: backend_address });
        Ok(())
    }

    pub fn get_backend_address(env: Env) -> Option<Address> {
        env.storage().instance().get(&DataKey::BackendAddress)
    }

    pub fn name(env: Env) -> String {
        env.storage().instance().get(&DataKey::Name).unwrap_or_else(|| String::from_str(&env, "ClipCash Clips"))
    }

    pub fn symbol(env: Env) -> String {
        env.storage().instance().get(&DataKey::Symbol).unwrap_or_else(|| String::from_str(&env, "CLIP"))
    }

    pub fn set_name(env: Env, admin: Address, name: String) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Name, &name);
        Ok(())
    }

    pub fn set_symbol(env: Env, admin: Address, symbol: String) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Symbol, &symbol);
        Ok(())
    }

    pub fn set_platform_recipient(env: Env, admin: Address, recipient: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::PlatformRecipient, &recipient);
        env.events().publish((symbol_short!("platform_recipient_upd"), recipient.clone()), PlatformRecipientUpdatedEvent { new_recipient: recipient });
        Ok(())
    }

    pub fn set_default_royalty_asset(env: Env, admin: Address, asset_address: Option<Address>) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::DefaultRoyaltyAsset, &asset_address);
        env.events().publish((symbol_short!("default_royalty_asset_upd"),), DefaultRoyaltyAssetUpdatedEvent { asset_address });
        Ok(())
    }

    pub fn get_default_royalty_asset(env: Env) -> Option<Address> {
        env.storage().instance().get::<DataKey, Option<Address>>(&DataKey::DefaultRoyaltyAsset).unwrap_or(None)
    }

    pub fn set_mint_cooldown(env: Env, admin: Address, seconds: u64) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::MintCooldownSeconds, &seconds);
        env.events().publish((symbol_short!("cfg_upd"), symbol_short!("mint_cooldown")), ConfigUpdatedEvent { key: String::from_str(&env, "mint_cooldown"), new_value: seconds as i128 });
        Ok(())
    }

    pub fn get_mint_cooldown(env: Env) -> u64 {
        env.storage().instance().get(&DataKey::MintCooldownSeconds).unwrap_or(DEFAULT_MINT_COOLDOWN_SECONDS)
    }

    // Task 2: alias for set_mint_cooldown
    pub fn set_mint_cooldown_seconds(env: Env, admin: Address, seconds: u64) -> Result<(), Error> {
        Self::set_mint_cooldown(env, admin, seconds)
    }

    // Task 4: revoke a single-token approval
    pub fn revoke_approval(env: Env, caller: Address, token_id: TokenId) -> Result<(), Error> {
        caller.require_auth();
        let data: TokenData = Self::load_token(&env, token_id)?;
        if data.owner != caller {
            return Err(Error::Unauthorized);
        }
        env.storage().persistent().remove(&DataKey::Approved(token_id));
        Ok(())
    }

    // Task 4: revoke all operator approvals for the caller
    pub fn revoke_all_approvals(env: Env, caller: Address, operator: Address) -> Result<(), Error> {
        caller.require_auth();
        env.storage()
            .persistent()
            .remove(&DataKey::ApprovalForAll(caller.clone(), operator.clone()));
        env.events().publish(
            (symbol_short!("app_all"), caller.clone(), operator.clone()),
            ApprovalForAllEvent { owner: caller, operator, approved: false },
        );
        Ok(())
    }

    pub fn blacklist_clip(env: Env, admin: Address, clip_id: u32) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().persistent().set(&DataKey::BlacklistedClip(clip_id), &true);
        env.events().publish((symbol_short!("blacklist"), clip_id), BlacklistEvent { clip_id });
        Ok(())
    }

    pub fn freeze_token(env: Env, admin: Address, token_id: TokenId) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        if !env.storage().persistent().has(&DataKey::Token(token_id)) { return Err(Error::InvalidTokenId); }
        env.storage().persistent().set(&DataKey::Frozen(token_id), &true);
        env.events().publish((symbol_short!("freeze"), token_id), TokenFrozenEvent { token_id });
        Ok(())
    }

    /// Lift a freeze on a token, re-enabling transfers.
    ///
    /// Only the admin may unfreeze. No-op if the token is not currently frozen.
    ///
    /// Emits: `"unfreeze"` [`TokenUnfrozenEvent`].
    pub fn unfreeze_token(env: Env, admin: Address, token_id: TokenId) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        if !env.storage().persistent().has(&DataKey::Token(token_id)) {
            return Err(Error::InvalidTokenId);
        }
        env.storage().persistent().remove(&DataKey::Frozen(token_id));
        env.events().publish((symbol_short!("unfreeze"), token_id), TokenUnfrozenEvent { token_id });
        Ok(())
    }

    /// Mint using a backend-provided signature instead of a wallet-signed tx.
    ///
    /// This entrypoint still requires the recipient to authorize the mint.
    /// The backend signature must include a nonce to prevent replay attacks.
    pub fn mint_with_signature(
        env: Env,
        to: Address,
        clip_id: u32,
        metadata_uri: String,
        image: Option<String>,
        animation_url: Option<String>,
        royalty: Royalty,
        is_soulbound: bool,
    ) -> Result<TokenId, Error> {
        Self::require_clip_owner(&to);
        Self::mint_core(
            &env,
            &to,
            clip_id,
            metadata_uri,
            image,
            animation_url,
            royalty,
            is_soulbound,
        )
    }

    /// Internal mint implementation shared by public mint entrypoints.
    fn mint_core(
        env: &Env,
        to: &Address,
        clip_id: u32,
        metadata_uri: String,
        image: Option<String>,
        animation_url: Option<String>,
        royalty: Royalty,
        is_soulbound: bool,
    ) -> Result<TokenId, Error> {
        if Self::is_clip_minted(env, clip_id) {
            return Err(Error::ClipAlreadyMinted);
        }

        if env
            .storage()
            .persistent()
            .get(&DataKey::BlacklistedClip(clip_id))
            .unwrap_or(false)
        {
            return Err(Error::ClipBlacklisted);
        }

        let royalty = Self::normalize_royalty(env, royalty)?;

        let token_id: TokenId = env
            .storage()
            .instance()
            .get(&DataKey::NextTokenId)
            .unwrap_or(1);

        env.storage().persistent().set(
            &DataKey::Token(token_id),
            &TokenData {
                owner: to.clone(),
                clip_id,
                is_soulbound,
                metadata_uri: metadata_uri.clone(),
                image: image.clone(),
                animation_url: animation_url.clone(),
                description: None,
                external_url: None,
                attributes: Vec::new(env),
                royalty,
            },
        );
        Self::bump_persistent_ttl(env, &DataKey::Token(token_id));
        Self::mark_clip_minted(env, clip_id, token_id);

        env.storage()
            .instance()
            .set(&DataKey::NextTokenId, &(token_id + 1));

        let total_supply: u32 = env.storage().instance().get(&DataKey::TotalSupply).unwrap_or(0);
        env.storage().instance().set(&DataKey::TotalSupply, &(total_supply + 1));

        let balance: u32 = env.storage().persistent().get(&DataKey::Balance(to.clone())).unwrap_or(0);
        env.storage().persistent().set(&DataKey::Balance(to.clone()), &(balance + 1));

        let mint_from = env.current_contract_address();
        Self::emit_mint_event(env, token_id, &mint_from, to, clip_id);
        Self::emit_transfer_event(env, token_id, &mint_from, to);

        let count_mint: u64 = env.storage().instance().get(&DataKey::CountMint).unwrap_or(0);
        env.storage().instance().set(&DataKey::CountMint, &(count_mint + 1));
        let total_gas_mint: u64 = env.storage().instance().get(&DataKey::TotalGasMint).unwrap_or(0);
        env.storage().instance().set(&DataKey::TotalGasMint, &total_gas_mint.saturating_add(GAS_BASE_MINT));
        Self::record_mint_timestamp(env, &to);
        Self::update_circuit_breaker_counter(env, 1);

        Ok(token_id)
    }

    /// Mint a new NFT for a video clip using a backend-signed payload and a
    /// strictly increasing nonce to prevent replay attacks.
    ///
    /// # Arguments
    /// * `to` — Recipient address (must authorize the call).
    /// * `clip_id` — Off-chain clip identifier.
    /// * `metadata_uri` — Metadata URI (IPFS or Arweave).
    /// * `image` — Static thumbnail URL (optional).
    /// * `animation_url` — Animated preview URL (optional).
    /// * `royalty` — Royalty configuration for secondary sales.
    /// * `is_soulbound` — When `true` the token cannot be transferred.
    /// * `nonce` — Strictly increasing backend nonce for this recipient.
    /// * `signature` — 64-byte Ed25519 signature from the registered backend signer.
    pub fn mint_with_signature(
        env: Env,
        to: Address,
        clip_id: u32,
        metadata_uri: String,
        image: Option<String>,
        animation_url: Option<String>,
        royalty: Royalty,
        is_soulbound: bool,
        nonce: u64,
        signature: BytesN<64>,
    ) -> Result<TokenId, Error> {
        Self::require_clip_owner(&to);
        Self::require_not_paused(&env)?;
        Self::enforce_mint_cooldown(&env, &to)?;
        Self::check_circuit_breaker(&env, 1)?;

        Self::validate_url(&env, &image, Error::InvalidImageUrl)?;
        Self::validate_url(&env, &animation_url, Error::InvalidAnimationUrl)?;

        Self::only_clip_owner_with_nonce(&env, &to, clip_id, &metadata_uri, nonce, &signature)?;
        Self::ensure_mint_nonce(&env, &to, nonce)?;

        let token_id = Self::mint_core(
            &env,
            &to,
            clip_id,
            metadata_uri,
            image,
            animation_url,
            royalty,
            is_soulbound,
        )?;
        Self::record_mint_nonce(&env, &to, nonce);

        Ok(token_id)
    }

    // -------------------------------------------------------------------------
    // Approvals
    // -------------------------------------------------------------------------

    pub fn is_frozen(env: Env, token_id: TokenId) -> bool {
        env.storage().persistent().get::<DataKey, bool>(&DataKey::Frozen(token_id)).unwrap_or(false)
    }

    pub fn set_token_uri(env: Env, owner: Address, token_id: TokenId, new_uri: String) -> Result<(), Error> {
        owner.require_auth();
        let data = Self::load_token(&env, token_id)?;
        if data.owner != owner { return Err(Error::Unauthorized); }
        env.storage().persistent().set(&DataKey::CustomTokenUri(token_id), &new_uri.clone());
        env.events().publish((symbol_short!("uri_chg"), token_id, owner.clone()), TokenUriChangedEvent { token_id, owner, new_uri });
        Ok(())
    }

    pub fn request_withdraw_asset(env: Env, admin: Address, amount: i128) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        let unlock_time = env.ledger().timestamp().saturating_add(172_800);
        env.storage().instance().set(&DataKey::WithdrawXlmRequest, &WithdrawRequest { amount, unlock_time });
        env.events().publish((symbol_short!("wdraw_req"), admin.clone()), WithdrawRequestedEvent { amount, unlock_time });
        Ok(())
    }

    pub fn withdraw_asset(env: Env, admin: Address, asset: Address, amount: i128) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        let req: WithdrawRequest = env.storage().instance().get(&DataKey::WithdrawXlmRequest).ok_or(Error::NoWithdrawalRequest)?;
        if env.ledger().timestamp() < req.unlock_time { return Err(Error::WithdrawalStillLocked); }
        env.storage().instance().remove(&DataKey::WithdrawXlmRequest);
        env.storage().instance().set(&DataKey::LastWithdrawalTime, &env.ledger().timestamp());
        soroban_sdk::token::TokenClient::new(&env, &asset).transfer(&env.current_contract_address(), &admin, &amount);
        env.events().publish((symbol_short!("wdraw_exe"), admin.clone()), WithdrawExecutedEvent { amount, recipient: admin });
        Ok(())
    }

    pub fn withdraw_xlm(env: Env, admin: Address, xlm_address: Address, amount: i128) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        if let Some(req) = env.storage().instance().get::<_, WithdrawRequest>(&DataKey::WithdrawXlmRequest) {
            if req.amount == amount {
                if env.ledger().timestamp() < req.unlock_time {
                    return Err(Error::WithdrawalStillLocked);
                }
                env.storage().instance().remove(&DataKey::WithdrawXlmRequest);
                env.storage().instance().set(&DataKey::LastWithdrawalTime, &env.ledger().timestamp());
                soroban_sdk::token::TokenClient::new(&env, &xlm_address).transfer(&env.current_contract_address(), &admin, &amount);
                env.events().publish((symbol_short!("wdraw_xlm"), admin.clone()), WithdrawExecutedEvent { amount, recipient: admin });
                return Ok(());
            }
        }
        let unlock_time = env.ledger().timestamp().saturating_add(86_400);
        env.storage().instance().set(&DataKey::WithdrawXlmRequest, &WithdrawRequest { amount, unlock_time });
        env.events().publish((symbol_short!("wreq_xlm"), admin.clone()), WithdrawRequestedEvent { amount, unlock_time });
        Ok(())
    }

    pub fn set_circuit_breaker_enabled(env: Env, admin: Address, enabled: bool) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::CircuitBreakerEnabled, &enabled);
        env.events().publish((symbol_short!("cfg_upd"), symbol_short!("circuit_enabled")), ConfigUpdatedEvent { key: String::from_str(&env, "circuit_enabled"), new_value: if enabled { 1 } else { 0 } });
        Ok(())
    }

    pub fn set_circuit_breaker_threshold(env: Env, admin: Address, threshold: u64) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::CircuitBreakerThreshold, &threshold);
        env.events().publish((symbol_short!("cfg_upd"), symbol_short!("circuit_threshold")), ConfigUpdatedEvent { key: String::from_str(&env, "circuit_threshold"), new_value: threshold as i128 });
        Ok(())
    }

    pub fn set_circuit_breaker_window(env: Env, admin: Address, window_seconds: u64) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::CircuitBreakerWindowSeconds, &window_seconds);
        env.events().publish((symbol_short!("cfg_upd"), symbol_short!("circuit_window")), ConfigUpdatedEvent { key: String::from_str(&env, "circuit_window"), new_value: window_seconds as i128 });
        Ok(())
    }

    pub fn reset_circuit_breaker(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::CircuitBreakerWindowStart, &0u64);
        env.storage().instance().set(&DataKey::CircuitBreakerWindowCount, &0u64);
        env.events().publish((symbol_short!("circuit_reset"), admin.clone()), CircuitBreakerResetEvent {});
        Ok(())
    }

    // -------------------------------------------------------------------------
    // Batch mint
    // -------------------------------------------------------------------------

    pub fn batch_mint(
        env: Env,
        to: Address,
        clip_ids: Vec<u32>,
        metadata_uris: Vec<String>,
        images: Vec<Option<String>>,
        animation_urls: Vec<Option<String>>,
        royalty: Royalty,
        is_soulbound: bool,
        signatures: Vec<BytesN<64>>,
    ) -> Result<Vec<TokenId>, Error> {
        Self::require_clip_owner(&to);
        Self::require_not_paused(&env)?;
        Self::enforce_mint_cooldown(&env, &to)?;
        let n = clip_ids.len();
        if n > MAX_BATCH_MINT { return Err(Error::BatchTooLarge); }
        Self::check_circuit_breaker(&env, n as u64)?;
        if n != metadata_uris.len() || n != signatures.len() || n != images.len() || n != animation_urls.len() {
            return Err(Error::InvalidRoyaltySplit);
        }
        let royalty = Self::normalize_royalty(&env, royalty)?;
        let mut minted: Vec<TokenId> = Vec::new(&env);
        for i in 0..n {
            let clip_id = clip_ids.get(i).ok_or(Error::InvalidTokenId)?;
            let metadata_uri = metadata_uris.get(i).ok_or(Error::InvalidTokenId)?;
            let image = images.get(i).ok_or(Error::InvalidTokenId)?;
            let animation_url = animation_urls.get(i).ok_or(Error::InvalidTokenId)?;
            let signature = signatures.get(i).ok_or(Error::InvalidTokenId)?;
            Self::validate_url(&env, &image)?;
            Self::validate_url(&env, &animation_url)?;
            Self::only_clip_owner(&env, &to, clip_id, &metadata_uri, &signature)?;
            if Self::is_clip_minted(&env, clip_id) { return Err(Error::ClipAlreadyMinted); }
            if env.storage().persistent().get::<DataKey, bool>(&DataKey::BlacklistedClip(clip_id)).unwrap_or(false) { return Err(Error::ClipBlacklisted); }
            let token_id: TokenId = env.storage().instance().get(&DataKey::NextTokenId).unwrap_or(1);
            env.storage().instance().set(&DataKey::NextTokenId, &(token_id + 1));
            env.storage().persistent().set(&DataKey::Token(token_id), &TokenData {
                owner: to.clone(), clip_id, is_soulbound, metadata_uri, image, animation_url,
                description: None, external_url: None, attributes: Vec::new(&env), royalty: royalty.clone(), is_locked: false,
            });
            Self::bump_persistent_ttl(&env, &DataKey::Token(token_id));
            Self::mark_clip_minted(&env, clip_id, token_id);
            let supply: u32 = env.storage().instance().get(&DataKey::TotalSupply).unwrap_or(0);
            env.storage().instance().set(&DataKey::TotalSupply, &(supply + 1));
            env.storage().persistent().set(&DataKey::TokenIndex(supply), &token_id);
            let bal: u32 = env.storage().persistent().get(&DataKey::Balance(to.clone())).unwrap_or(0);
            env.storage().persistent().set(&DataKey::Balance(to.clone()), &(bal + 1));
            env.storage().persistent().set(&DataKey::OwnerTokenIndex(to.clone(), bal), &token_id);
            let mint_from = env.current_contract_address();
            Self::emit_mint_event(&env, token_id, &mint_from, &to, clip_id);
            Self::emit_transfer_event(&env, token_id, &mint_from, &to);
            minted.push_back(token_id);
        }
        Self::record_mint_timestamp(&env, &to);
        Self::update_circuit_breaker_counter(&env, n as u64);
        let first = minted.get(0).unwrap_or(0);
        env.events().publish((symbol_short!("batch_mnt"), first), BatchMintEvent { to, count: n, first_token_id: first });
        Ok(minted)
    }

    // -------------------------------------------------------------------------
    // View functions
    // -------------------------------------------------------------------------

    pub fn owner_of(env: Env, token_id: TokenId) -> Result<Address, Error> {
        Ok(Self::load_token(&env, token_id)?.owner)
    }

    /// Returns the metadata URI for a given token ID.
    ///
    /// # Errors
    /// * [`Error::InvalidTokenId`] — token does not exist.
    pub fn token_uri(env: Env, token_id: TokenId) -> Result<String, Error> {
        // Return custom URI override if set, otherwise fall back to TokenData.
        if let Some(uri) = env
            .storage()
            .persistent()
            .get::<DataKey, String>(&DataKey::CustomTokenUri(token_id))
        {
            // Verify token exists.
            if !env.storage().persistent().has(&DataKey::Token(token_id)) {
                return Err(Error::InvalidTokenId);
            }
            return Ok(uri);
        }
        Ok(Self::load_token(&env, token_id)?.metadata_uri)
    }

    /// Alias for [`token_uri`], kept for backwards compatibility.
    pub fn get_metadata(env: Env, token_id: TokenId) -> Result<String, Error> {
        Self::token_uri(env, token_id)
    }

    /// Returns OpenSea-compatible JSON metadata for a given token ID.
    ///
    /// Serializes the token's metadata following the OpenSea metadata standard:
    /// https://docs.opensea.io/docs/metadata-standards
    ///
    /// The JSON output includes:
    /// - "metadata_uri": The base metadata URI
    /// - "image": Static thumbnail URL (only if set)
    /// - "animation_url": Animated preview URL (only if set)
    ///
    /// # Errors
    /// * [`Error::InvalidTokenId`] — token does not exist.
    pub fn get_metadata_json(env: Env, token_id: TokenId) -> Result<String, Error> {
        let data = Self::load_token(&env, token_id)?;

        let mut json = Bytes::from_slice(&env, b"{\"metadata_uri\":\"");
        Self::append_string_bytes(&env, &mut json, &data.metadata_uri);
        Self::append_literal_bytes(&env, &mut json, b"\"");

        if let Some(ref img) = data.image {
            Self::append_literal_bytes(&env, &mut json, b",\"image\":\"");
            Self::append_string_bytes(&env, &mut json, img);
            Self::append_literal_bytes(&env, &mut json, b"\"");
        }

        if let Some(ref anim) = data.animation_url {
            Self::append_literal_bytes(&env, &mut json, b",\"animation_url\":\"");
            Self::append_string_bytes(&env, &mut json, anim);
            Self::append_literal_bytes(&env, &mut json, b"\"");
        }

        if let Some(ref desc) = data.description {
            Self::append_literal_bytes(&env, &mut json, b",\"description\":\"");
            Self::append_string_bytes(&env, &mut json, desc);
            Self::append_literal_bytes(&env, &mut json, b"\"");
        }

        if let Some(ref url) = data.external_url {
            Self::append_literal_bytes(&env, &mut json, b",\"external_url\":\"");
            Self::append_string_bytes(&env, &mut json, url);
            Self::append_literal_bytes(&env, &mut json, b"\"");
        }

        Self::append_literal_bytes(&env, &mut json, b",\"attributes\":[");
        for i in 0..data.attributes.len() {
            if i > 0 {
                Self::append_literal_bytes(&env, &mut json, b",");
            }
            let attribute = data.attributes.get(i).ok_or(Error::InvalidTokenId)?;
            Self::append_literal_bytes(&env, &mut json, b"{\"trait_type\":\"");
            Self::append_string_bytes(&env, &mut json, &attribute.trait_type);
            Self::append_literal_bytes(&env, &mut json, b"\",\"value\":\"");
            Self::append_string_bytes(&env, &mut json, &attribute.value);
            Self::append_literal_bytes(&env, &mut json, b"\"}");
        }
        Self::append_literal_bytes(&env, &mut json, b"]}");
        Ok(json.to_string())
    }

    /// Look up the on-chain token ID for a given `clip_id`.
    ///
    /// # Errors
    /// * [`Error::InvalidTokenId`] — no token exists for this clip.
    pub fn clip_token_id(env: Env, clip_id: u32) -> Result<TokenId, Error> {
        Self::load_clip_token_id(&env, clip_id).ok_or(Error::InvalidTokenId)
    }

    /// Look up the on-chain token IDs for multiple clip IDs.
    /// Returns None for clip IDs that have not been minted.
    pub fn get_tokens_by_clip_ids(env: Env, clip_ids: Vec<u32>) -> Vec<Option<TokenId>> {
        let mut result = Vec::new(&env);
        for i in 0..clip_ids.len() {
            let clip_id = clip_ids.get(i).unwrap();
            result.push_back(Self::load_clip_token_id(&env, clip_id));
        }
        result
    }

    /// Returns the stored [`Royalty`] struct for a token.
    ///
    /// # Errors
    /// * [`Error::InvalidTokenId`] — token does not exist.
    pub fn get_royalty(env: Env, token_id: TokenId) -> Result<Royalty, Error> {
        Ok(Self::load_token(&env, token_id)?.royalty)
    }

    /// Returns the total number of tokens currently in circulation.
    ///
    /// Maintains a separate counter that is incremented on mint and decremented on burn,
    /// ensuring accurate supply accounting across the contract lifetime.
    pub fn total_supply(env: Env) -> u32 {
        env.storage()
            .instance()
            .get(&DataKey::TotalSupply)
            .unwrap_or(0)
    }

    /// Returns the total number of clips minted so far (all-time).
    pub fn minted_count(env: Env) -> u32 {
        env.storage()
            .instance()
            .get::<DataKey, u32>(&DataKey::NextTokenId)
            .unwrap_or(1)
            .saturating_sub(1)
    }

    /// Returns `true` if the token is soulbound (non-transferable).
    pub fn is_soulbound(env: Env, token_id: TokenId) -> bool {
        Self::load_token(&env, token_id)
            .map(|d| d.is_soulbound)
            .unwrap_or(false)
    }

    /// Returns the accrued royalty balance for `token_id` (in asset smallest units).
    /// Returns `0` if no balance is recorded or the token does not exist.
    pub fn royalty_balance_of(env: Env, token_id: TokenId) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::RoyaltyBalance(token_id))
            .unwrap_or(0)
    }

    /// Returns the average gas cost for mint operations.
    /// Returns 0 if no mints have been performed.
    pub fn average_gas_mint(env: Env) -> u64 {
        let total_gas: u64 = env
            .storage()
            .temporary()
            .get(&DataKey::TotalGasMint)
            .unwrap_or(0);
        let count: u64 = env
            .storage()
            .temporary()
            .get(&DataKey::CountMint)
            .unwrap_or(0);

        if count == 0 {
            0
        } else {
            total_gas / count
        }
    }

    /// Returns the average gas cost for transfer operations.
    /// Returns 0 if no transfers have been performed.
    pub fn average_gas_transfer(env: Env) -> u64 {
        let total_gas: u64 = env
            .storage()
            .temporary()
            .get(&DataKey::TotalGasTransfer)
            .unwrap_or(0);
        let count: u64 = env
            .storage()
            .temporary()
            .get(&DataKey::CountTransfer)
            .unwrap_or(0);

        if count == 0 {
            0
        } else {
            total_gas / count
        }
    }

    /// Returns the total number of mint operations performed.
    pub fn total_mints(env: Env) -> u64 {
        env.storage()
            .temporary()
            .get(&DataKey::CountMint)
            .unwrap_or(0)
    }

    /// Returns the total number of transfer operations performed.
    pub fn total_transfers(env: Env) -> u64 {
        env.storage()
            .temporary()
            .get(&DataKey::CountTransfer)
            .unwrap_or(0)
    }

    /// Returns whether the circuit breaker is enabled.
    pub fn circuit_breaker_enabled(env: Env) -> bool {
        env.storage()
            .instance()
            .get(&DataKey::CircuitBreakerEnabled)
            .unwrap_or(DEFAULT_CIRCUIT_BREAKER_ENABLED)
    }

    /// Returns the circuit breaker threshold (max mints per window).
    pub fn circuit_breaker_threshold(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::CircuitBreakerThreshold)
            .unwrap_or(DEFAULT_CIRCUIT_BREAKER_THRESHOLD)
    }

    /// Returns the circuit breaker time window duration in seconds.
    pub fn circuit_breaker_window_seconds(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::CircuitBreakerWindowSeconds)
            .unwrap_or(DEFAULT_CIRCUIT_BREAKER_WINDOW_SECONDS)
    }

    /// Returns the current circuit breaker window start timestamp.
    pub fn circuit_breaker_window_start(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::CircuitBreakerWindowStart)
            .unwrap_or(0)
    }

    /// Returns the current circuit breaker window mint count.
    pub fn circuit_breaker_window_count(env: Env) -> u64 {
        env.storage()
            .instance()
            .get(&DataKey::CircuitBreakerWindowCount)
            .unwrap_or(0)
    }

    /// Returns the number of tokens owned by `owner`.
    /// Compliant with emerging Soroban NFT standard view functions.
    pub fn balance_of(env: Env, owner: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(owner))
            .unwrap_or(0)
    }

    /// Returns the token ID at the given global index (0-based).
    ///
    /// O(1) — reads directly from the `TokenIndex` persistent map maintained
    /// by mint and burn. No iteration over burned slots.
    ///
    /// # Errors
    /// * [`Error::InvalidTokenId`] — `index` ≥ `total_supply`.
    pub fn token_by_index(env: Env, index: u32) -> Result<TokenId, Error> {
        let supply: u32 = env.storage().instance().get(&DataKey::TotalSupply).unwrap_or(0);
        if index >= supply {
            return Err(Error::InvalidTokenId);
        }
        env.storage()
            .persistent()
            .get(&DataKey::TokenIndex(index))
            .ok_or(Error::InvalidTokenId)
    }

    /// Returns the N-th token owned by `owner` (0-indexed).
    ///
    /// O(1) — reads directly from the `OwnerTokenIndex` persistent map
    /// maintained by mint, burn, and transfer. No iteration required.
    ///
    /// # Arguments
    /// * `owner` — Address to query.
    /// * `index` — 0-based position among the owner's tokens.
    ///
    /// # Errors
    /// * [`Error::InvalidTokenId`] — `index` ≥ `balance_of(owner)`.
    pub fn token_of_owner_by_index(env: Env, owner: Address, index: u32) -> Result<TokenId, Error> {
        let balance: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(owner.clone()))
            .unwrap_or(0);
        if index >= balance {
            return Err(Error::InvalidTokenId);
        }
        env.storage()
            .persistent()
            .get(&DataKey::OwnerTokenIndex(owner, index))
            .ok_or(Error::InvalidTokenId)
    }

    /// Returns the earliest ledger timestamp at which `token_id` is eligible
    /// for its next metadata refresh (i.e. `last_refresh + 30 days`).
    ///
    /// Returns `0` if the token has never been refreshed (eligible immediately).
    ///
    /// # Arguments
    /// * `token_id` — Token to query.
    ///
    /// # Errors
    /// * [`Error::InvalidTokenId`] — token does not exist.
    ///
    /// Closes #172
    pub fn get_next_metadata_refresh_time(env: Env, token_id: TokenId) -> Result<u64, Error> {
        if !Self::exists(env.clone(), token_id) {
            return Err(Error::InvalidTokenId);
        }
        const COOLDOWN: u64 = 2_592_000; // 30 days in seconds
        let next_time = env
            .storage()
            .persistent()
            .get::<DataKey, u64>(&DataKey::MetadataRefreshTime(token_id))
            .map(|last| last.saturating_add(COOLDOWN))
            .unwrap_or(0);
        Ok(next_time)
    }

    // -------------------------------------------------------------------------
    // Royalty extension (EIP-2981 style)
    // -------------------------------------------------------------------------

    /// Returns the royalty receiver, total amount, and payment asset for a sale.
    ///
    /// Formula: `royalty_amount = sale_price × total_basis_points / 10_000`
    ///
    /// Uses overflow-safe arithmetic; returns [`Error::RoyaltyOverflow`] when
    /// `sale_price > i128::MAX / 10_000`.
    ///
    /// # Arguments
    /// * `token_id`   — Token being sold.
    /// * `sale_price` — Sale price in the asset's smallest unit (must be > 0).
    ///
    /// # Errors
    /// * [`Error::InvalidTokenId`]    — token does not exist.
    /// * [`Error::InvalidSalePrice`]  — `sale_price` ≤ 0.
    /// * [`Error::RoyaltyOverflow`]   — arithmetic would overflow.
    /// * [`Error::InvalidRoyaltySplit`] — royalty recipients list is empty.
    pub fn royalty_info(
        env: Env,
        token_id: TokenId,
        sale_price: i128,
    ) -> Result<RoyaltyInfo, Error> {
        let royalty = Self::load_token(&env, token_id)?.royalty;
        let total_royalty_amount = royalty.calculate_royalty(sale_price)?;
        let first = royalty.recipients.get(0).ok_or(Error::InvalidRoyaltySplit)?;

        Ok(RoyaltyInfo {
            receiver: first.recipient,
            royalty_amount: total_royalty_amount,
            asset_address: royalty.asset_address,
        })
    }

    /// Pay royalties for a token sale using the SEP-0041 asset in the royalty config.
    ///
    /// Iterates over all recipients and transfers each share via the token client.
    /// Also accrues the total royalty amount to `RoyaltyBalance(token_id)` so
    /// recipients can track lifetime earnings and claim via [`claim_royalties`].
    /// For XLM royalties (`asset_address = None`) the marketplace must handle
    /// the transfer directly — this function returns [`Error::InvalidRecipient`].
    ///
    /// Emits: `"royalty"` [`RoyaltyPaidEvent`] per recipient paid.
    ///
    /// # Arguments
    /// * `payer`      — Address making the payment (must authorize).
    /// * `token_id`   — Token being sold.
    /// * `sale_price` — Sale price in the asset's smallest unit (must be > 0).
    ///
    /// # Errors
    /// * [`Error::InvalidSalePrice`]  — `sale_price` ≤ 0.
    /// * [`Error::InvalidTokenId`]    — token does not exist.
    /// * [`Error::InvalidRecipient`]  — no SEP-0041 asset configured (XLM royalty).
    /// * [`Error::InvalidRoyaltySplit`] — recipients list is empty.
    /// * [`Error::RoyaltyOverflow`]   — arithmetic would overflow.
    pub fn pay_royalty(
        env: Env,
        payer: Address,
        token_id: TokenId,
        sale_price: i128,
    ) -> Result<(), Error> {
        payer.require_auth();
        Self::acquire_reentrancy_lock(&env)?;
        let result = Self::pay_royalty_internal(&env, &payer, token_id, sale_price);
        Self::release_reentrancy_lock(&env);
        result
    }

    /// Internal royalty payout logic (caller must hold reentrancy lock).
    /// Follows check-effects-interactions pattern: updates storage before external transfers.
    fn pay_royalty_internal(
        env: &Env,
        payer: &Address,
        token_id: TokenId,
        sale_price: i128,
    ) -> Result<(), Error> {
        if sale_price <= 0 {
            return Err(Error::InvalidSalePrice);
        }

        let royalty = Self::load_token(env, token_id)?.royalty;
        let asset_address = royalty
            .asset_address
            .clone()
            .ok_or(Error::InvalidRecipient)?;

        // First, calculate total royalty amount (check phase)
        let mut cumulative_bps: u32 = 0;
        let mut cumulative_royalty: i128 = 0;

        for idx in 0..royalty.recipients.len() {
            let split = royalty
                .recipients
                .get(idx)
                .ok_or(Error::InvalidRoyaltySplit)?;
            cumulative_bps = cumulative_bps.saturating_add(split.basis_points);
            let total_so_far = Self::calculate_royalty(sale_price, cumulative_bps)?;
            cumulative_royalty = total_so_far;
        }

        // Update storage before external transfers (effects phase)
        // This ensures state changes are committed before any external calls
        if cumulative_royalty > 0 {
            let prev: i128 = env
                .storage()
                .persistent()
                .get(&DataKey::RoyaltyBalance(token_id))
                .unwrap_or(0);
            env.storage().persistent().set(
                &DataKey::RoyaltyBalance(token_id),
                &(prev.saturating_add(cumulative_royalty)),
            );
        }

        // Now perform external transfers (interactions phase)
        let token_client = soroban_sdk::token::TokenClient::new(env, &asset_address);
        let mut cumulative_bps: u32 = 0;
        let mut cumulative_royalty: i128 = 0;

        for idx in 0..royalty.recipients.len() {
            let split = royalty
                .recipients
                .get(idx)
                .ok_or(Error::InvalidRoyaltySplit)?;

            cumulative_bps = cumulative_bps.saturating_add(split.basis_points);
            let total_so_far = Self::calculate_royalty(sale_price, cumulative_bps)?;
            let amount = total_so_far.saturating_sub(cumulative_royalty);
            cumulative_royalty = total_so_far;

            if amount == 0 {
                continue;
            }

            token_client.transfer(payer, &split.recipient, &amount);
            Self::emit_royalty_paid_event(env, token_id, payer, &split.recipient, amount);
        }

        Ok(())
    }

    /// Claim accumulated royalties for a token.
    ///
    /// Transfers the full `RoyaltyBalance` for `token_id` to the primary royalty
    /// recipient using the SEP-0041 asset configured in the royalty. Clears the
    /// balance atomically (check-effects-interactions) to prevent double-claiming.
    ///
    /// Only the primary royalty recipient (index 0) may call this.
    ///
    /// Emits: `"roy_claim"` [`RoyaltyClaimedEvent`].
    ///
    /// # Arguments
    /// * `caller`   — Must be the primary royalty recipient.
    /// * `token_id` — Token whose royalties are being claimed.
    ///
    /// # Errors
    /// * [`Error::InvalidTokenId`]    — token does not exist.
    /// * [`Error::Unauthorized`]      — caller is not the primary recipient.
    /// * [`Error::InvalidRecipient`]  — no SEP-0041 asset configured.
    /// * [`Error::InsufficientBalance`] — no royalties to claim.
    pub fn claim_royalties(env: Env, caller: Address, token_id: TokenId) -> Result<(), Error> {
        caller.require_auth();
        Self::acquire_reentrancy_lock(&env)?;
        let result = Self::claim_royalties_internal(&env, &caller, token_id);
        Self::release_reentrancy_lock(&env);
        result
    }

    /// Internal royalty claim logic (caller must hold reentrancy lock).
    fn claim_royalties_internal(
        env: &Env,
        caller: &Address,
        token_id: TokenId,
    ) -> Result<(), Error> {
        let royalty = Self::load_token(env, token_id)?.royalty;
        let recipient = royalty
            .recipients
            .get(0)
            .ok_or(Error::InvalidRoyaltySplit)?
            .recipient;

        if caller != &recipient {
            return Err(Error::Unauthorized);
        }

        let asset_address = royalty.asset_address.ok_or(Error::InvalidRecipient)?;

        let balance: i128 = env
            .storage()
            .persistent()
            .get(&DataKey::RoyaltyBalance(token_id))
            .unwrap_or(0);

        if balance <= 0 {
            return Err(Error::InsufficientBalance);
        }

        // Clear balance before transfer (check-effects-interactions).
        env.storage()
            .persistent()
            .remove(&DataKey::RoyaltyBalance(token_id));

        soroban_sdk::token::TokenClient::new(env, &asset_address).transfer(
            &env.current_contract_address(),
            &recipient,
            &balance,
        );

        env.events().publish(
            (symbol_short!("roy_clm"),),
            RoyaltyClaimedEvent {
                token_id,
                recipient,
                amount: balance,
                asset: asset_address,
            },
        );

        Ok(())
    }

    /// Update the royalty configuration for a token.
    /// Access Control: Admin only.
    /// Emits RoyaltyRecipientUpdated event when the primary recipient changes.
    pub fn set_royalty(
        env: Env,
        admin: Address,
        token_id: TokenId,
        new_royalty: Royalty,
    ) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;

        // 1 persistent read
        let mut data = Self::load_token(&env, token_id)?;
        let old_royalty = data.royalty.clone();

        // Emit event if primary recipient changed (compare before normalization)
        if !old_royalty.recipients.is_empty() && !new_royalty.recipients.is_empty() {
            let old_recipient = old_royalty
                .recipients
                .get(0)
                .ok_or(Error::InvalidRoyaltySplit)?;
            let new_recipient = new_royalty
                .recipients
                .get(0)
                .ok_or(Error::InvalidRoyaltySplit)?;

            if old_recipient.recipient != new_recipient.recipient {
                env.events().publish(
                    (symbol_short!("royalty"),),
                    RoyaltyRecipientUpdatedEvent {
                        token_id,
                        old_recipient: old_recipient.recipient,
                        new_recipient: new_recipient.recipient,
                    },
                );
            }
        }

        let new_royalty = Self::normalize_royalty(&env, new_royalty)?;

        data.royalty = new_royalty;
        env.storage()
            .persistent()
            .set(&DataKey::Token(token_id), &data);

        env.events().publish(
            (symbol_short!("roy_upd"),),
            RoyaltyUpdatedEvent { token_id },
        );

        Ok(())
    }

    /// Burn (destroy) an NFT. Only the current owner may burn.
    ///
    /// Storage removes (persistent): TokenData, ClipIdMinted = **2** (Optimized from 4)
    pub fn burn(env: Env, owner: Address, token_id: TokenId) -> Result<(), Error> {
        owner.require_auth();

        if Self::is_frozen(env.clone(), token_id) {
            return Err(Error::TokenFrozen);
        }

        // 1 persistent read — also gives us clip_id for dedup cleanup
        let data: TokenData = Self::load_token(&env, token_id)?;

        if owner != data.owner {
            return Err(Error::Unauthorized);
        }

        // 2 persistent removes
        env.storage().persistent().remove(&DataKey::Token(token_id));
        env.storage()
            .persistent()
            .remove(&DataKey::ClipIdMinted(data.clip_id));

        // Update total supply
        let total_supply: u32 = env
            .storage()
            .instance()
            .get(&DataKey::TotalSupply)
            .unwrap_or(0);
        env.storage()
            .instance()
            .set(&DataKey::TotalSupply, &total_supply.saturating_sub(1));

        // Update balance
        let balance: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::Balance(owner.clone()))
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::Balance(owner.clone()), &balance.saturating_sub(1));

        // Remove from O(1) enumeration indexes (must come after supply/balance decrements).
        Self::index_remove_global(&env, token_id);
        Self::index_remove_owner(&env, &owner, token_id);

        env.events().publish(
            (symbol_short!("burn"),),
            BurnEvent {
                owner: owner.clone(),
                token_id,
                clip_id: data.clip_id,
            },
        );

        // Emit standard Transfer event for ERC-721 compliance
        let burn_to = env.current_contract_address();
        Self::emit_transfer_event(&env, token_id, &owner, &burn_to);

        Ok(())
    }

    /// Burn an NFT and optionally refund unclaimed royalties to the primary royalty recipient.
    ///
    /// When `claim_royalty = true` and the token has a positive `RoyaltyBalance` with a
    /// configured SEP-0041 asset, the balance is transferred to the primary recipient before
    /// the token is destroyed. This follows check-effects-interactions: the balance is cleared
    /// in storage *before* the external token transfer.
    ///
    /// Emits: `"refunded"` [`RefundedEvent`] when royalties are paid out, then `"burn"` [`BurnEvent`].
    ///
    /// # Arguments
    /// * `owner`         — Current owner (must authorize).
    /// * `token_id`      — Token to destroy.
    /// * `claim_royalty` — When `true`, refund any accrued royalty balance before burning.
    ///
    /// # Errors
    /// * [`Error::TokenFrozen`]   — token is frozen.
    /// * [`Error::Unauthorized`]  — caller is not the owner.
    /// * [`Error::InvalidTokenId`] — token does not exist.
    pub fn burn_with_refund(env: Env, owner: Address, token_id: TokenId, claim_royalty: bool) -> Result<(), Error> {
        owner.require_auth();

        if Self::is_frozen(env.clone(), token_id) {
            return Err(Error::TokenFrozen);
        }

        let data: TokenData = Self::load_token(&env, token_id)?;
        if owner != data.owner {
            return Err(Error::Unauthorized);
        }

        // Optionally refund unclaimed royalties before destroying storage.
        if claim_royalty {
            let royalty_balance: i128 = env
                .storage()
                .persistent()
                .get(&DataKey::RoyaltyBalance(token_id))
                .unwrap_or(0);

            if royalty_balance > 0 {
                if let Some(ref asset_address) = data.royalty.asset_address {
                    if let Some(primary) = data.royalty.recipients.get(0) {
                        // Clear balance before external transfer (CEI pattern).
                        env.storage().persistent().remove(&DataKey::RoyaltyBalance(token_id));
                        Self::acquire_reentrancy_lock(&env)?;
                        soroban_sdk::token::TokenClient::new(&env, asset_address)
                            .transfer(&env.current_contract_address(), &primary.recipient, &royalty_balance);
                        Self::release_reentrancy_lock(&env);
                        env.events().publish(
                            (symbol_short!("refunded"),),
                            RefundedEvent {
                                token_id,
                                recipient: primary.recipient.clone(),
                                amount: royalty_balance,
                            },
                        );
                    }
                }
            }
        }

        // Destroy token storage.
        env.storage().persistent().remove(&DataKey::Token(token_id));
        env.storage().persistent().remove(&DataKey::ClipIdMinted(data.clip_id));
        env.storage().persistent().remove(&DataKey::RoyaltyBalance(token_id));

        let total_supply: u32 = env.storage().instance().get(&DataKey::TotalSupply).unwrap_or(0);
        env.storage().instance().set(&DataKey::TotalSupply, &total_supply.saturating_sub(1));

        let bal: u32 = env.storage().persistent().get(&DataKey::Balance(owner.clone())).unwrap_or(0);
        env.storage().persistent().set(&DataKey::Balance(owner.clone()), &bal.saturating_sub(1));

        Self::index_remove_global(&env, token_id, total_supply);
        Self::index_remove_owner(&env, &owner, token_id);

        env.events().publish(
            (symbol_short!("burn"),),
            BurnEvent { owner, token_id, clip_id: data.clip_id },
        );

    /// Get the approved operator for a specific token, if any.
    pub fn get_approved(env: Env, token_id: TokenId) -> Option<Address> {
        env.storage().persistent().get(&DataKey::Approved(token_id))
    }


    /// Transfer NFT ownership from `from` to `to`.
    ///
    /// Blocked if the token is soulbound (non-transferable).
    /// Storage writes (persistent): TokenData = **1**
    ///
    /// # Arguments
    /// * `from`     - Current owner (must authorize)
    /// * `to`       - New owner
    /// * `token_id` - Token to transfer
    pub fn transfer(env: Env, from: Address, to: Address, token_id: TokenId) -> Result<(), Error> {
        from.require_auth();
        Self::require_not_paused(&env)?;

        // 1 persistent read
        let mut data: TokenData = env
            .storage()
            .persistent()
            .get(&DataKey::Token(token_id))
            .ok_or(Error::InvalidTokenId)?;

            // Emit standard Transfer event for ERC-721 compliance (to zero address)
            let burn_to = env.current_contract_address();
            Self::emit_transfer_event(&env, token_id, &owner, &burn_to);
        }

        // Check if token is soulbound
        if data.is_soulbound {
            return Err(Error::SoulboundTransferBlocked);
        }

        // Clear approval when transferred
        env.storage().persistent().remove(&DataKey::Approved(token_id));

        // 1 persistent write — update owner in-place, clip_id unchanged
        data.owner = to.clone();
        env.storage().persistent().set(&DataKey::Token(token_id), &data);

        let gas_used = GAS_BASE_TRANSFER;

        // Update totals
        let total_gas: u64 = env.storage().instance().get(&DataKey::TotalGasTransfer).unwrap_or(0);
        let count: u64 = env.storage().instance().get(&DataKey::CountTransfer).unwrap_or(0);
        env.storage().instance().set(&DataKey::TotalGasTransfer, &total_gas.saturating_add(gas_used));
        env.storage().instance().set(&DataKey::CountTransfer, &count.saturating_add(1));

        env.events().publish(
            (symbol_short!("transfer"),),
            TransferEvent { token_id, from, to, gas_used },
        );

        Ok(())
    }

    /// Transfer NFT ownership from `from` to `to` by an approved `spender`.
    pub fn transfer_from(
        env: Env,
        spender: Address,
        from: Address,
        to: Address,
        token_id: TokenId,
    ) -> Result<(), Error> {
        spender.require_auth();
        Self::require_not_paused(&env)?;

        let mut data: TokenData = env
            .storage()
            .persistent()
            .get(&DataKey::Token(token_id))
            .ok_or(Error::InvalidTokenId)?;

        if from != data.owner {
            return Err(Error::Unauthorized);
        }

        let is_approved_for_all = Self::is_approved_for_all(env.clone(), from.clone(), spender.clone());
        let approved_operator = Self::get_approved(env.clone(), token_id);
        
        let is_approved = is_approved_for_all || approved_operator == Some(spender);
        
        if !is_approved {
            return Err(Error::Unauthorized);
        }

        if data.is_soulbound {
            return Err(Error::SoulboundTransferBlocked);
        }

        // Clear approval
        env.storage().persistent().remove(&DataKey::Approved(token_id));

        data.owner = to.clone();
        env.storage().persistent().set(&DataKey::Token(token_id), &data);
        let gas_used = GAS_BASE_TRANSFER;
        env.events().publish(
            (symbol_short!("transfer"),),
            TransferEvent { token_id, from, to, gas_used },
        );

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Admin Configuration
    // -------------------------------------------------------------------------

    /// Set the collection name. Admin only.
    pub fn set_name(env: Env, admin: Address, name: String) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Name, &name);
        Ok(())
    }

    /// Set the collection symbol. Admin only.
    pub fn set_symbol(env: Env, admin: Address, symbol: String) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Symbol, &symbol);
        Ok(())
    }

    // -------------------------------------------------------------------------
    // View functions
    // -------------------------------------------------------------------------

    /// Returns the contract version.
    pub fn version(_env: Env) -> u32 {
        VERSION
    }

    /// Returns the collection name.
    pub fn name(env: Env) -> String {
        env.storage()
            .instance()
            .get(&DataKey::Name)
            .unwrap_or_else(|| String::from_str(&env, "ClipCash Clips"))
    }

    /// Returns the collection symbol.
    pub fn symbol(env: Env) -> String {
        env.storage()
            .instance()
            .get(&DataKey::Symbol)
            .unwrap_or_else(|| String::from_str(&env, "CLIP"))
    }

    /// Returns the original clip ID for a given token ID.
    pub fn get_clip_id(env: Env, token_id: TokenId) -> Result<u32, Error> {
        Ok(Self::load_token(&env, token_id)?.clip_id)
    }

    /// Returns the owner of a given token ID.
    pub fn owner_of(env: Env, token_id: TokenId) -> Result<Address, Error> {
        Ok(Self::load_token(&env, token_id)?.owner)
    }

    /// Returns the metadata URI for a given token ID.
    pub fn token_uri(env: Env, token_id: TokenId) -> Result<String, Error> {
        Ok(Self::load_token(&env, token_id)?.metadata_uri)
    }

    /// Alias for `token_uri`, kept for compatibility.
    pub fn get_metadata(env: Env, token_id: TokenId) -> Result<String, Error> {
        Ok(Self::load_token(&env, token_id)?.metadata_uri)
    }

    /// Look up the on-chain token ID for a given clip_id.
    pub fn clip_token_id(env: Env, clip_id: u32) -> Result<TokenId, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::ClipIdMinted(clip_id))
            .ok_or(Error::InvalidTokenId)
    }

    /// Returns the stored `Royalty` struct for a token.
    pub fn get_royalty(env: Env, token_id: TokenId) -> Result<Royalty, Error> {
        Ok(Self::load_token(&env, token_id)?.royalty)
    }

    /// Returns the total number of minted (and not yet burned) tokens.
    ///
    /// Derived from `NextTokenId - 1` — no separate counter needed.
    pub fn total_supply(env: Env) -> u32 {
        env.storage()
            .instance()
            .get::<DataKey, u32>(&DataKey::NextTokenId)
            .unwrap_or(1)
            .saturating_sub(1)
    }

    /// Returns token_id for a given 0-based index in minted token order.
    ///
    /// This scans existing token IDs and skips burned entries.
    pub fn token_by_index(env: Env, index: u32) -> Result<TokenId, Error> {
        let mut found: u32 = 0;
        let next_token_id: u32 = env
            .storage()
            .instance()
            .get::<DataKey, u32>(&DataKey::NextTokenId)
            .unwrap_or(1);

        let mut token_id: u32 = 1;
        while token_id < next_token_id {
            if env.storage().persistent().has(&DataKey::Token(token_id)) {
                if found == index {
                    return Ok(token_id);
                }
                found = found.saturating_add(1);
            }
            token_id = token_id.saturating_add(1);
        }

        Err(Error::InvalidTokenId)
    }

    /// Returns true if the token exists.
    pub fn exists(env: Env, token_id: TokenId) -> bool {
        env.storage()
            .persistent()
            .has(&DataKey::Token(token_id))
    }

    /// Returns true if the token is soulbound (non-transferable).
    pub fn is_soulbound(env: Env, token_id: TokenId) -> bool {
        if let Ok(data) = Self::load_token(&env, token_id) {
            data.is_soulbound
        } else {
            false
        }
    }

    /// Returns the average synthetic gas cost for a given operation type.
    /// 0 = Mint, 1 = Transfer
    pub fn get_avg_gas_cost(env: Env, op_type: u32) -> u64 {
        let (total, count) = match op_type {
            0 => (
                env.storage()
                    .instance()
                    .get::<DataKey, u64>(&DataKey::TotalGasMint)
                    .unwrap_or(0),
                env.storage()
                    .instance()
                    .get::<DataKey, u64>(&DataKey::CountMint)
                    .unwrap_or(0),
            ),
            1 => (
                env.storage()
                    .instance()
                    .get::<DataKey, u64>(&DataKey::TotalGasTransfer)
                    .unwrap_or(0),
                env.storage()
                    .instance()
                    .get::<DataKey, u64>(&DataKey::CountTransfer)
                    .unwrap_or(0),
            ),
            _ => (0, 0),
        };

        if count == 0 {
            0
        } else {
            total / count
        }
    }

    // -------------------------------------------------------------------------
    // Royalty extension (EIP-2981 style, with custom asset support)
    // -------------------------------------------------------------------------

    /// Returns the royalty receiver, amount, and payment asset for a given sale price.
    ///
    /// Uses safe math to prevent overflow. Royalty amount is calculated as:
    /// `royalty_amount = sale_price * basis_points / 10000`
    ///
    /// Safe limits: sale_price should not exceed i128::MAX / 10000 to avoid overflow.
    pub fn royalty_info(
        env: Env,
        token_id: TokenId,
        sale_price: i128,
    ) -> Result<RoyaltyInfo, Error> {
        if sale_price <= 0 {
            return Err(Error::InvalidSalePrice);
        }

        let royalty = Self::load_token(&env, token_id)?.royalty;

        let mut total_bps: u32 = 0;
        for idx in 0..royalty.recipients.len() {
            let split = royalty.recipients.get(idx).ok_or(Error::InvalidRoyaltySplit)?;
            total_bps = total_bps.saturating_add(split.basis_points);
        }

        let total_royalty_amount = Self::calculate_royalty(sale_price, total_bps)?;
        let first = royalty.recipients.get(0).ok_or(Error::InvalidRoyaltySplit)?;

        Ok(RoyaltyInfo {
            receiver: first.recipient,
            royalty_amount: total_royalty_amount,
            asset_address: royalty.asset_address,
        })
    }

    /// Returns stored royalty configurations for multiple token IDs.
    pub fn batch_royalty_info(env: Env, token_ids: Vec<TokenId>) -> Result<Vec<Royalty>, Error> {
        let mut out = Vec::new(&env);
        for idx in 0..token_ids.len() {
            let token_id = token_ids.get(idx).ok_or(Error::InvalidTokenId)?;
            out.push_back(Self::load_token(&env, token_id)?.royalty);
        }
        Ok(out)
    }

    /// Returns total platform royalty revenue accumulated on-chain.
    pub fn get_platform_revenue(env: Env) -> i128 {
        env.storage()
            .instance()
            .get::<DataKey, i128>(&DataKey::TotalPlatformFees)
            .unwrap_or(0)
    }

    /// Pay royalties for a token sale using the asset configured in the royalty.
    ///
    /// Only handles SEP-0041 custom assets. For XLM (`asset_address` is `None`)
    /// the marketplace must handle the transfer directly.
    pub fn pay_royalty(
        env: Env,
        payer: Address,
        token_id: TokenId,
        sale_price: i128,
    ) -> Result<(), Error> {
        payer.require_auth();

        if sale_price <= 0 {
            return Err(Error::InvalidSalePrice);
        }
        let royalty = Self::load_token(&env, token_id)?.royalty;
        let platform_recipient: Address = env
            .storage()
            .instance()
            .get(&DataKey::PlatformRecipient)
            .ok_or(Error::InvalidRecipient)?;
        let asset_address = royalty.asset_address.clone().ok_or(Error::InvalidRecipient)?;
        let token_client = soroban_sdk::token::TokenClient::new(&env, &asset_address);
        let mut cumulative_bps: u32 = 0;
        let mut cumulative_royalty: i128 = 0;
        for idx in 0..royalty.recipients.len() {
            let split = royalty.recipients.get(idx).ok_or(Error::InvalidRoyaltySplit)?;
            
            cumulative_bps = cumulative_bps.saturating_add(split.basis_points);
            let total_royalty_so_far = Self::calculate_royalty(sale_price, cumulative_bps)?;
            let amount = total_royalty_so_far.saturating_sub(cumulative_royalty);
            cumulative_royalty = total_royalty_so_far;

            if amount == 0 {
                continue;
            }
            token_client.transfer(&payer, &split.recipient, &amount);

            if split.recipient == platform_recipient {
                let total_fees: i128 = env
                    .storage()
                    .instance()
                    .get(&DataKey::TotalPlatformFees)
                    .unwrap_or(0);
                env.storage()
                    .instance()
                    .set(&DataKey::TotalPlatformFees, &total_fees.saturating_add(amount));
            }

            env.events().publish(
                (symbol_short!("royalty"),),
                RoyaltyPaidEvent {
                    token_id,
                    from: payer.clone(),
                    to: split.recipient,
                    amount,
                },
            );
        }

        Ok(())
    }

    /// Update the royalty configuration for a token. Admin only.
    /// Emits RoyaltyRecipientUpdated event when the primary recipient changes.
    pub fn set_royalty(
        env: Env,
        admin: Address,
        token_id: TokenId,
        new_royalty: Royalty,
    ) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;

        // 1 persistent read
        let mut data = Self::load_token(&env, token_id)?;
        let old_royalty = data.royalty.clone();

        let new_royalty = Self::normalize_royalty(&env, new_royalty)?;

        // Emit event if primary recipient changed
        if !old_royalty.recipients.is_empty() && !new_royalty.recipients.is_empty() {
            let old_recipient = old_royalty.recipients.get(0).ok_or(Error::InvalidRoyaltySplit)?;
            let new_recipient = new_royalty.recipients.get(0).ok_or(Error::InvalidRoyaltySplit)?;
            
            if old_recipient.recipient != new_recipient.recipient {
                env.events().publish(
                    (symbol_short!("royalty"),),
                    RoyaltyRecipientUpdatedEvent {
                        token_id,
                        old_recipient: old_recipient.recipient,
                        new_recipient: new_recipient.recipient,
                    },
                );
            }
        }

        data.royalty = new_royalty;
        env.storage()
            .persistent()
            .set(&DataKey::Token(token_id), &data);

        Ok(())
    }

    /// Burn (destroy) an NFT. Only the current owner may burn.
    ///
    /// Storage removes (persistent): TokenData, ClipIdMinted = **2** (Optimized from 4)
    pub fn burn(env: Env, owner: Address, token_id: TokenId) -> Result<(), Error> {
        owner.require_auth();

        // 1 persistent read — also gives us clip_id for dedup cleanup
        let data: TokenData = Self::load_token(&env, token_id)?;

        if owner != data.owner {
            return Err(Error::Unauthorized);
        }

        // 2 persistent removes
        env.storage().persistent().remove(&DataKey::Token(token_id));
        env.storage()
            .persistent()
            .remove(&DataKey::ClipIdMinted(data.clip_id));

        env.events().publish(
            (symbol_short!("burn"),),
            BurnEvent {
                owner,
                token_id,
                clip_id: data.clip_id,
            },
        );

        Ok(())
    }

    // -------------------------------------------------------------------------
    // Internal helpers
    // -------------------------------------------------------------------------

    /// Load and return `TokenData`, or `InvalidTokenId` if not found.
    fn load_token(env: &Env, token_id: TokenId) -> Result<TokenData, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Token(token_id))
            .ok_or(Error::InvalidTokenId)
    }

    /// Verify the backend Ed25519 signature over the canonical mint payload.
    ///
    /// Payload construction (all hashing via SHA-256):
    /// ```text
    /// owner_hash    = SHA-256(XDR(owner))
    /// uri_hash      = SHA-256(UTF-8(metadata_uri))
    /// message       = SHA-256( clip_id_le4 || owner_hash || uri_hash )
    /// ```
    /// The signer signs `message` (32 bytes) with their Ed25519 private key.
    fn verify_clip_signature(
        env: &Env,
        owner: &Address,
        clip_id: u32,
        metadata_uri: &String,
        signature: &BytesN<64>,
    ) -> Result<(), Error> {
        let signer: BytesN<32> = env
            .storage()
            .instance()
            .get(&DataKey::Signer)
            .ok_or(Error::SignerNotSet)?;

        // Hash the owner address XDR so the payload is always fixed-width
        let owner_hash: BytesN<32> = env.crypto().sha256(&owner.clone().to_xdr(env)).into();

        // Hash the metadata URI bytes
        let uri_hash: BytesN<32> = env.crypto().sha256(&Bytes::from(metadata_uri.to_xdr(env))).into();

        // Build the 68-byte pre-image: 4 (clip_id LE) + 32 (owner_hash) + 32 (uri_hash)
        let mut preimage = Bytes::new(env);
        preimage.extend_from_array(&clip_id.to_le_bytes());
        preimage.append(&Bytes::from(owner_hash));
        preimage.append(&Bytes::from(uri_hash));

        // Final message digest that was signed
        let message: BytesN<32> = env.crypto().sha256(&preimage).into();

        // Panics (traps) on invalid signature — map to our error type
        env.crypto().ed25519_verify(&signer, &Bytes::from(message), signature);

        Ok(())
    }

    fn require_admin(env: &Env, addr: &Address) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .expect("Admin not initialized");

        if addr != &admin {
            return Err(Error::Unauthorized);
        }

        addr.require_auth();
        Ok(())
    }

    fn require_not_paused(env: &Env) -> Result<(), Error> {
        if env
            .storage()
            .instance()
            .get(&DataKey::Paused)
            .unwrap_or(false)
        {
            return Err(Error::ContractPaused);
        }
        Ok(())
    }

    fn normalize_royalty(env: &Env, royalty: Royalty) -> Result<Royalty, Error> {
        if royalty.recipients.is_empty() {
            return Err(Error::InvalidRoyaltySplit);
        }
        let platform: Address = env
            .storage()
            .instance()
            .get(&DataKey::PlatformRecipient)
            .ok_or(Error::InvalidRecipient)?;
        let mut recipients = royalty.recipients;
        let mut has_platform = false;
        let mut total_bps: u32 = 0;
        for idx in 0..recipients.len() {
            let split = recipients.get(idx).ok_or(Error::InvalidRoyaltySplit)?;
            if split.recipient == platform {
                has_platform = true;
            }
            total_bps = total_bps.saturating_add(split.basis_points);
        }
        if !has_platform {
            recipients.push_back(RoyaltyRecipient {
                recipient: platform,
                basis_points: 100, // fixed default 1%
            });
            total_bps = total_bps.saturating_add(100);
        }
        if total_bps > 10_000 {
            return Err(Error::RoyaltyTooHigh);
        }
        Ok(Royalty {
            recipients,
            asset_address: royalty.asset_address,
        })
    }
    pub fn calculate_royalty(sale_price: i128, basis_points: u32) -> Result<i128, Error> {
        if sale_price <= 0 {
            return Err(Error::InvalidSalePrice);
        }
        if sale_price > i128::MAX / 10_000 {
            return Err(Error::RoyaltyOverflow);
        }
        let amount = sale_price.saturating_mul(basis_points as i128);
        Ok((amount.saturating_add(5_000)) / 10_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, BytesN as _, Events as _},
        token, Address, Bytes, BytesN, Env, String, Vec, xdr::ToXdr,
    };

    fn setup() -> (Env, Address, Address, Address) {
        let env = Env::default();
        env.mock_all_auths();
        let admin = Address::generate(&env);
        let user1 = Address::generate(&env);
        let user2 = Address::generate(&env);
        (env, admin, user1, user2)
    }

    fn default_royalty(env: &Env, recipient: Address) -> Royalty {
        let mut recipients = Vec::new(env);
        recipients.push_back(RoyaltyRecipient {
            recipient,
            basis_points: 500,
        });
        Royalty {
            recipients,
            asset_address: None,
        }
    }

    /// Build the canonical mint payload and sign it with `signer_secret`.
    /// Mirrors the on-chain `verify_clip_signature` logic exactly.
    fn sign_mint(
        env: &Env,
        signer_secret: &ed25519_dalek::SigningKey,
        owner: &Address,
        clip_id: u32,
        metadata_uri: &String,
    ) -> BytesN<64> {
        let owner_hash: BytesN<32> = env.crypto().sha256(&owner.clone().to_xdr(env)).into();
        let uri_hash: BytesN<32> = env.crypto().sha256(&Bytes::from(metadata_uri.to_xdr(env))).into();

        let mut preimage = Bytes::new(env);
        preimage.extend_from_array(&clip_id.to_le_bytes());
        preimage.append(&Bytes::from(owner_hash));
        preimage.append(&Bytes::from(uri_hash));

        let message: BytesN<32> = env.crypto().sha256(&preimage).into();
        use ed25519_dalek::Signer as _;
        let sig = signer_secret.sign(&message.to_array());
        BytesN::from_array(env, &sig.to_bytes())
    }

    /// Register a fresh signer keypair and return (pubkey, secret).
    fn register_signer(
        env: &Env,
        client: &ClipsNftContractClient,
        admin: &Address,
    ) -> ed25519_dalek::SigningKey {
        let sk_bytes = soroban_sdk::BytesN::<32>::random(env).to_array();
        let keypair = ed25519_dalek::SigningKey::from_bytes(&sk_bytes);
        let pubkey = BytesN::from_array(env, &keypair.verifying_key().to_bytes());
        client.set_signer(admin, &pubkey);
        keypair
    }

    fn do_mint(
        client: &ClipsNftContractClient,
        env: &Env,
        to: &Address,
        clip_id: u32,
        keypair: &ed25519_dalek::SigningKey,
    ) -> TokenId {
        let uri = String::from_str(env, "ipfs://QmExample");
        let sig = sign_mint(env, keypair, to, clip_id, &uri);
        client.mint(to, &clip_id, &uri, &default_royalty(env, to.clone()), &false, &sig)
    }

    fn do_mint_soulbound(
        client: &ClipsNftContractClient,
        env: &Env,
        to: &Address,
        clip_id: u32,
        keypair: &ed25519_dalek::SigningKey,
    ) -> TokenId {
        let uri = String::from_str(env, "ipfs://QmExample");
        let sig = sign_mint(env, keypair, to, clip_id, &uri);
        client.mint(to, &clip_id, &uri, &default_royalty(env, to.clone()), &true, &sig)
    }

    #[test]
    fn test_version() {
        let env = Env::default();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        assert_eq!(client.version(), 1);
    }

    #[test]
    fn test_mint_stores_owner_and_uri() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 42, &kp);
        assert_eq!(token_id, 1);

        assert_eq!(client.owner_of(&token_id), user1);
        assert_eq!(
            client.token_uri(&token_id),
            String::from_str(&env, "ipfs://QmExample")
        );
        assert_eq!(client.total_supply(), 1);
    }

    #[test]
    fn test_clip_token_id_lookup() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 99, &kp);
        assert_eq!(client.clip_token_id(&99), token_id);
    }

    #[test]
    #[should_panic]
    fn test_double_mint_same_clip_id_panics() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        do_mint(&client, &env, &user1, 7, &kp);
        do_mint(&client, &env, &user1, 7, &kp);
    }

    #[test]
    fn test_mint_emits_event() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 5, &kp);

        let events = env.events().all();
        assert_eq!(events.events().len(), 1);
        assert_eq!(token_id, 1);
    }

    // -------------------------------------------------------------------------
    // Signature verification tests
    // -------------------------------------------------------------------------

    #[test]
    fn test_mint_fails_without_signer_set() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        // No set_signer call

        let kp_bytes = soroban_sdk::BytesN::<32>::random(&env).to_array();
        let kp = ed25519_dalek::SigningKey::from_bytes(&kp_bytes);
        let uri = String::from_str(&env, "ipfs://QmExample");
        let sig = sign_mint(&env, &kp, &user1, 1, &uri);

        let result = client.try_mint(&user1, &1u32, &uri, &default_royalty(&env, user1.clone()), &false, &sig);
        assert_eq!(result, Err(Ok(Error::SignerNotSet)));
    }

    #[test]
    #[should_panic]
    fn test_mint_fails_with_wrong_signature() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        register_signer(&env, &client, &admin);

        // Sign with a *different* keypair — not the registered signer
        let wrong_kp_bytes = soroban_sdk::BytesN::<32>::random(&env).to_array();
        let wrong_kp = ed25519_dalek::SigningKey::from_bytes(&wrong_kp_bytes);
        let uri = String::from_str(&env, "ipfs://QmExample");
        let bad_sig = sign_mint(&env, &wrong_kp, &user1, 1, &uri);

        // ed25519_verify traps on bad sig, which surfaces as a panic in tests
        client.mint(&user1, &1u32, &uri, &default_royalty(&env, user1.clone()), &false, &bad_sig);
    }

    #[test]
    #[should_panic]
    fn test_mint_fails_with_wrong_owner_in_payload() {
        let (env, admin, user1, user2) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let uri = String::from_str(&env, "ipfs://QmExample");
        // Signature is over user2 but we pass user1 as `to`
        let sig_for_user2 = sign_mint(&env, &kp, &user2, 1, &uri);

        client.mint(&user1, &1u32, &uri, &default_royalty(&env, user1.clone()), &false, &sig_for_user2);
    }

    #[test]
    #[should_panic]
    fn test_mint_fails_with_wrong_clip_id_in_payload() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let uri = String::from_str(&env, "ipfs://QmExample");
        // Signature is over clip_id=99 but we pass clip_id=1
        let sig_for_99 = sign_mint(&env, &kp, &user1, 99, &uri);

        client.mint(&user1, &1u32, &uri, &default_royalty(&env, user1.clone()), &false, &sig_for_99);
    }

    #[test]
    fn test_set_signer_and_rotate() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);

        let kp1 = register_signer(&env, &client, &admin);
        let kp1_pub = BytesN::from_array(&env, &kp1.verifying_key().to_bytes());
        assert_eq!(client.get_signer(), Some(kp1_pub));

        // Rotate to a new keypair
        let kp2_bytes = soroban_sdk::BytesN::<32>::random(&env).to_array();
        let kp2 = ed25519_dalek::SigningKey::from_bytes(&kp2_bytes);
        let kp2_pub = BytesN::from_array(&env, &kp2.verifying_key().to_bytes());
        client.set_signer(&admin, &kp2_pub);
        assert_eq!(client.get_signer(), Some(kp2_pub));

        // Old signer's signature should now fail
        let uri = String::from_str(&env, "ipfs://QmExample");
        let old_sig = sign_mint(&env, &kp1, &user1, 1, &uri);
        let result = client.try_mint(&user1, &1u32, &uri, &default_royalty(&env, user1.clone()), &false, &old_sig);
        assert!(result.is_err());
    }

    // -------------------------------------------------------------------------
    // Transfer / royalty / burn / pause tests (unchanged logic)
    // -------------------------------------------------------------------------

    #[test]
    fn test_transfer_updates_owner() {
        let (env, admin, user1, user2) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 1, &kp);
        client.transfer(&user1, &user2, &token_id);

        assert_eq!(client.owner_of(&token_id), user2);
    }

    #[test]
    fn test_transfer_emits_event() {
        let (env, admin, user1, user2) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 3, &kp);
        client.transfer(&user1, &user2, &token_id);

        let events = env.events().all();
        assert_eq!(events.events().len(), 1);
    }

    #[test]
    fn test_total_supply_derived_from_next_token_id() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        assert_eq!(client.total_supply(), 0);
        do_mint(&client, &env, &user1, 1, &kp);
        assert_eq!(client.total_supply(), 1);
        do_mint(&client, &env, &user1, 2, &kp);
        assert_eq!(client.total_supply(), 2);
    }

    #[test]
    fn test_royalty_info_xlm() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 1, &kp);

        let info = client.royalty_info(&token_id, &1_000_000i128);
        assert_eq!(info.royalty_amount, 60_000i128);
        assert_eq!(info.asset_address, None);
    }

    #[test]
    fn test_royalty_info_custom_asset() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let asset_addr = Address::generate(&env);
        let mut recipients = Vec::new(&env);
        recipients.push_back(RoyaltyRecipient {
            recipient: user1.clone(),
            basis_points: 1000,
        });
        let royalty = Royalty {
            recipients,
            asset_address: Some(asset_addr.clone()),
        };
        let uri = String::from_str(&env, "ipfs://QmCustom");
        let sig = sign_mint(&env, &kp, &user1, 2, &uri);
        let token_id = client.mint(&user1, &2u32, &uri, &royalty, &false, &sig);

        let info = client.royalty_info(&token_id, &500i128);
        assert_eq!(info.royalty_amount, 55i128);
        assert_eq!(info.asset_address, Some(asset_addr));
    }

    #[test]
    fn test_set_royalty_with_custom_asset() {
        let (env, admin, user1, user2) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 1, &kp);

        let asset_addr = Address::generate(&env);
        let mut recipients = Vec::new(&env);
        recipients.push_back(RoyaltyRecipient {
            recipient: user2.clone(),
            basis_points: 1000,
        });
        let new_royalty = Royalty {
            recipients,
            asset_address: Some(asset_addr.clone()),
        };
        client.set_royalty(&admin, &token_id, &new_royalty);

        let stored = client.get_royalty(&token_id);
        assert_eq!(stored.recipients.get(0).unwrap().recipient, user2);
        assert_eq!(stored.recipients.get(0).unwrap().basis_points, 1000);
        assert_eq!(stored.recipients.len(), 2);
        assert_eq!(stored.asset_address, Some(asset_addr));
    }

    #[test]
    fn test_burn() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 1, &kp);
        client.burn(&user1, &token_id);

        assert!(!client.exists(&token_id));
        // clip_id dedup entry also removed — can re-mint same clip_id
        let token_id2 = do_mint(&client, &env, &user1, 1, &kp);
        assert!(client.exists(&token_id2));
    }

    #[test]
    fn test_burn_emits_event() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 77, &kp);
        client.burn(&user1, &token_id);

        let events = env.events().all();
        assert_eq!(events.events().len(), 1);
    }

    #[test]
    fn test_pause_blocks_mint() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        assert!(!client.is_paused());
        client.pause(&admin);
        assert!(client.is_paused());

        let uri = String::from_str(&env, "ipfs://QmPaused");
        let sig = sign_mint(&env, &kp, &user1, 1, &uri);
        let result = client.try_mint(&user1, &1u32, &uri, &default_royalty(&env, user1.clone()), &false, &sig);
        assert_eq!(result, Err(Ok(Error::ContractPaused)));
    }

    #[test]
    fn test_pause_blocks_transfer() {
        let (env, admin, user1, user2) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 1, &kp);
        client.pause(&admin);

        let result = client.try_transfer(&user1, &user2, &token_id);
        assert_eq!(result, Err(Ok(Error::ContractPaused)));
    }

    #[test]
    fn test_unpause_restores_mint_and_transfer() {
        let (env, admin, user1, user2) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        client.pause(&admin);
        client.unpause(&admin);
        assert!(!client.is_paused());

        let token_id = do_mint(&client, &env, &user1, 1, &kp);
        client.transfer(&user1, &user2, &token_id);
        assert_eq!(client.owner_of(&token_id), user2);
    }

    #[test]
    #[should_panic]
    fn test_non_admin_cannot_pause() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);

        client.pause(&user1);
    }

    // =========================================================================
    // NEW COMPREHENSIVE TESTS FOR ISSUES #55, #53, #57, #9
    // =========================================================================

    // -------------------------------------------------------------------------
    // Issue #55: Soulbound (non-transferable) clips support
    // -------------------------------------------------------------------------

    #[test]
    fn test_mint_soulbound_token() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint_soulbound(&client, &env, &user1, 100, &kp);
        assert_eq!(token_id, 1);
        assert_eq!(client.owner_of(&token_id), user1);
        assert!(client.is_soulbound(&token_id));
    }

    #[test]
    fn test_soulbound_transfer_blocked() {
        let (env, admin, user1, user2) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint_soulbound(&client, &env, &user1, 101, &kp);
        
        // Attempt to transfer soulbound token should fail
        let result = client.try_transfer(&user1, &user2, &token_id);
        assert_eq!(result, Err(Ok(Error::SoulboundTransferBlocked)));
        
        // Owner should remain unchanged
        assert_eq!(client.owner_of(&token_id), user1);
    }

    #[test]
    fn test_regular_token_transferable() {
        let (env, admin, user1, user2) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 102, &kp);
        assert!(!client.is_soulbound(&token_id));
        
        // Regular token should transfer successfully
        client.transfer(&user1, &user2, &token_id);
        assert_eq!(client.owner_of(&token_id), user2);
    }

    #[test]
    fn test_soulbound_can_be_burned() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint_soulbound(&client, &env, &user1, 103, &kp);
        assert!(client.exists(&token_id));
        
        // Soulbound token can still be burned by owner
        client.burn(&user1, &token_id);
        assert!(!client.exists(&token_id));
    }

    // -------------------------------------------------------------------------
    // Issue #53: Safe royalty calculation with overflow protection
    // -------------------------------------------------------------------------

    #[test]
    fn test_royalty_calculation_safe_math() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 104, &kp);
        
        // Test with large but safe values
        let large_price = 1_000_000_000_000_000i128; // 10^15
        let info = client.royalty_info(&token_id, &large_price);
        
        // Should calculate without overflow: 10^15 * 600 / 10000 = 6 * 10^13
        assert_eq!(info.royalty_amount, 60_000_000_000_000i128);
    }

    #[test]
    fn test_royalty_overflow_detection() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 105, &kp);
        
        // Test with value that would overflow: i128::MAX
        let overflow_price = i128::MAX;
        let result = client.try_royalty_info(&token_id, &overflow_price);
        
        // Should detect overflow and return error
        assert_eq!(result, Err(Ok(Error::RoyaltyOverflow)));
    }

    #[test]
    fn test_royalty_calculation_max_u128_values() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 106, &kp);
        
        // Test with maximum safe price: i128::MAX / 10000
        let max_safe_price = i128::MAX / 10_000;
        let info = client.royalty_info(&token_id, &max_safe_price);
        
        // Should succeed with safe calculation
        assert!(info.royalty_amount > 0);
    }

    // -------------------------------------------------------------------------
    // Issue #57: Events for royalty recipient changes
    // -------------------------------------------------------------------------

    #[test]
    fn test_royalty_recipient_updated_event() {
        let (env, admin, user1, user2) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 107, &kp);
        
        // Change royalty recipient
        let mut recipients = Vec::new(&env);
        recipients.push_back(RoyaltyRecipient {
            recipient: user2.clone(),
            basis_points: 500,
        });
        let new_royalty = Royalty {
            recipients,
            asset_address: None,
        };
        
        client.set_royalty(&admin, &token_id, &new_royalty);
        
        // Verify event was emitted
        let events = env.events().all();
        assert!(events.events().len() > 0);
    }

    #[test]
    fn test_royalty_recipient_no_event_if_unchanged() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 108, &kp);
        
        // Get current royalty
        let _current_royalty = client.get_royalty(&token_id);
        
        // Set same royalty (only basis points change, recipient stays same)
        let mut recipients = Vec::new(&env);
        recipients.push_back(RoyaltyRecipient {
            recipient: user1.clone(),
            basis_points: 600, // Different basis points, same recipient
        });
        let new_royalty = Royalty {
            recipients,
            asset_address: None,
        };
        
        client.set_royalty(&admin, &token_id, &new_royalty);
        
        // Verify royalty was updated
        let updated = client.get_royalty(&token_id);
        assert_eq!(updated.recipients.get(0).unwrap().basis_points, 600);
    }

    // -------------------------------------------------------------------------
    // Issue #9: Comprehensive unit tests for mint and royalty functions
    // -------------------------------------------------------------------------

    #[test]
    fn test_successful_mint_with_metadata() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let uri = String::from_str(&env, "ipfs://QmTestMetadata");
        let sig = sign_mint(&env, &kp, &user1, 200, &uri);
        let token_id = client.mint(&user1, &200u32, &uri, &default_royalty(&env, user1.clone()), &false, &sig);
        
        assert_eq!(token_id, 1);
        assert_eq!(client.token_uri(&token_id), uri);
        assert_eq!(client.owner_of(&token_id), user1);
    }

    #[test]
    fn test_royalty_payment_on_transfer() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 201, &kp);
        
        // Verify royalty info is correct
        let sale_price = 10_000_000i128;
        let royalty_info = client.royalty_info(&token_id, &sale_price);
        
        // 5% creator + 1% platform = 6% total
        assert_eq!(royalty_info.royalty_amount, 600_000i128);
        assert_eq!(royalty_info.receiver, user1);
    }

    #[test]
    fn test_double_mint_prevention() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let uri = String::from_str(&env, "ipfs://QmUnique");
        let sig = sign_mint(&env, &kp, &user1, 202, &uri);
        
        // First mint succeeds
        let token_id = client.mint(&user1, &202u32, &uri, &default_royalty(&env, user1.clone()), &false, &sig);
        assert_eq!(token_id, 1);
        
        // Second mint with same clip_id should fail
        let sig2 = sign_mint(&env, &kp, &user1, 202, &uri);
        let result = client.try_mint(&user1, &202u32, &uri, &default_royalty(&env, user1.clone()), &false, &sig2);
        assert_eq!(result, Err(Ok(Error::TokenAlreadyMinted)));
    }

    #[test]
    fn test_unauthorized_mint_attempt() {
        let (env, admin, user1, user2) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let uri = String::from_str(&env, "ipfs://QmUnauth");
        // Sign for user1 but try to mint as user2
        let sig = sign_mint(&env, &kp, &user1, 203, &uri);
        
        let result = client.try_mint(&user2, &203u32, &uri, &default_royalty(&env, user2.clone()), &false, &sig);
        // Should fail because signature doesn't match the caller
        assert!(result.is_err());
    }

    #[test]
    fn test_mint_and_burn_cycle() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        // Mint token
        let token_id = do_mint(&client, &env, &user1, 204, &kp);
        assert!(client.exists(&token_id));
        assert_eq!(client.total_supply(), 1);
        
        // Burn token
        client.burn(&user1, &token_id);
        assert!(!client.exists(&token_id));
        // Note: total_supply is derived from NextTokenId - 1, so it remains 1
        // even after burning (NextTokenId is still 2)
        assert_eq!(client.total_supply(), 1);
        
        // Can re-mint same clip_id after burn
        let token_id2 = do_mint(&client, &env, &user1, 204, &kp);
        assert!(client.exists(&token_id2));
        // Now NextTokenId is 3, so total_supply is 2
        assert_eq!(client.total_supply(), 2);
    }

    #[test]
    fn test_multiple_mints_increment_token_id() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id1 = do_mint(&client, &env, &user1, 205, &kp);
        let token_id2 = do_mint(&client, &env, &user1, 206, &kp);
        let token_id3 = do_mint(&client, &env, &user1, 207, &kp);
        
        assert_eq!(token_id1, 1);
        assert_eq!(token_id2, 2);
        assert_eq!(token_id3, 3);
        assert_eq!(client.total_supply(), 3);
    }

    #[test]
    fn test_royalty_with_zero_sale_price_fails() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 208, &kp);
        
        // Zero price should fail
        let result = client.try_royalty_info(&token_id, &0i128);
        assert_eq!(result, Err(Ok(Error::InvalidSalePrice)));
        
        // Negative price should fail
        let result = client.try_royalty_info(&token_id, &(-1000i128));
        assert_eq!(result, Err(Ok(Error::InvalidSalePrice)));
    }

    #[test]
    fn test_royalty_calculation_accuracy() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 209, &kp);
        
        // Test various prices
        let test_cases: [(i128, i128); 4] = [
            (100i128, 6i128),           // 100 * 0.06 = 6
            (1000i128, 60i128),         // 1000 * 0.06 = 60
            (10000i128, 600i128),       // 10000 * 0.06 = 600
            (1_000_000i128, 60_000i128), // 1M * 0.06 = 60k
        ];
        
        for (price, expected) in test_cases.iter() {
            let info = client.royalty_info(&token_id, price);
            assert_eq!(info.royalty_amount, *expected);
        }
    }

    #[test]
    fn test_gas_tracking_averages() {
        let (env, admin, user1, user2) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        // Op types: 0 = Mint, 1 = Transfer

        // 1. First Mint
        let uri1 = String::from_str(&env, "ipfs://short");
        let sig1 = sign_mint(&env, &kp, &user1, 1, &uri1);
        client.mint(&user1, &1u32, &uri1, &default_royalty(&env, user1.clone()), &false, &sig1);

        let avg1 = client.get_avg_gas_cost(&0);
        assert!(avg1 >= GAS_BASE_MINT);

        // 2. Second Mint with longer URI
        let uri2 = String::from_str(&env, "ipfs://very-very-very-long-metadata-uri-that-should-increase-gas");
        let sig2 = sign_mint(&env, &kp, &user1, 2, &uri2);
        client.mint(&user1, &2u32, &uri2, &default_royalty(&env, user1.clone()), &false, &sig2);

        let avg2 = client.get_avg_gas_cost(&0);
        assert!(avg2 > avg1);

        // 3. Transfer
        client.transfer(&user1, &user2, &1u32);
        let avg_t = client.get_avg_gas_cost(&1);
        assert_eq!(avg_t, GAS_BASE_TRANSFER);

        // 4. Second Transfer (same cost)
        client.transfer(&user1, &user2, &2u32);
        assert_eq!(client.get_avg_gas_cost(&1), GAS_BASE_TRANSFER);
    }

    #[test]
    fn test_token_by_index_skips_burned_tokens() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let t1 = do_mint(&client, &env, &user1, 300, &kp);
        let t2 = do_mint(&client, &env, &user1, 301, &kp);
        let t3 = do_mint(&client, &env, &user1, 302, &kp);
        client.burn(&user1, &t2);

        assert_eq!(client.token_by_index(&0), t1);
        assert_eq!(client.token_by_index(&1), t3);
        assert!(client.try_token_by_index(&2).is_err());
    }

    #[test]
    fn test_batch_royalty_info_returns_multiple_entries() {
        let (env, admin, user1, _) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let t1 = do_mint(&client, &env, &user1, 310, &kp);
        let t2 = do_mint(&client, &env, &user1, 311, &kp);

        let mut ids = Vec::new(&env);
        ids.push_back(t1);
        ids.push_back(t2);

        let info = client.batch_royalty_info(&ids);
        assert_eq!(info.len(), 2);
        assert_eq!(info.get(0).unwrap().recipients.len(), 2);
        assert_eq!(info.get(1).unwrap().recipients.len(), 2);
    }

    #[test]
    fn test_platform_revenue_tracks_royalty_payment() {
        let (env, admin, creator, buyer) = setup();
        let contract_id = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &contract_id);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_admin = Address::generate(&env);
        let token_address = env.register_stellar_asset_contract_v2(token_admin.clone()).address();
        let token_admin_client = token::StellarAssetClient::new(&env, &token_address);

        let sale_price = 1_000i128;
        token_admin_client.mint(&buyer, &sale_price);

        let mut recipients = Vec::new(&env);
        recipients.push_back(RoyaltyRecipient {
            recipient: creator.clone(),
            basis_points: 500,
        });
        let royalty = Royalty {
            recipients,
            asset_address: Some(token_address),
        };

        let uri = String::from_str(&env, "ipfs://QmRevenue");
        let sig = sign_mint(&env, &kp, &creator, 320, &uri);
        let token_id = client.mint(&creator, &320u32, &uri, &royalty, &false, &sig);

        assert_eq!(client.get_platform_revenue(), 0);
        client.pay_royalty(&buyer, &token_id, &sale_price);
        assert_eq!(client.get_platform_revenue(), 10);
    }

    // -------------------------------------------------------------------------
    // #294: freeze / unfreeze individual NFTs
    // -------------------------------------------------------------------------

    #[test]
    fn test_freeze_blocks_transfer() {
        let (env, admin, user1, user2) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let sk = register_signer(&env, &client, &admin);
        let tid = do_mint(&client, &env, &user1, 1100, &sk);
        client.freeze_token(&admin, &tid);
        assert!(client.is_frozen(&tid));
        assert_eq!(
            client.try_transfer(&user1, &user2, &tid, &0i128, &None),
            Err(Ok(Error::TokenFrozen))
        );
    }

    #[test]
    fn test_unfreeze_restores_transfer() {
        let (env, admin, user1, user2) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let sk = register_signer(&env, &client, &admin);
        let tid = do_mint(&client, &env, &user1, 1101, &sk);
        client.freeze_token(&admin, &tid);
        client.unfreeze_token(&admin, &tid);
        assert!(!client.is_frozen(&tid));
        client.transfer(&user1, &user2, &tid, &0i128, &None);
        assert_eq!(client.owner_of(&tid), user2);
    }

    #[test]
    fn test_unfreeze_non_admin_fails() {
        let (env, admin, user1, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let sk = register_signer(&env, &client, &admin);
        let tid = do_mint(&client, &env, &user1, 1102, &sk);
        client.freeze_token(&admin, &tid);
        assert_eq!(
            client.try_unfreeze_token(&user1, &tid),
            Err(Ok(Error::Unauthorized))
        );
    }

    #[test]
    fn test_freeze_invalid_token_fails() {
        let (env, admin, _, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        assert_eq!(
            client.try_freeze_token(&admin, &9999u32),
            Err(Ok(Error::InvalidTokenId))
        );
    }

    // -------------------------------------------------------------------------
    // #297: burn_with_refund
    // -------------------------------------------------------------------------

    #[test]
    fn test_burn_with_refund_no_royalty_balance() {
        let (env, admin, user1, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let sk = register_signer(&env, &client, &admin);
        let tid = do_mint(&client, &env, &user1, 1200, &sk);
        // claim_royalty=true but no royalty balance — should still burn cleanly
        client.burn_with_refund(&user1, &tid, &true);
        assert!(!client.exists(&tid));
        assert_eq!(client.total_supply(), 0);
    }

    #[test]
    fn test_burn_with_refund_false_skips_refund() {
        let (env, admin, user1, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let sk = register_signer(&env, &client, &admin);
        let tid = do_mint(&client, &env, &user1, 1201, &sk);
        client.burn_with_refund(&user1, &tid, &false);
        assert!(!client.exists(&tid));
    }

    #[test]
    fn test_burn_with_refund_frozen_fails() {
        let (env, admin, user1, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let sk = register_signer(&env, &client, &admin);
        let tid = do_mint(&client, &env, &user1, 1202, &sk);
        client.freeze_token(&admin, &tid);
        assert_eq!(
            client.try_burn_with_refund(&user1, &tid, &true),
            Err(Ok(Error::TokenFrozen))
        );
    }

    #[test]
    fn test_burn_with_refund_non_owner_fails() {
        let (env, admin, user1, user2) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let sk = register_signer(&env, &client, &admin);
        let tid = do_mint(&client, &env, &user1, 1203, &sk);
        assert_eq!(
            client.try_burn_with_refund(&user2, &tid, &false),
            Err(Ok(Error::Unauthorized))
        );
    }
}
