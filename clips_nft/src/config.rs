//! Global contract configuration.
//!
//! [`Config`] consolidates all top-level settings into a single storable
//! struct so callers can read or update the contract state in one round-trip.

use soroban_sdk::{contracttype, Address, Env};

use crate::types::{DataKey, Error};

/// Contract version constant — bump on breaking interface changes.
pub const CONTRACT_VERSION: u32 = 1;

/// Reusable struct that holds every global contract setting.
///
/// Stored under [`DataKey::Config`] in instance storage.
#[contracttype]
#[derive(Clone)]
pub struct Config {
    /// Contract owner / administrator address.
    pub owner: Address,
    /// Semantic version number (monotonically increasing integer).
    pub version: u32,
    /// Platform fee in basis points (0–1 000, i.e. 0 %–10 %).
    pub platform_fee_bps: u32,
    /// Default royalty in basis points applied to newly minted NFTs (0–10 000).
    pub default_royalty_bps: u32,
    /// When `true`, mint and transfer operations are blocked.
    pub paused: bool,
}

/// Persist a [`Config`] snapshot to instance storage.
///
/// # Errors
/// Returns [`Error::InvalidBasisPoints`] when fee or royalty limits are exceeded.
pub fn set_config(env: &Env, config: Config) -> Result<(), Error> {
    if config.platform_fee_bps > crate::platform_fee::MAX_PLATFORM_FEE_BPS {
        return Err(Error::InvalidBasisPoints);
    }
    if config.default_royalty_bps > crate::default_royalty::MAX_ROYALTY_BPS {
        return Err(Error::InvalidBasisPoints);
    }
    env.storage().instance().set(&DataKey::Config, &config);
    Ok(())
}

/// Return the stored [`Config`], or `None` if the contract is not yet initialized.
pub fn get_config(env: &Env) -> Option<Config> {
    env.storage().instance().get(&DataKey::Config)
}
