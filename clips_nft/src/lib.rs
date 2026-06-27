//! ClipCashNFT — Soroban smart contract entry point.
//!
//! This module is the single gateway for all public contract methods.
//! It re-exports types and registers the contract implementation
//! via the `#[contract]` / `#[contractimpl]` macros.

#![no_std]

mod config;
mod default_royalty;
mod platform_fee;
mod types;

pub use config::{get_config, set_config, Config, CONTRACT_VERSION};
pub use default_royalty::{
    get_default_royalty_bps, set_default_royalty_bps, DEFAULT_ROYALTY_BPS, MAX_ROYALTY_BPS,
};
pub use platform_fee::{get_platform_fee, set_platform_fee, MAX_PLATFORM_FEE_BPS};
pub use types::{DataKey, Error, MintEvent, Royalty, RoyaltyInfo, TokenData, TokenId};

use soroban_sdk::{
    contract, contractimpl, BytesN, Env, String,
    Address,
};

#[contract]
pub struct ClipCashNFT;

#[contractimpl]
impl ClipCashNFT {
    // ─── Initialization ──────────────────────────────────────────────────────

    /// Initialize the contract and set the admin.
    pub fn init(env: Env, admin: Address) {
        if env.storage().instance().has(&DataKey::Admin) {
            panic!("already initialized");
        }
        admin.require_auth();
        env.storage().instance().set(&DataKey::Admin, &admin);
        env.storage().instance().set(&DataKey::NextTokenId, &0u32);
        env.storage().instance().set(&DataKey::Paused, &false);
    }

    // ─── Config ───────────────────────────────────────────────────────────────

    /// Persist a full [`Config`] snapshot. Admin only.
    pub fn set_config(env: Env, admin: Address, cfg: Config) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        admin.require_auth();
        config::set_config(&env, cfg)
    }

    /// Return the current [`Config`], or `None` before initialization.
    pub fn get_config(env: Env) -> Option<Config> {
        config::get_config(&env)
    }

    // ─── Default Royalty ─────────────────────────────────────────────────────

    /// Set the contract-wide default royalty in basis points (max 10 000 = 100 %).
    /// Admin only.
    pub fn set_default_royalty_bps(env: Env, admin: Address, bps: u32) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        admin.require_auth();
        default_royalty::set_default_royalty_bps(&env, bps)
    }

    /// Return the default royalty in basis points (defaults to 500 = 5 %).
    pub fn get_default_royalty_bps(env: Env) -> u32 {
        default_royalty::get_default_royalty_bps(&env)
    }

    // ─── Platform Fee ────────────────────────────────────────────────────────

    /// Set the platform fee in basis points (max 1 000 = 10 %).
    /// Admin only.
    pub fn set_platform_fee(env: Env, admin: Address, fee_bps: u32) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        admin.require_auth();
        platform_fee::set_platform_fee(&env, fee_bps)
    }

    /// Return the current platform fee in basis points.
    pub fn get_platform_fee(env: Env) -> u32 {
        platform_fee::get_platform_fee(&env)
    }

    // ─── Signer ──────────────────────────────────────────────────────────────

    /// Register or rotate the backend Ed25519 signer public key.
    pub fn set_signer(env: Env, admin: Address, pubkey: BytesN<32>) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        admin.require_auth();
        env.storage().instance().set(&DataKey::Signer, &pubkey);
        Ok(())
    }

    /// Return the currently registered signer, if any.
    pub fn get_signer(env: Env) -> Option<BytesN<32>> {
        env.storage().instance().get(&DataKey::Signer)
    }

    // ─── Pause ───────────────────────────────────────────────────────────────

    /// Pause the contract — blocks mint and transfer.
    pub fn pause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        admin.require_auth();
        env.storage().instance().set(&DataKey::Paused, &true);
        env.events().publish(("paused",), ());
        Ok(())
    }

    /// Unpause the contract.
    pub fn unpause(env: Env, admin: Address) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        admin.require_auth();
        env.storage().instance().set(&DataKey::Paused, &false);
        env.events().publish(("unpaused",), ());
        Ok(())
    }

    /// Returns `true` if the contract is paused.
    pub fn is_paused(env: Env) -> bool {
        env.storage().instance().get(&DataKey::Paused).unwrap_or(false)
    }

    // ─── Mint ────────────────────────────────────────────────────────────────

    /// Mint a new NFT for a video clip.
    pub fn mint(
        env: Env,
        admin: Address,
        to: Address,
        clip_id: u32,
        metadata_uri: String,
        royalty: Royalty,
        _signature: BytesN<64>,
    ) -> Result<TokenId, Error> {
        Self::require_admin(&env, &admin)?;
        admin.require_auth();
        Self::require_not_paused(&env)?;

        if env.storage().persistent().has(&DataKey::ClipIdMinted(clip_id)) {
            return Err(Error::ClipAlreadyMinted);
        }
        if royalty.basis_points > 10_000 {
            return Err(Error::InvalidBasisPoints);
        }

        let token_id: u32 = env
            .storage()
            .instance()
            .get(&DataKey::NextTokenId)
            .unwrap_or(0);

        env.storage()
            .persistent()
            .set(&DataKey::Token(token_id), &TokenData { owner: to.clone(), clip_id });
        env.storage()
            .persistent()
            .set(&DataKey::Metadata(token_id), &metadata_uri);
        env.storage()
            .persistent()
            .set(&DataKey::Royalty(token_id), &royalty);
        env.storage()
            .persistent()
            .set(&DataKey::ClipIdMinted(clip_id), &token_id);
        env.storage()
            .instance()
            .set(&DataKey::NextTokenId, &(token_id + 1));

        env.events().publish(
            ("mint",),
            MintEvent {
                to,
                clip_id,
                token_id,
                metadata_uri,
            },
        );
        Ok(token_id)
    }

    // ─── Transfer / Burn ─────────────────────────────────────────────────────

    /// Transfer NFT ownership.
    pub fn transfer(
        env: Env,
        from: Address,
        to: Address,
        token_id: TokenId,
    ) -> Result<(), Error> {
        from.require_auth();
        Self::require_not_paused(&env)?;
        let mut data = Self::get_token_data(&env, token_id)?;
        if data.owner != from {
            return Err(Error::Unauthorized);
        }
        data.owner = to;
        env.storage().persistent().set(&DataKey::Token(token_id), &data);
        Ok(())
    }

    /// Burn an NFT. Only the current owner may burn.
    pub fn burn(env: Env, owner: Address, token_id: TokenId) -> Result<(), Error> {
        owner.require_auth();
        let data = Self::get_token_data(&env, token_id)?;
        if data.owner != owner {
            return Err(Error::Unauthorized);
        }
        env.storage().persistent().remove(&DataKey::Token(token_id));
        env.storage().persistent().remove(&DataKey::Metadata(token_id));
        env.storage().persistent().remove(&DataKey::Royalty(token_id));
        Ok(())
    }

    // ─── Queries ─────────────────────────────────────────────────────────────

    /// Returns the owner of a token.
    pub fn owner_of(env: Env, token_id: TokenId) -> Result<Address, Error> {
        Ok(Self::get_token_data(&env, token_id)?.owner)
    }

    /// Returns the metadata URI of a token.
    pub fn token_uri(env: Env, token_id: TokenId) -> Result<String, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Metadata(token_id))
            .ok_or(Error::TokenNotFound)
    }

    /// Alias for `token_uri`.
    pub fn get_metadata(env: Env, token_id: TokenId) -> Result<String, Error> {
        Self::token_uri(env, token_id)
    }

    /// Look up token ID by clip ID.
    pub fn clip_token_id(env: Env, clip_id: u32) -> Result<TokenId, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::ClipIdMinted(clip_id))
            .ok_or(Error::TokenNotFound)
    }

    /// Returns the royalty struct for a token.
    pub fn get_royalty(env: Env, token_id: TokenId) -> Result<Royalty, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Royalty(token_id))
            .ok_or(Error::TokenNotFound)
    }

    /// Returns royalty receiver and computed amount for a given sale price.
    pub fn royalty_info(
        env: Env,
        token_id: TokenId,
        sale_price: i128,
    ) -> Result<RoyaltyInfo, Error> {
        let r = Self::get_royalty(env, token_id)?;
        let amount = sale_price * r.basis_points as i128 / 10_000;
        Ok(RoyaltyInfo {
            receiver: r.recipient,
            royalty_amount: amount,
            asset_address: r.asset_address,
        })
    }

    /// Update royalty config for a token. Admin only.
    pub fn set_royalty(
        env: Env,
        admin: Address,
        token_id: TokenId,
        new_royalty: Royalty,
    ) -> Result<(), Error> {
        Self::require_admin(&env, &admin)?;
        admin.require_auth();
        if new_royalty.basis_points > 10_000 {
            return Err(Error::InvalidBasisPoints);
        }
        if !env.storage().persistent().has(&DataKey::Token(token_id)) {
            return Err(Error::TokenNotFound);
        }
        env.storage()
            .persistent()
            .set(&DataKey::Royalty(token_id), &new_royalty);
        Ok(())
    }

    /// Returns total minted token count.
    pub fn total_supply(env: Env) -> u32 {
        env.storage().instance().get(&DataKey::NextTokenId).unwrap_or(0)
    }

    /// Returns true if the token exists.
    pub fn exists(env: Env, token_id: TokenId) -> bool {
        env.storage().persistent().has(&DataKey::Token(token_id))
    }

    // ─── Internal helpers ────────────────────────────────────────────────────

    fn require_admin(env: &Env, caller: &Address) -> Result<(), Error> {
        let admin: Address = env
            .storage()
            .instance()
            .get(&DataKey::Admin)
            .ok_or(Error::NotInitialized)?;
        if *caller != admin {
            return Err(Error::Unauthorized);
        }
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

    fn get_token_data(env: &Env, token_id: TokenId) -> Result<TokenData, Error> {
        env.storage()
            .persistent()
            .get(&DataKey::Token(token_id))
            .ok_or(Error::TokenNotFound)
    }
}
