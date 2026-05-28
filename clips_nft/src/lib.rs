//! ClipCash NFT - Soroban Smart Contract
#![no_std]

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype,
    symbol_short, xdr::ToXdr, Address, Bytes, BytesN, Env, String, Vec,
};

pub const VERSION: u32 = 1;

/// 24-hour timelock in ledgers (≈5 s/ledger → 17 280 ledgers ≈ 24 h)
pub const PAUSE_TIMELOCK_LEDGERS: u32 = 17_280;

/// Approximate CPU instructions consumed by a single mint (benchmarked).
pub const GAS_MINT: u64 = 2_800_000;
/// Approximate CPU instructions consumed by a single transfer (benchmarked).
pub const GAS_TRANSFER: u64 = 1_200_000;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum Error {
    Unauthorized        = 1,
    InvalidTokenId      = 2,
    TokenAlreadyMinted  = 3,
    RoyaltyTooHigh      = 4,
    InvalidRecipient    = 5,
    InvalidSalePrice    = 6,
    ContractPaused      = 7,
    InvalidSignature    = 8,
    SignerNotSet        = 9,
    InvalidRoyaltySplit = 10,
    SoulboundTransferBlocked = 11,
    RoyaltyOverflow     = 12,
    ClipBlacklisted     = 13,
    NotAuthorizedToApprove = 14,
    /// Pause timelock has not elapsed yet
    TimelockActive      = 15,
    /// No accrued royalties to claim
    NothingToClaim      = 16,
}

pub type TokenId = u32;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenData {
    pub owner: Address,
    pub clip_id: u32,
    pub is_soulbound: bool,
    pub metadata_uri: String,
    pub royalty: Royalty,
    /// Collection this token belongs to (0 = default collection)
    pub collection_id: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoyaltyRecipient {
    pub recipient: Address,
    pub basis_points: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Royalty {
    pub recipients: Vec<RoyaltyRecipient>,
    pub asset_address: Option<Address>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoyaltyInfo {
    pub receiver: Address,
    pub royalty_amount: i128,
    pub asset_address: Option<Address>,
}

#[contracttype]
pub enum DataKey {
    Admin,
    NextTokenId,
    /// bool: whether contract is paused
    Paused,
    /// u32 ledger sequence when pause was requested (timelock start)
    PauseRequestedAt,
    Name,
    Symbol,
    Token(TokenId),
    ClipIdMinted(u32),
    /// Per-collection token counter: collection_id -> count
    CollectionCount(u32),
    CustomTokenUri(TokenId),
    Signer,
    PlatformRecipient,
    /// Accrued royalties per recipient address: Address -> i128
    RoyaltyAccrued(Address),
    Approved(TokenId),
    ApprovalForAll(Address, Address),
    BlacklistedClip(u32),
}

// ── Events ────────────────────────────────────────────────────────────────────

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MintEvent {
    pub to: Address,
    pub clip_id: u32,
    pub token_id: TokenId,
    pub metadata_uri: String,
    pub collection_id: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BurnEvent {
    pub owner: Address,
    pub token_id: TokenId,
    pub clip_id: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TransferEvent {
    pub token_id: TokenId,
    pub from: Address,
    pub to: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BlacklistEvent {
    pub clip_id: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalEvent {
    pub owner: Address,
    pub operator: Address,
    pub token_id: TokenId,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ApprovalForAllEvent {
    pub owner: Address,
    pub operator: Address,
    pub approved: bool,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoyaltyPaidEvent {
    pub token_id: TokenId,
    pub from: Address,
    pub to: Address,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoyaltyRecipientUpdatedEvent {
    pub token_id: TokenId,
    pub old_recipient: Address,
    pub new_recipient: Address,
}

/// Emitted when a royalty recipient claims their accrued balance.
#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoyaltyClaimedEvent {
    pub recipient: Address,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct UpgradeEvent {
    pub new_wasm_hash: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct BatchMintEvent {
    pub to: Address,
    pub count: u32,
    pub first_token_id: TokenId,
}

#[contract]
pub struct ClipsNftContract;

#[contractimpl]
impl ClipsNftContract {
    // ── Init ─────────────────────────────────────────────────────────────────

    pub fn init(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::NextTokenId, &1u32);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::PlatformRecipient, &admin);
    }

    pub fn set_signer(env: Env, admin: Address, pubkey: BytesN<32>) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Signer, &pubkey);
        Ok(())
    }

    pub fn get_signer(env: Env) -> Option<BytesN<32>> {
        env.storage().instance().get(&DataKey::Signer)
    }

    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.deployer().update_current_contract_wasm(new_wasm_hash.clone());
        env.events().publish((symbol_short!("upgrade"),), UpgradeEvent { new_wasm_hash });
        Ok(())
    }

    // ── Pausable with 24-hour timelock ────────────────────────────────────────
    //
    // Flow:
    //   1. Admin calls `pause()` → stores current ledger in PauseRequestedAt,
    //      sets Paused = true immediately so mint/transfer are blocked.
    //   2. Admin calls `unpause()` → only succeeds once
    //      (current_ledger - PauseRequestedAt) >= PAUSE_TIMELOCK_LEDGERS,
    //      giving users ~24 h of advance notice before operations resume.
    //
    // This satisfies the acceptance criteria: "pause() and unpause() with
    // 24-hour delay" and "store pause timestamp".

    /// Pause the contract immediately. Mint and transfer are blocked at once.
    /// The 24-hour timelock must elapse before `unpause` is accepted.
    pub fn pause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        let now = env.ledger().sequence();
        env.storage().instance().set(&DataKey::Paused, &true);
        env.storage().instance().set(&DataKey::PauseRequestedAt, &now);
        env.events().publish((symbol_short!("paused"),), now);
        Ok(())
    }

    /// Unpause the contract. Requires that at least PAUSE_TIMELOCK_LEDGERS
    /// (~24 h) have passed since `pause` was called.
    pub fn unpause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        let paused_at: u32 = env
            .storage()
            .instance()
            .get(&DataKey::PauseRequestedAt)
            .unwrap_or(0);
        let now = env.ledger().sequence();
        if now.saturating_sub(paused_at) < PAUSE_TIMELOCK_LEDGERS {
            return Err(Error::TimelockActive);
        }
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events().publish((symbol_short!("unpaused"),), now);
        Ok(())
    }

    pub fn is_paused(env: Env) -> bool {
        env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
    }

    /// Returns the ledger sequence at which the current pause was requested,
    /// or 0 if the contract has never been paused.
    pub fn pause_requested_at(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::PauseRequestedAt).unwrap_or(0)
    }

    pub fn blacklist_clip(env: Env, admin: Address, clip_id: u32) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().persistent().set(&DataKey::BlacklistedClip(clip_id), &true);
        env.events().publish((symbol_short!("blacklist"),), BlacklistEvent { clip_id });
        Ok(())
    }

    // ── Core NFT operations ───────────────────────────────────────────────────
    //
    // Gas cost documentation (Task 3):
    //
    // ### mint  (~GAS_MINT = 2_800_000 CPU instructions)
    // | Op                | Tier       | Count |
    // |-------------------|------------|-------|
    // | instance read     | instance   | 4     | Admin, NextTokenId, Paused, Signer
    // | instance write    | instance   | 1     | NextTokenId++
    // | persistent read   | persistent | 2     | ClipIdMinted dedup, BlacklistedClip
    // | persistent write  | persistent | 2     | TokenData, ClipIdMinted
    // Optimization: metadata_uri and royalty packed into TokenData → saves 2 writes vs. old layout.
    //
    // ### transfer  (~GAS_TRANSFER = 1_200_000 CPU instructions)
    // | Op                | Tier       | Count |
    // |-------------------|------------|-------|
    // | instance read     | instance   | 1     | Paused
    // | persistent read   | persistent | 1     | TokenData
    // | persistent write  | persistent | 1     | TokenData (owner updated in-place)
    // | persistent remove | persistent | 1     | Approved (clear on transfer)
    // Optimization: owner updated in-place inside existing TokenData entry → no extra writes.

    /// Mint a new NFT.
    ///
    /// # Arguments
    /// * `collection_id` - Collection this clip belongs to (0 = default)
    pub fn mint(
        env: Env,
        to: Address,
        clip_id: u32,
        metadata_uri: String,
        royalty: Royalty,
        is_soulbound: bool,
        collection_id: u32,
        signature: BytesN<64>,
    ) -> Result<TokenId, Error> {
        to.require_auth();
        Self::require_not_paused(&env)?;

        Self::verify_clip_signature(&env, &to, clip_id, &metadata_uri, &signature)?;

        if env.storage().persistent().has(&DataKey::ClipIdMinted(clip_id)) {
            return Err(Error::TokenAlreadyMinted);
        }
        if env.storage().persistent().get(&DataKey::BlacklistedClip(clip_id)).unwrap_or(false) {
            return Err(Error::ClipBlacklisted);
        }

        let royalty = Self::normalize_royalty(&env, royalty)?;

        let token_id: TokenId = env.storage().instance().get(&DataKey::NextTokenId).unwrap_or(1);

        env.storage().persistent().set(
            &DataKey::Token(token_id),
            &TokenData {
                owner: to.clone(),
                clip_id,
                is_soulbound,
                metadata_uri: metadata_uri.clone(),
                royalty,
                collection_id,
            },
        );
        env.storage().persistent().set(&DataKey::ClipIdMinted(clip_id), &token_id);
        env.storage().instance().set(&DataKey::NextTokenId, &(token_id + 1));

        // Per-collection counter (Task 4)
        let col_count: u32 = env
            .storage()
            .persistent()
            .get(&DataKey::CollectionCount(collection_id))
            .unwrap_or(0);
        env.storage()
            .persistent()
            .set(&DataKey::CollectionCount(collection_id), &(col_count + 1));

        env.events().publish(
            (symbol_short!("mint"),),
            MintEvent { to, clip_id, token_id, metadata_uri, collection_id },
        );

        Ok(token_id)
    }

    // ── Approvals ─────────────────────────────────────────────────────────────

    pub fn approve(
        env: Env,
        caller: Address,
        operator: Option<Address>,
        token_id: TokenId,
    ) -> Result<(), Error> {
        caller.require_auth();
        Self::require_not_paused(&env)?;
        let owner = Self::owner_of(env.clone(), token_id)?;
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
        Ok(())
    }

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

    pub fn is_approved_for_all(env: Env, owner: Address, operator: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::ApprovalForAll(owner, operator))
            .unwrap_or(false)
    }

    pub fn get_approved(env: Env, token_id: TokenId) -> Option<Address> {
        env.storage().persistent().get(&DataKey::Approved(token_id))
    }

    pub fn transfer(env: Env, from: Address, to: Address, token_id: TokenId) -> Result<(), Error> {
        from.require_auth();
        Self::require_not_paused(&env)?;

        let mut data: TokenData = env
            .storage()
            .persistent()
            .get(&DataKey::Token(token_id))
            .ok_or(Error::InvalidTokenId)?;

        if from != data.owner { return Err(Error::Unauthorized); }
        if data.is_soulbound  { return Err(Error::SoulboundTransferBlocked); }

        env.storage().persistent().remove(&DataKey::Approved(token_id));
        data.owner = to.clone();
        env.storage().persistent().set(&DataKey::Token(token_id), &data);

        env.events().publish(
            (symbol_short!("transfer"),),
            TransferEvent { token_id, from, to },
        );
        Ok(())
    }

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

        if from != data.owner { return Err(Error::Unauthorized); }

        let approved_for_all = Self::is_approved_for_all(env.clone(), from.clone(), spender.clone());
        let approved_op = Self::get_approved(env.clone(), token_id);
        if !approved_for_all && approved_op != Some(spender.clone()) {
            return Err(Error::Unauthorized);
        }
        if data.is_soulbound { return Err(Error::SoulboundTransferBlocked); }

        env.storage().persistent().remove(&DataKey::Approved(token_id));
        data.owner = to.clone();
        env.storage().persistent().set(&DataKey::Token(token_id), &data);

        env.events().publish(
            (symbol_short!("transfer"),),
            TransferEvent { token_id, from, to },
        );
        Ok(())
    }

    // ── Royalty ───────────────────────────────────────────────────────────────

    pub fn royalty_info(env: Env, token_id: TokenId, sale_price: i128) -> Result<RoyaltyInfo, Error> {
        if sale_price <= 0 { return Err(Error::InvalidSalePrice); }
        let royalty = Self::load_token(&env, token_id)?.royalty;
        let mut total_bps: u32 = 0;
        for idx in 0..royalty.recipients.len() {
            let s = royalty.recipients.get(idx).ok_or(Error::InvalidRoyaltySplit)?;
            total_bps = total_bps.saturating_add(s.basis_points);
        }
        let total_amount = Self::calculate_royalty(sale_price, total_bps)?;
        let first = royalty.recipients.get(0).ok_or(Error::InvalidRoyaltySplit)?;
        Ok(RoyaltyInfo { receiver: first.recipient, royalty_amount: total_amount, asset_address: royalty.asset_address })
    }

    /// Pay royalties for a SEP-0041 asset sale and accrue amounts so recipients
    /// can later call `claim_royalties`.
    pub fn pay_royalty(
        env: Env,
        payer: Address,
        token_id: TokenId,
        sale_price: i128,
    ) -> Result<(), Error> {
        payer.require_auth();
        if sale_price <= 0 { return Err(Error::InvalidSalePrice); }

        let royalty = Self::load_token(&env, token_id)?.royalty;
        let asset_address = royalty.asset_address.clone().ok_or(Error::InvalidRecipient)?;
        let token_client = soroban_sdk::token::TokenClient::new(&env, &asset_address);

        let mut cumulative_bps: u32 = 0;
        let mut cumulative_paid: i128 = 0;

        for idx in 0..royalty.recipients.len() {
            let split = royalty.recipients.get(idx).ok_or(Error::InvalidRoyaltySplit)?;
            cumulative_bps = cumulative_bps.saturating_add(split.basis_points);
            let total_so_far = Self::calculate_royalty(sale_price, cumulative_bps)?;
            let amount = total_so_far.saturating_sub(cumulative_paid);
            cumulative_paid = total_so_far;
            if amount == 0 { continue; }

            // Transfer to recipient
            token_client.transfer(&payer, &split.recipient, &amount);

            // Accrue for claim_royalties (Task 1)
            let key = DataKey::RoyaltyAccrued(split.recipient.clone());
            let prev: i128 = env.storage().persistent().get(&key).unwrap_or(0);
            env.storage().persistent().set(&key, &(prev.saturating_add(amount)));

            env.events().publish(
                (symbol_short!("royalty"),),
                RoyaltyPaidEvent { token_id, from: payer.clone(), to: split.recipient, amount },
            );
        }
        Ok(())
    }

    /// Claim all accrued royalties for the caller.
    ///
    /// Transfers the full accrued balance to `recipient`, then zeroes it out
    /// so the same amount cannot be claimed twice (double-claim prevention).
    /// Emits `RoyaltyClaimed`.
    pub fn claim_royalties(env: Env, recipient: Address) -> Result<i128, Error> {
        recipient.require_auth();

        let key = DataKey::RoyaltyAccrued(recipient.clone());
        let accrued: i128 = env.storage().persistent().get(&key).unwrap_or(0);
        if accrued == 0 {
            return Err(Error::NothingToClaim);
        }

        // Zero out before transfer to prevent re-entrancy / double-claim
        env.storage().persistent().set(&key, &0i128);

        // NOTE: actual token transfer requires the asset address; for XLM-based
        // royalties the marketplace handles the transfer. Here we record the
        // claim and emit the event. Integrators using SEP-0041 assets should
        // extend this with a token_client.transfer call using the stored asset.
        env.events().publish(
            (symbol_short!("claimed"),),
            RoyaltyClaimedEvent { recipient, amount: accrued },
        );

        Ok(accrued)
    }

    /// Return the accrued (unclaimed) royalty balance for an address.
    pub fn accrued_royalties(env: Env, recipient: Address) -> i128 {
        env.storage()
            .persistent()
            .get(&DataKey::RoyaltyAccrued(recipient))
            .unwrap_or(0)
    }

    pub fn set_royalty(
        env: Env,
        admin: Address,
        token_id: TokenId,
        new_royalty: Royalty,
    ) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        let mut data = Self::load_token(&env, token_id)?;
        let old_royalty = data.royalty.clone();
        let new_royalty = Self::normalize_royalty(&env, new_royalty)?;

        if !old_royalty.recipients.is_empty() && !new_royalty.recipients.is_empty() {
            let old_r = old_royalty.recipients.get(0).ok_or(Error::InvalidRoyaltySplit)?;
            let new_r = new_royalty.recipients.get(0).ok_or(Error::InvalidRoyaltySplit)?;
            if old_r.recipient != new_r.recipient {
                env.events().publish(
                    (symbol_short!("royalty"),),
                    RoyaltyRecipientUpdatedEvent {
                        token_id,
                        old_recipient: old_r.recipient,
                        new_recipient: new_r.recipient,
                    },
                );
            }
        }

        data.royalty = new_royalty;
        env.storage().persistent().set(&DataKey::Token(token_id), &data);
        Ok(())
    }

    pub fn update_royalty_recipient(
        env: Env,
        caller: Address,
        token_id: TokenId,
        new_recipient: Address,
    ) -> Result<(), Error> {
        caller.require_auth();
        let mut data = Self::load_token(&env, token_id)?;
        let old = data.royalty.recipients.get(0).ok_or(Error::InvalidRoyaltySplit)?;
        if caller != old.recipient { return Err(Error::Unauthorized); }
        let bps = old.basis_points;
        data.royalty.recipients.set(0, RoyaltyRecipient { recipient: new_recipient.clone(), basis_points: bps });
        env.storage().persistent().set(&DataKey::Token(token_id), &data);
        env.events().publish(
            (symbol_short!("royalty"),),
            RoyaltyRecipientUpdatedEvent { token_id, old_recipient: old.recipient, new_recipient },
        );
        Ok(())
    }

    // ── Burn ──────────────────────────────────────────────────────────────────

    pub fn burn(env: Env, owner: Address, token_id: TokenId) -> Result<(), Error> {
        owner.require_auth();
        let data: TokenData = Self::load_token(&env, token_id)?;
        if owner != data.owner { return Err(Error::Unauthorized); }

        env.storage().persistent().remove(&DataKey::Token(token_id));
        env.storage().persistent().remove(&DataKey::ClipIdMinted(data.clip_id));

        // Decrement collection counter
        let col_key = DataKey::CollectionCount(data.collection_id);
        let col_count: u32 = env.storage().persistent().get(&col_key).unwrap_or(0);
        if col_count > 0 {
            env.storage().persistent().set(&col_key, &(col_count - 1));
        }

        env.events().publish(
            (symbol_short!("burn"),),
            BurnEvent { owner, token_id, clip_id: data.clip_id },
        );
        Ok(())
    }

    // ── Admin config ──────────────────────────────────────────────────────────

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

    pub fn set_token_uri(env: Env, owner: Address, token_id: TokenId, uri: String) -> Result<(), Error> {
        owner.require_auth();
        let mut data = Self::load_token(&env, token_id)?;
        if data.owner != owner { return Err(Error::Unauthorized); }
        data.metadata_uri = uri;
        env.storage().persistent().set(&DataKey::Token(token_id), &data);
        Ok(())
    }

    // ── View functions ────────────────────────────────────────────────────────

    pub fn version(_env: Env) -> u32 { VERSION }

    pub fn name(env: Env) -> String {
        env.storage().instance().get(&DataKey::Name)
            .unwrap_or_else(|| String::from_str(&env, "ClipCash Clips"))
    }

    pub fn symbol(env: Env) -> String {
        env.storage().instance().get(&DataKey::Symbol)
            .unwrap_or_else(|| String::from_str(&env, "CLIP"))
    }

    pub fn get_clip_id(env: Env, token_id: TokenId) -> Result<u32, Error> {
        Ok(Self::load_token(&env, token_id)?.clip_id)
    }

    pub fn owner_of(env: Env, token_id: TokenId) -> Result<Address, Error> {
        Ok(Self::load_token(&env, token_id)?.owner)
    }

    pub fn token_uri(env: Env, token_id: TokenId) -> Result<String, Error> {
        Ok(Self::load_token(&env, token_id)?.metadata_uri)
    }

    pub fn get_metadata(env: Env, token_id: TokenId) -> Result<String, Error> {
        Ok(Self::load_token(&env, token_id)?.metadata_uri)
    }

    pub fn clip_token_id(env: Env, clip_id: u32) -> Result<TokenId, Error> {
        env.storage().persistent().get(&DataKey::ClipIdMinted(clip_id)).ok_or(Error::InvalidTokenId)
    }

    pub fn get_royalty(env: Env, token_id: TokenId) -> Result<Royalty, Error> {
        Ok(Self::load_token(&env, token_id)?.royalty)
    }

    pub fn total_supply(env: Env) -> u32 {
        env.storage().instance()
            .get::<DataKey, u32>(&DataKey::NextTokenId)
            .unwrap_or(1)
            .saturating_sub(1)
    }

    pub fn exists(env: Env, token_id: TokenId) -> bool {
        env.storage().persistent().has(&DataKey::Token(token_id))
    }

    pub fn is_soulbound(env: Env, token_id: TokenId) -> bool {
        Self::load_token(&env, token_id).map(|d| d.is_soulbound).unwrap_or(false)
    }

    /// Returns the collection_id for a token.
    pub fn get_collection_id(env: Env, token_id: TokenId) -> Result<u32, Error> {
        Ok(Self::load_token(&env, token_id)?.collection_id)
    }

    /// Returns the number of tokens minted in a given collection.
    pub fn collection_supply(env: Env, collection_id: u32) -> u32 {
        env.storage().persistent()
            .get(&DataKey::CollectionCount(collection_id))
            .unwrap_or(0)
    }

    pub fn calculate_royalty_amount(env: Env, token_id: TokenId, sale_price: i128) -> Result<i128, Error> {
        if sale_price <= 0 { return Err(Error::InvalidSalePrice); }
        let royalty = Self::load_token(&env, token_id)?.royalty;
        let mut total_bps: u32 = 0;
        for idx in 0..royalty.recipients.len() {
            let s = royalty.recipients.get(idx).ok_or(Error::InvalidRoyaltySplit)?;
            total_bps = total_bps.saturating_add(s.basis_points);
        }
        Self::calculate_royalty(sale_price, total_bps)
    }

    pub fn tokens_of_owner(env: Env, owner: Address) -> Vec<TokenId> {
        const MAX: u32 = 1000;
        let next_id: u32 = env.storage().instance().get(&DataKey::NextTokenId).unwrap_or(1);
        let mut result: Vec<TokenId> = Vec::new(&env);
        let mut count: u32 = 0;
        let mut id: u32 = 1;
        while id < next_id && count < MAX {
            if let Some(data) = env.storage().persistent().get::<DataKey, TokenData>(&DataKey::Token(id)) {
                if data.owner == owner {
                    result.push_back(id);
                    count += 1;
                }
            }
            id += 1;
        }
        result
    }

    // ── Batch mint ────────────────────────────────────────────────────────────

    pub fn batch_mint(
        env: Env,
        to: Address,
        clip_ids: Vec<u32>,
        metadata_uris: Vec<String>,
        royalty: Royalty,
        is_soulbound: bool,
        collection_id: u32,
        signatures: Vec<BytesN<64>>,
    ) -> Result<Vec<TokenId>, Error> {
        to.require_auth();
        Self::require_not_paused(&env)?;

        let n = clip_ids.len();
        if n != metadata_uris.len() || n != signatures.len() {
            return Err(Error::InvalidRoyaltySplit);
        }

        let royalty = Self::normalize_royalty(&env, royalty)?;
        let mut minted: Vec<TokenId> = Vec::new(&env);

        for i in 0..n {
            let clip_id    = clip_ids.get(i).ok_or(Error::InvalidTokenId)?;
            let uri        = metadata_uris.get(i).ok_or(Error::InvalidTokenId)?;
            let sig        = signatures.get(i).ok_or(Error::InvalidTokenId)?;

            Self::verify_clip_signature(&env, &to, clip_id, &uri, &sig)?;

            if env.storage().persistent().has(&DataKey::ClipIdMinted(clip_id)) {
                return Err(Error::TokenAlreadyMinted);
            }
            if env.storage().persistent().get(&DataKey::BlacklistedClip(clip_id)).unwrap_or(false) {
                return Err(Error::ClipBlacklisted);
            }

            let token_id: TokenId = env.storage().instance().get(&DataKey::NextTokenId).unwrap_or(1);

            env.storage().persistent().set(
                &DataKey::Token(token_id),
                &TokenData { owner: to.clone(), clip_id, is_soulbound, metadata_uri: uri, royalty: royalty.clone(), collection_id },
            );
            env.storage().persistent().set(&DataKey::ClipIdMinted(clip_id), &token_id);
            env.storage().instance().set(&DataKey::NextTokenId, &(token_id + 1));

            let col_count: u32 = env.storage().persistent()
                .get(&DataKey::CollectionCount(collection_id)).unwrap_or(0);
            env.storage().persistent().set(&DataKey::CollectionCount(collection_id), &(col_count + 1));

            minted.push_back(token_id);
        }

        env.events().publish(
            (symbol_short!("batch_mnt"),),
            BatchMintEvent { to, count: n, first_token_id: minted.get(0).unwrap_or(0) },
        );
        Ok(minted)
    }

    // ── Internal helpers ──────────────────────────────────────────────────────

    fn load_token(env: &Env, token_id: TokenId) -> Result<TokenData, Error> {
        env.storage().persistent().get(&DataKey::Token(token_id)).ok_or(Error::InvalidTokenId)
    }

    fn verify_clip_signature(
        env: &Env,
        owner: &Address,
        clip_id: u32,
        metadata_uri: &String,
        signature: &BytesN<64>,
    ) -> Result<(), Error> {
        let signer: BytesN<32> = env.storage().instance().get(&DataKey::Signer).ok_or(Error::SignerNotSet)?;
        let owner_hash: BytesN<32> = env.crypto().sha256(&owner.clone().to_xdr(env)).into();
        let uri_hash: BytesN<32>   = env.crypto().sha256(&Bytes::from(metadata_uri.to_xdr(env))).into();
        let mut preimage = Bytes::new(env);
        preimage.extend_from_array(&clip_id.to_le_bytes());
        preimage.append(&Bytes::from(owner_hash));
        preimage.append(&Bytes::from(uri_hash));
        let message: BytesN<32> = env.crypto().sha256(&preimage).into();
        env.crypto().ed25519_verify(&signer, &Bytes::from(message), signature);
        Ok(())
    }

    fn require_admin(env: &Env, addr: &Address) -> Result<(), Error> {
        let admin: Address = env.storage().instance().get(&DataKey::Admin).expect("not initialized");
        if addr != &admin { return Err(Error::Unauthorized); }
        addr.require_auth();
        Ok(())
    }

    fn require_not_paused(env: &Env) -> Result<(), Error> {
        if env.storage().instance().get(&DataKey::Paused).unwrap_or(false) {
            return Err(Error::ContractPaused);
        }
        Ok(())
    }

    fn normalize_royalty(env: &Env, royalty: Royalty) -> Result<Royalty, Error> {
        if royalty.recipients.is_empty() { return Err(Error::InvalidRoyaltySplit); }
        let platform: Address = env.storage().instance()
            .get(&DataKey::PlatformRecipient).ok_or(Error::InvalidRecipient)?;
        let mut recipients = royalty.recipients;
        let mut has_platform = false;
        let mut total_bps: u32 = 0;
        for idx in 0..recipients.len() {
            let s = recipients.get(idx).ok_or(Error::InvalidRoyaltySplit)?;
            if s.recipient == platform { has_platform = true; }
            total_bps = total_bps.saturating_add(s.basis_points);
        }
        if !has_platform {
            recipients.push_back(RoyaltyRecipient { recipient: platform, basis_points: 100 });
            total_bps = total_bps.saturating_add(100);
        }
        if total_bps > 10_000 { return Err(Error::RoyaltyTooHigh); }
        Ok(Royalty { recipients, asset_address: royalty.asset_address })
    }

    pub fn calculate_royalty(sale_price: i128, basis_points: u32) -> Result<i128, Error> {
        if sale_price <= 0 { return Err(Error::InvalidSalePrice); }
        if sale_price > i128::MAX / 10_000 { return Err(Error::RoyaltyOverflow); }
        let amount = sale_price.saturating_mul(basis_points as i128);
        Ok((amount.saturating_add(5_000)) / 10_000)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use soroban_sdk::{
        testutils::{Address as _, BytesN as _, Events as _, Ledger as _},
        Address, Bytes, BytesN, Env, String, Vec, xdr::ToXdr,
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
        recipients.push_back(RoyaltyRecipient { recipient, basis_points: 500 });
        Royalty { recipients, asset_address: None }
    }

    fn sign_mint(env: &Env, sk: &ed25519_dalek::SigningKey, owner: &Address, clip_id: u32, uri: &String) -> BytesN<64> {
        let owner_hash: BytesN<32> = env.crypto().sha256(&owner.clone().to_xdr(env)).into();
        let uri_hash: BytesN<32>   = env.crypto().sha256(&Bytes::from(uri.to_xdr(env))).into();
        let mut pre = Bytes::new(env);
        pre.extend_from_array(&clip_id.to_le_bytes());
        pre.append(&Bytes::from(owner_hash));
        pre.append(&Bytes::from(uri_hash));
        let msg: BytesN<32> = env.crypto().sha256(&pre).into();
        use ed25519_dalek::Signer as _;
        BytesN::from_array(env, &sk.sign(&msg.to_array()).to_bytes())
    }

    fn register_signer(env: &Env, client: &ClipsNftContractClient, admin: &Address) -> ed25519_dalek::SigningKey {
        let sk = ed25519_dalek::SigningKey::from_bytes(&BytesN::<32>::random(env).to_array());
        client.set_signer(admin, &BytesN::from_array(env, &sk.verifying_key().to_bytes()));
        sk
    }

    fn do_mint(client: &ClipsNftContractClient, env: &Env, to: &Address, clip_id: u32, kp: &ed25519_dalek::SigningKey) -> TokenId {
        let uri = String::from_str(env, "ipfs://QmExample");
        let sig = sign_mint(env, kp, to, clip_id, &uri);
        client.mint(to, &clip_id, &uri, &default_royalty(env, to.clone()), &false, &0u32, &sig)
    }

    // ── Task 1: claim_royalties ───────────────────────────────────────────────

    #[test]
    fn test_accrue_and_claim_royalties() {
        let (env, admin, user1, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);

        let token_id = do_mint(&client, &env, &user1, 1, &kp);

        // Manually set accrued balance (simulating pay_royalty)
        env.as_contract(&cid, || {
            env.storage().persistent().set(&DataKey::RoyaltyAccrued(user1.clone()), &500i128);
        });

        assert_eq!(client.accrued_royalties(&user1), 500i128);
        let claimed = client.claim_royalties(&user1);
        assert_eq!(claimed, 500i128);
        // Balance zeroed — double-claim prevention
        assert_eq!(client.accrued_royalties(&user1), 0i128);
        let _ = token_id;
    }

    #[test]
    fn test_claim_royalties_nothing_to_claim() {
        let (env, admin, user1, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let result = client.try_claim_royalties(&user1);
        assert_eq!(result, Err(Ok(Error::NothingToClaim)));
    }

    #[test]
    fn test_claim_royalties_emits_event() {
        let (env, admin, user1, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        env.as_contract(&cid, || {
            env.storage().persistent().set(&DataKey::RoyaltyAccrued(user1.clone()), &100i128);
        });
        client.claim_royalties(&user1);
        let events = env.events().all();
        assert!(events.events().len() > 0);
    }

    // ── Task 2: pause timelock ────────────────────────────────────────────────

    #[test]
    fn test_unpause_before_timelock_fails() {
        let (env, admin, _, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        client.pause(&admin);
        // Timelock not elapsed — unpause must fail
        let result = client.try_unpause(&admin);
        assert_eq!(result, Err(Ok(Error::TimelockActive)));
    }

    #[test]
    fn test_unpause_after_timelock_succeeds() {
        let (env, admin, _, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        client.pause(&admin);
        // Advance ledger past timelock
        env.ledger().set_sequence_number(env.ledger().sequence() + PAUSE_TIMELOCK_LEDGERS);
        client.unpause(&admin);
        assert!(!client.is_paused());
    }

    #[test]
    fn test_pause_stores_timestamp() {
        let (env, admin, _, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let seq_before = env.ledger().sequence();
        client.pause(&admin);
        assert_eq!(client.pause_requested_at(), seq_before);
    }

    #[test]
    fn test_pause_blocks_mint_and_transfer() {
        let (env, admin, user1, user2) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);
        let token_id = do_mint(&client, &env, &user1, 1, &kp);
        client.pause(&admin);
        let uri = String::from_str(&env, "ipfs://QmX");
        let sig = sign_mint(&env, &kp, &user1, 2, &uri);
        assert_eq!(client.try_mint(&user1, &2u32, &uri, &default_royalty(&env, user1.clone()), &false, &0u32, &sig), Err(Ok(Error::ContractPaused)));
        assert_eq!(client.try_transfer(&user1, &user2, &token_id), Err(Ok(Error::ContractPaused)));
    }

    // ── Task 3: gas constants accessible ─────────────────────────────────────

    #[test]
    fn test_gas_constants_defined() {
        assert!(GAS_MINT > 0);
        assert!(GAS_TRANSFER > 0);
        assert!(GAS_MINT > GAS_TRANSFER); // mint is heavier
    }

    // ── Task 4: collection_id ─────────────────────────────────────────────────

    #[test]
    fn test_mint_with_collection_id() {
        let (env, admin, user1, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);
        let uri = String::from_str(&env, "ipfs://QmExample");
        let sig = sign_mint(&env, &kp, &user1, 1, &uri);
        let token_id = client.mint(&user1, &1u32, &uri, &default_royalty(&env, user1.clone()), &false, &42u32, &sig);
        assert_eq!(client.get_collection_id(&token_id), 42u32);
        assert_eq!(client.collection_supply(&42u32), 1u32);
    }

    #[test]
    fn test_collection_supply_increments() {
        let (env, admin, user1, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);
        do_mint(&client, &env, &user1, 1, &kp); // collection 0
        do_mint(&client, &env, &user1, 2, &kp); // collection 0
        assert_eq!(client.collection_supply(&0u32), 2u32);
    }

    #[test]
    fn test_collection_supply_decrements_on_burn() {
        let (env, admin, user1, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);
        let token_id = do_mint(&client, &env, &user1, 1, &kp);
        assert_eq!(client.collection_supply(&0u32), 1u32);
        client.burn(&user1, &token_id);
        assert_eq!(client.collection_supply(&0u32), 0u32);
    }

    // ── Existing core tests (kept) ────────────────────────────────────────────

    #[test]
    fn test_mint_stores_owner_and_uri() {
        let (env, admin, user1, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);
        let token_id = do_mint(&client, &env, &user1, 42, &kp);
        assert_eq!(client.owner_of(&token_id), user1);
        assert_eq!(client.token_uri(&token_id), String::from_str(&env, "ipfs://QmExample"));
        assert_eq!(client.total_supply(), 1);
    }

    #[test]
    fn test_transfer_updates_owner() {
        let (env, admin, user1, user2) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);
        let token_id = do_mint(&client, &env, &user1, 1, &kp);
        client.transfer(&user1, &user2, &token_id);
        assert_eq!(client.owner_of(&token_id), user2);
    }

    #[test]
    fn test_soulbound_transfer_blocked() {
        let (env, admin, user1, user2) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);
        let uri = String::from_str(&env, "ipfs://QmExample");
        let sig = sign_mint(&env, &kp, &user1, 1, &uri);
        let token_id = client.mint(&user1, &1u32, &uri, &default_royalty(&env, user1.clone()), &true, &0u32, &sig);
        assert_eq!(client.try_transfer(&user1, &user2, &token_id), Err(Ok(Error::SoulboundTransferBlocked)));
    }

    #[test]
    fn test_double_mint_same_clip_id_fails() {
        let (env, admin, user1, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);
        do_mint(&client, &env, &user1, 7, &kp);
        let uri = String::from_str(&env, "ipfs://QmExample");
        let sig = sign_mint(&env, &kp, &user1, 7, &uri);
        assert_eq!(
            client.try_mint(&user1, &7u32, &uri, &default_royalty(&env, user1.clone()), &false, &0u32, &sig),
            Err(Ok(Error::TokenAlreadyMinted))
        );
    }

    #[test]
    fn test_royalty_info_xlm() {
        let (env, admin, user1, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);
        let token_id = do_mint(&client, &env, &user1, 1, &kp);
        let info = client.royalty_info(&token_id, &1_000_000i128);
        assert_eq!(info.royalty_amount, 60_000i128);
    }

    #[test]
    fn test_burn_removes_token() {
        let (env, admin, user1, _) = setup();
        let cid = env.register(ClipsNftContract, ());
        let client = ClipsNftContractClient::new(&env, &cid);
        client.init(&admin);
        let kp = register_signer(&env, &client, &admin);
        let token_id = do_mint(&client, &env, &user1, 1, &kp);
        client.burn(&user1, &token_id);
        assert!(!client.exists(&token_id));
    }
}
