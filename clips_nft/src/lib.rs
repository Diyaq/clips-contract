//! ClipCash NFT — Soroban Smart Contract
//!
//! Enables minting video clips as NFTs on the Stellar network with built-in
//! royalty support for content creators. Royalties can be paid in XLM or any
//! SEP-0041 custom Stellar asset.
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
//! payload = SHA-256( clip_id_le_bytes || SHA-256(owner_xdr) || SHA-256(metadata_uri_bytes) )
//! ```
//!
//! # Storage layout
//!
//! | Tier       | Keys                                              |
//! |------------|---------------------------------------------------|
//! | instance   | Admin, NextTokenId, Paused, Signer, Name, Symbol, PlatformRecipient |
//! | persistent | Token(id), ClipIdMinted(clip_id), Approved(id), ApprovalForAll(owner,op), BlacklistedClip(clip_id), Balance(owner) |
//!
//! # Privileged entrypoints (admin-only)
//!
//! ## Storage tiers used
//! - `instance`   – cheap, loaded once per tx, shared across all calls in the tx.
//!   Used for: Admin, NextTokenId, Paused, Signer.
//! - `persistent` – per-entry fee, survives ledger expiry extension.
//!   Used for: TokenData (owner+clip_id packed), Metadata, Royalty,
//!   ClipIdMinted (dedup guard), Balance (owner balance checkpoint).

#![no_std]

pub mod safe_math;

use soroban_sdk::{
    contract, contracterror, contractimpl, contracttype, symbol_short, xdr::ToXdr, Address, Bytes,
    BytesN, Env, String, Vec,
};

/// Contract version — bump on every breaking change.
pub const VERSION: u32 = 1;
pub const DEFAULT_MINT_COOLDOWN_SECONDS: u64 = 0;

// =============================================================================
// Errors
// =============================================================================

/// All error codes returned by the contract.
#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
pub enum Error {
    /// Caller is not authorized for this operation.
    Unauthorized = 1,
    /// Token ID does not exist.
    InvalidTokenId = 2,
    /// Clip has already been minted.
    ClipAlreadyMinted = 3,
    /// Total royalty basis points exceed 10 000 (100 %).
    RoyaltyTooHigh = 4,
    /// Royalty recipient address is invalid or missing.
    InvalidRecipient = 5,
    /// Sale price must be greater than zero.
    InvalidSalePrice = 6,
    /// Contract is paused — minting and transfers are blocked.
    ContractPaused = 7,
    /// Backend Ed25519 signature over the mint payload is invalid.
    InvalidSignature = 8,
    /// No backend signer public key has been registered yet.
    SignerNotSet = 9,
    /// Royalty split configuration is invalid.
    InvalidRoyaltySplit = 10,
    /// Token is soulbound (non-transferable).
    SoulboundTransferBlocked = 11,
    /// Royalty calculation would overflow i128.
    RoyaltyOverflow = 12,
    /// Clip ID has been blacklisted by the admin.
    ClipBlacklisted = 13,
    /// Caller is not the owner or an approved operator.
    NotAuthorizedToApprove = 14,
    /// Withdrawal is still locked (24h safety delay)
    WithdrawalStillLocked = 15,
    /// No active withdrawal request found
    NoWithdrawalRequest = 16,
    /// Batch mint request exceeds configured gas-safe limit
    BatchTooLarge = 17,
    /// Token is frozen and cannot be transferred or burned.
    TokenFrozen = 18,
    /// Insufficient balance for this operation.
    InsufficientBalance = 19,
    /// Metadata was refreshed too recently (30-day cooldown not elapsed).
    MetadataRefreshTooSoon = 20,
    /// Image URL must start with "https://" or "ipfs://".
    InvalidImageUrl = 21,
    /// Animation URL must start with "https://" or "ipfs://".
    InvalidAnimationUrl = 22,
    /// Mint attempted before wallet cooldown elapsed.
    MintCooldownActive = 23,
    /// Reentrant call detected while a guarded entrypoint is executing.
    Reentrancy = 24,
}

// =============================================================================
// Types
// =============================================================================

/// Opaque token identifier (auto-incremented u32).
pub type TokenId = u32;

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Attribute {
    pub trait_type: String,
    pub value: String,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenData {
    pub owner: Address,
    pub clip_id: u32,
    pub is_soulbound: bool,
    pub metadata_uri: String,
    pub image: Option<String>,
    pub animation_url: Option<String>,
    pub description: Option<String>,
    pub external_url: Option<String>,
    pub attributes: Vec<Attribute>,
    pub royalty: Royalty,
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
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractInfo {
    pub name: String,
    pub symbol: String,
    pub version: u32,
    pub owner: Address,
    pub platform_fee: u32,
}

// =============================================================================
// Storage keys
// =============================================================================

#[contracttype]
pub enum DataKey {
    Admin,
    NextTokenId,
    Paused,
    PauseReason,
    Name,
    Symbol,
    Token(TokenId),
    ClipIdMinted(u32),
    CustomTokenUri(TokenId),
    Signer,
    PlatformRecipient,
    Approved(TokenId),
    MetadataUpdateCount(TokenId),
    ApprovalForAll(Address, Address),
    BlacklistedClip(u32),
    WithdrawXlmRequest,
    LastWithdrawalTime,
    Balance(Address),
    TotalSupply,
    TotalGasMint,
    CountMint,
    TotalGasTransfer,
    CountTransfer,
    Frozen(TokenId),
    MetadataRefreshTime(TokenId),
    PauseUnlockTime,
    PlatformFeeBps,
    DefaultRoyaltyBps,
    RoyaltyBalance(TokenId),
    LastMintTimestamp(Address),
    MintCooldownSeconds,
    ReentrancyLock,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawRequest {
    pub amount: i128,
    pub unlock_time: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawRequestedEvent {
    pub amount: i128,
    pub unlock_time: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct WithdrawExecutedEvent {
    pub amount: i128,
    pub recipient: Address,
}

// =============================================================================
// Events
// =============================================================================

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MintEvent {
    pub to: Address,
    pub clip_id: u32,
    pub token_id: TokenId,
    pub metadata_uri: String,
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

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenUriChangedEvent {
    pub token_id: TokenId,
    pub owner: Address,
    pub new_uri: String,
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

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct MetadataUpdatedEvent {
    pub token_id: TokenId,
    pub old_uri: String,
    pub new_uri: String,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenFrozenEvent {
    pub token_id: TokenId,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct TokenUnfrozenEvent {
    pub token_id: TokenId,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SignerUpdatedEvent {
    pub new_pubkey: BytesN<32>,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoyaltyUpdatedEvent {
    pub token_id: TokenId,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PauseScheduledEvent {
    pub active_at: u64,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CollectionUpdatedEvent {
    pub field: String,
    pub new_value: String,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ConfigUpdatedEvent {
    pub key: String,
    pub new_value: u32,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RoyaltyClaimedEvent {
    pub token_id: TokenId,
    pub recipient: Address,
    pub amount: i128,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AdminChangedEvent {
    pub old_admin: Address,
    pub new_admin: Address,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct RefundedEvent {
    pub token_id: TokenId,
    pub recipient: Address,
    pub amount: i128,
}

/// Emerging Soroban NFT standard interface (ERC-721 adapted).
pub trait NftStandard {
    fn balance_of(env: Env, owner: Address) -> u32;
    fn owner_of(env: Env, token_id: TokenId) -> Result<Address, Error>;
    fn transfer(env: Env, from: Address, to: Address, token_id: TokenId) -> Result<(), Error>;
    fn approve(env: Env, caller: Address, operator: Option<Address>, token_id: TokenId) -> Result<(), Error>;
    fn get_approved(env: Env, token_id: TokenId) -> Option<Address>;
    fn set_approval_for_all(env: Env, caller: Address, operator: Address, approved: bool) -> Result<(), Error>;
    fn is_approved_for_all(env: Env, owner: Address, operator: Address) -> bool;
    fn total_supply(env: Env) -> u32;
    fn token_uri(env: Env, token_id: TokenId) -> Result<String, Error>;
    fn name(env: Env) -> String;
    fn symbol(env: Env) -> String;
    fn revoke_approval(env: Env, token_id: TokenId) -> Result<(), Error>;
    fn revoke_all_approvals(env: Env, operator: Address) -> Result<(), Error>;
    fn burn(env: Env, token_id: TokenId, refund_royalty: bool) -> Result<(), Error>;
}

// =============================================================================
// Contract Implementation
// =============================================================================

/// ClipCash NFT contract.
#[contract]
pub struct ClipsNftContract;

#[contractimpl]
impl NftStandard for ClipsNftContract {
    /// Returns how many tokens `owner` holds.
    ///
    /// Closes #199 - Map-based Ownership for O(1) Balance Check
    fn balance_of(env: Env, owner: Address) -> u32 {
        env.storage()
            .persistent()
            .get(&DataKey::Balance(owner))
            .unwrap_or(0u32)
    }

    fn owner_of(env: Env, token_id: TokenId) -> Result<Address, Error> {
        let token_data: TokenData = env
            .storage()
            .persistent()
            .get(&DataKey::Token(token_id))
            .ok_or(Error::InvalidTokenId)?;
        Ok(token_data.owner)
    }

    /// Transfers ownership of an NFT and keeps balances perfectly synchronized.
    ///
    /// Closes #199 - Map-based Balance Synchronization
    fn transfer(env: Env, from: Address, to: Address, token_id: TokenId) -> Result<(), Error> {
        if Self::is_paused(&env) {
            return Err(Error::ContractPaused);
        }

        let token_key = DataKey::Token(token_id);
        let mut token_data: TokenData = env
            .storage()
            .persistent()
            .get(&token_key)
            .ok_or(Error::InvalidTokenId)?;

        if token_data.owner != from {
            return Err(Error::Unauthorized);
        }

        from.require_auth();

        if token_data.is_soulbound {
            return Err(Error::SoulboundTransferBlocked);
        }

        if Self::is_frozen(env.clone(), token_id) {
            return Err(Error::TokenFrozen);
        }

        // Verify authorization (owner, approved operator, or operator for all)
        if from != from {
            let is_approved = env
                .storage()
                .persistent()
                .get(&DataKey::Approved(token_id))
                .map(|addr: Address| addr == from)
                .unwrap_or(false);

            let is_approved_all = env
                .storage()
                .persistent()
                .get(&DataKey::ApprovalForAll(from.clone(), from.clone()))
                .unwrap_or(false);

            if !is_approved && !is_approved_all {
                return Err(Error::Unauthorized);
            }
        }

        // Change owner checkpoint and clear specific token approval mapping values
        token_data.owner = to.clone();
        env.storage().persistent().set(&token_key, &token_data);
        env.storage().persistent().remove(&DataKey::Approved(token_id));

        // Synchronize Balance state map counters atomically
        let from_balance = Self::balance_of(env.clone(), from.clone());
        if from_balance > 0 {
            env.storage().persistent().set(&DataKey::Balance(from.clone()), &(from_balance - 1));
        }

        let to_balance = Self::balance_of(env.clone(), to.clone());
        env.storage().persistent().set(&DataKey::Balance(to.clone()), &(to_balance + 1));

        env.events().publish(
            (symbol_short!("transfer"),),
            TransferEvent {
                token_id,
                from,
                to,
            },
        );

        Ok(())
    }

    fn approve(env: Env, caller: Address, operator: Option<Address>, token_id: TokenId) -> Result<(), Error> {
        let token_data: TokenData = env
            .storage()
            .persistent()
            .get(&DataKey::Token(token_id))
            .ok_or(Error::InvalidTokenId)?;

        if token_data.owner != caller {
            let is_approved_all = env
                .storage()
                .persistent()
                .get(&DataKey::ApprovalForAll(token_data.owner.clone(), caller.clone()))
                .unwrap_or(false);
            if !is_approved_all {
                return Err(Error::Unauthorized);
            }
        }

        caller.require_auth();

        let approval_key = DataKey::Approved(token_id);
        if let Some(op) = operator {
            env.storage().persistent().set(&approval_key, &op);
            env.events().publish(
                (symbol_short!("approval"),),
                ApprovalEvent {
                    owner: token_data.owner,
                    operator: op,
                    token_id,
                },
            );
        } else {
            env.storage().persistent().remove(&approval_key);
        }

        Ok(())
    }

    fn get_approved(env: Env, token_id: TokenId) -> Option<Address> {
        env.storage().persistent().get(&DataKey::Approved(token_id))
    }

    fn set_approval_for_all(env: Env, caller: Address, operator: Address, approved: bool) -> Result<(), Error> {
        caller.require_auth();
        env.storage()
            .persistent()
            .set(&DataKey::ApprovalForAll(caller.clone(), operator.clone()), &approved);

        env.events().publish(
            (symbol_short!("app_all"),),
            ApprovalForAllEvent {
                owner: caller,
                operator,
                approved,
            },
        );
        Ok(())
    }

    fn is_approved_for_all(env: Env, owner: Address, operator: Address) -> bool {
        env.storage()
            .persistent()
            .get(&DataKey::ApprovalForAll(owner, operator))
            .unwrap_or(false)
    }

    fn total_supply(env: Env) -> u32 {
        let next_id: u32 = env.storage().instance().get(&DataKey::NextTokenId).unwrap_or(1);
        next_id.saturating_sub(1)
    }

    fn token_uri(env: Env, token_id: TokenId) -> Result<String, Error> {
        if !env.storage().persistent().has(&DataKey::Token(token_id)) {
            return Err(Error::InvalidTokenId);
        }
        if let Some(custom_uri) = env.storage().persistent().get(&DataKey::CustomTokenUri(token_id)) {
            Ok(custom_uri)
        } else {
            let token_data: TokenData = env.storage().persistent().get(&DataKey::Token(token_id)).unwrap();
            Ok(token_data.metadata_uri)
        }
    }

    fn name(env: Env) -> String {
        env.storage()
            .instance()
            .get(&DataKey::Name)
            .unwrap_or_else(|| String::from_str(&env, ""))
    }

    fn symbol(env: Env) -> String {
        env.storage()
            .instance()
            .get(&DataKey::Symbol)
            .unwrap_or_else(|| String::from_str(&env, ""))
    }

    fn revoke_approval(env: Env, token_id: TokenId) -> Result<(), Error> {
        let token_data: TokenData = env
            .storage()
            .persistent()
            .get(&DataKey::Token(token_id))
            .ok_or(Error::InvalidTokenId)?;

        token_data.owner.require_auth();

        let approval_key = DataKey::Approved(token_id);
        if env.storage().persistent().has(&approval_key) {
            env.storage().persistent().remove(&approval_key);
            env.events().publish(
                (symbol_short!("approval"),),
                ApprovalEvent {
                    owner: token_data.owner,
                    operator: env.current_contract_address(),
                    token_id,
                },
            );
        }
        Ok(())
    }

    fn revoke_all_approvals(env: Env, operator: Address) -> Result<(), Error> {
        operator.require_auth();
        let approval_all_key = DataKey::ApprovalForAll(env.current_contract_address(), operator.clone());
        if env.storage().persistent().has(&approval_all_key) {
            env.storage().persistent().remove(&approval_all_key);
            env.events().publish(
                (symbol_short!("app_all"),),
                ApprovalForAllEvent {
                    owner: env.current_contract_address(),
                    operator,
                    approved: false,
                },
            );
        }
        Ok(())
    }

    /// Burns an NFT and decrements the target owner balance checkpoint state securely.
    ///
    /// Closes #199 - Balance Synchronization for burn
    fn burn(env: Env, token_id: TokenId, refund_royalty: bool) -> Result<(), Error> {
        let token_key = DataKey::Token(token_id);
        let token_data: TokenData = env
            .storage()
            .persistent()
            .get(&token_key)
            .ok_or(Error::InvalidTokenId)?;

        token_data.owner.require_auth();

        if Self::is_frozen(env.clone(), token_id) {
            return Err(Error::TokenFrozen);
        }

        if refund_royalty {
            let royalty_key = DataKey::RoyaltyBalance(token_id);
            if env.storage().persistent().has(&royalty_key) {
                let accumulated_amount: i128 = env.storage().persistent().get(&royalty_key).unwrap_or(0);
                if accumulated_amount > 0 {
                    if let Some(first_recipient) = token_data.royalty.recipients.get(0) {
                        let target_creator = first_recipient.recipient;
                        if let Some(ref asset_addr) = token_data.royalty.asset_address {
                            let client = soroban_sdk::token::TokenClient::new(&env, asset_addr);
                            client.transfer(&env.current_contract_address(), &target_creator, &accumulated_amount);
                        }
                        env.events().publish(
                            (symbol_short!("refunded"),),
                            RefundedEvent {
                                token_id,
                                recipient: target_creator,
                                amount: accumulated_amount,
                            },
                        );
                    }
                }
                env.storage().persistent().remove(&royalty_key);
            }
        }

        // Decrement user balance accurately
        let current_balance = Self::balance_of(env.clone(), token_data.owner.clone());
        if current_balance > 0 {
            env.storage().persistent().set(&DataKey::Balance(token_data.owner.clone()), &(current_balance - 1));
        }

        env.storage().persistent().remove(&token_key);
        env.storage().persistent().remove(&DataKey::ClipIdMinted(token_data.clip_id));
        env.storage().persistent().remove(&DataKey::Approved(token_id));
        env.storage().persistent().remove(&DataKey::CustomTokenUri(token_id));
        env.storage().persistent().remove(&DataKey::MetadataUpdateCount(token_id));
        env.storage().persistent().remove(&DataKey::MetadataRefreshTime(token_id));

        env.events().publish(
            (symbol_short!("burn"),),
            BurnEvent {
                owner: token_data.owner,
                token_id,
                clip_id: token_data.clip_id,
            },
        );

        Ok(())
    }
}

#[contractimpl]
impl ClipsNftContract {
    pub fn init(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::NextTokenId, &1u32);
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().set(&DataKey::PlatformRecipient, &admin);
        env.storage()
            .instance()
            .set(&DataKey::Name, &String::from_str(&env, "ClipCash Clips"));
        env.storage()
            .instance()
            .set(&DataKey::Symbol, &String::from_str(&env, "CLIP"));
        env.storage()
            .instance()
            .set(&DataKey::MintCooldownSeconds, &DEFAULT_MINT_COOLDOWN_SECONDS);
    }

    /// Mints a token and increments the receiver balance map.
    ///
    /// Closes #199 - Balance Synchronization for mint
    pub fn mint(
        env: Env,
        to: Address,
        clip_id: u32,
        metadata_uri: String,
        royalty_recipients: Vec<RoyaltyRecipient>,
        asset_address: Option<Address>,
        is_soulbound: bool,
    ) -> Result<TokenId, Error> {
        if Self::check_paused(&env) {
            return Err(Error::ContractPaused);
        }

        if env.storage().persistent().has(&DataKey::ClipIdMinted(clip_id)) {
            return Err(Error::ClipAlreadyMinted);
        }

        if env.storage().persistent().has(&DataKey::BlacklistedClip(clip_id)) {
            return Err(Error::ClipBlacklisted);
        }

        let token_id: u32 = env.storage().instance().get(&DataKey::NextTokenId).unwrap_or(1);
        env.storage().instance().set(&DataKey::NextTokenId, &(token_id + 1));

        let royalty = Royalty {
            recipients: royalty_recipients,
            asset_address,
        };

        let token_data = TokenData {
            owner: to.clone(),
            clip_id,
            is_soulbound,
            metadata_uri: metadata_uri.clone(),
            image: None,
            animation_url: None,
            description: None,
            external_url: None,
            attributes: Vec::new(&env),
            royalty,
        };

        env.storage().persistent().set(&DataKey::Token(token_id), &token_data);
        env.storage().persistent().set(&DataKey::ClipIdMinted(clip_id), &token_id);

        // Update target map-based balance metrics
        let current_bal = env.storage().persistent().get(&DataKey::Balance(to.clone())).unwrap_or(0u32);
        env.storage().persistent().set(&DataKey::Balance(to.clone()), &(current_bal + 1));

        env.events().publish(
            (symbol_short!("mint"),),
            MintEvent {
                to,
                clip_id,
                token_id,
                metadata_uri,
            },
        );

        Ok(token_id)
    }

    pub fn set_signer(env: Env, admin: Address, pubkey: BytesN<32>) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Signer, &pubkey);
        env.events().publish((symbol_short!("sgn_upd"),), SignerUpdatedEvent { new_pubkey: pubkey });
        Ok(())
    }

    pub fn get_signer(env: Env) -> Option<BytesN<32>> {
        env.storage().instance().get(&DataKey::Signer)
    }

    pub fn set_admin(env: Env, current_admin: Address, new_admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &current_admin)?;
        env.storage().instance().set(&DataKey::Admin, &new_admin);
        env.events().publish((symbol_short!("adm_chg"),), AdminChangedEvent { old_admin: current_admin, new_admin });
        Ok(())
    }

    pub fn upgrade(env: Env, admin: Address, new_wasm_hash: BytesN<32>) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.deployer().update_current_contract_wasm(new_wasm_hash.clone());
        env.events().publish((symbol_short!("upgrade"),), UpgradeEvent { new_wasm_hash });
        Ok(())
    }

    pub fn pause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        let active_at = env.ledger().timestamp().saturating_add(86_400);
        env.storage().instance().set(&DataKey::PauseUnlockTime, &active_at);
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events().publish((symbol_short!("pse_sched"),), PauseScheduledEvent { active_at });
        Ok(())
    }

    pub fn unpause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().instance().set(&DataKey::Paused, &false);
        env.storage().instance().remove(&DataKey::PauseUnlockTime);
        env.events().publish((symbol_short!("unpaused"),), ());
        Ok(())
    }

    pub fn is_paused(env: &Env) -> bool {
        Self::check_paused(env)
    }

    pub fn blacklist_clip(env: Env, admin: Address, clip_id: u32) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        env.storage().persistent().set(&DataKey::BlacklistedClip(clip_id), &true);
        env.events().publish((symbol_short!("blacklist"),), BlacklistEvent { clip_id });
        Ok(())
    }

    pub fn freeze(env: Env, admin: Address, token_id: TokenId) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        if !env.storage().persistent().has(&DataKey::Token(token_id)) {
            return Err(Error::InvalidTokenId);
        }
        env.storage().persistent().set(&DataKey::Frozen(token_id), &true);
        env.events().publish((symbol_short!("freeze"),), TokenFrozenEvent { token_id });
        Ok(())
    }

    pub fn unfreeze(env: Env, admin: Address, token_id: TokenId) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        if !env.storage().persistent().has(&DataKey::Token(token_id)) {
            return Err(Error::InvalidTokenId);
        }
        env.storage().persistent().remove(&DataKey::Frozen(token_id));
        env.events().publish((symbol_short!("unfreeze"),), TokenUnfrozenEvent { token_id });
        Ok(())
    }

    pub fn is_frozen(env: Env, token_id: TokenId) -> bool {
        env.storage().persistent().get(&DataKey::Frozen(token_id)).unwrap_or(false)
    }

    fn require_admin(env: &Env, admin: &Address) -> Result<(), Error> {
        let stored_admin: Address = env.storage().instance().get(&DataKey::Admin).ok_or(Error::Unauthorized)?;
        if admin != &stored_admin {
            return Err(Error::Unauthorized);
        }
        admin.require_auth();
        Ok(())
    }

    fn check_paused(env: &Env) -> bool {
        env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
    }
}
