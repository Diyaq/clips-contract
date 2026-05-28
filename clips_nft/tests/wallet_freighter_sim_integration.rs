#![cfg(test)]

mod test_helpers;

use clips_nft::{ClipsNftContract, ClipsNftContractClient, Royalty, RoyaltyRecipient};
use soroban_sdk::{
    testutils::{Address as _, Ledger as _},
    token, xdr::ToXdr, Address, Bytes, BytesN, Env, String, Vec,
};

use test_helpers::{sign_mint, TestContext};


/// Minimal Freighter-like simulation wrapper.
///
/// Notes:
/// - This is an integration test shim meant to model the *shape* of frontend wallet
///   interactions (connect -> get address -> sign & send).
/// - Soroban tests execute inside the contract test environment, so the actual auth
///   verification is performed via `env.mock_all_auths()`.
struct FreighterMock<'a> {
    env: &'a Env,
    addr: Address,
}

impl<'a> FreighterMock<'a> {
    fn connect(env: &'a Env, wallet_addr: Address) -> Self {
        // In real Freighter, this is where the wallet would prompt the user.
        // In Soroban tests, auth is simulated via `env.mock_all_auths()`.
        Self { env, addr: wallet_addr }
    }

    fn get_address(&self) -> Address {
        self.addr.clone()
    }

    fn sign_and_send_mint(
        &self,
        client: &ClipsNftContractClient<'a>,
        backend_signature: &BytesN<64>,
        clip_id: u32,
        metadata_uri: &String,
        royalty: &Royalty,
    ) -> u32 {
        // This call models the frontend submitting a transaction.
        // Authorization is covered by env.mock_all_auths().
        client.mint(
            &self.addr,
            &clip_id,
            metadata_uri,
            &None,
            &None,
            royalty,
            &false,
            backend_signature,
        )
    }

    fn sign_and_send_pay_royalty(
        &self,
        client: &ClipsNftContractClient<'a>,
        token_id: &u32,
        sale_price: &i128,
    ) {
        client.pay_royalty(&self.addr, token_id, sale_price);
    }

    fn sign_and_send_transfer(
        &self,
        client: &ClipsNftContractClient<'a>,
        to: &Address,
        token_id: &u32,
    ) {
        client.transfer(&self.addr, to, token_id, &0, &None);
    }
}

fn setup_with_backend() -> (Env, TestContext, Address) {
    let ctx = test_helpers::setup();
    let env = *ctx.env;
    let admin = ctx.admin.clone();

    // Return env by value-like reference for convenience.
    (env, ctx, admin)
}

#[test]
fn test_freighter_wallet_simulation_full_mint_and_royalty_flow() {
    // ----------- test context (contract + backend signer) -----------
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register(ClipsNftContract, ());
    let client = ClipsNftContractClient::new(&env, &contract_id);
    client.init(&admin);

    let backend = test_helpers::setup();
    // Reuse backend signer by re-registering signer on our freshly registered contract.
    // (Avoids changing existing helpers.)
    let pubkey = BytesN::from_array(&env, &backend.keypair.verifying_key().to_bytes());
    client.set_signer(&admin, &pubkey);

    let creator_wallet_addr = Address::generate(&env);
    let buyer_wallet_addr = Address::generate(&env);
    let creator = FreighterMock::connect(&env, creator_wallet_addr);
    let buyer = FreighterMock::connect(&env, buyer_wallet_addr);

    // ----------- mint (backend signs; wallet submits) -----------
    let clip_id = 777u32;
    let metadata_uri = String::from_str(&env, "ipfs://QmFreighterClip777");

    // Configure royalty: 5% creator + default platform 1%.
    let platform_bps = 100u32;
    let creator_bps = 500u32;

    let mut recipients = Vec::new(&env);
    recipients.push_back(RoyaltyRecipient {
        recipient: creator.get_address(),
        basis_points: creator_bps,
    });

    let royalty = Royalty {
        recipients,
        asset_address: None, // XLM
    };

    let backend_signature = sign_mint(
        &env,
        &backend.keypair,
        &creator.get_address(),
        clip_id,
        &metadata_uri,
    );

    let token_id = creator.sign_and_send_mint(&client, &backend_signature, clip_id, &metadata_uri, &royalty);
    assert_eq!(token_id, 1);
    assert_eq!(client.owner_of(&token_id), creator.get_address());
    assert_eq!(client.token_uri(&token_id), metadata_uri);

    // ----------- royalty flow (buyer pays royalty via wallet) -----------
    let sale_price = 1_000_i128;
    let royalty_info = client.royalty_info(&token_id, &sale_price);

    // creator should be one of the receivers; platform distribution is contract-internal.
    assert_eq!(royalty_info.asset_address, None);
    assert!(royalty_info.royalty_amount > 0);

    buyer.sign_and_send_pay_royalty(&client, &token_id, &sale_price);

    // ----------- verify stored royalty config splits correctly -----------
    // The contract stores: creator recipients + platform split.
    let stored = client.get_royalty(&token_id);
    assert_eq!(stored.recipients.len(), 2);

    let first = stored.recipients.get(0).unwrap();
    assert_eq!(first.recipient, creator.get_address());
    assert_eq!(first.basis_points, creator_bps);

    let second = stored.recipients.get(1).unwrap();
    assert_eq!(second.basis_points, platform_bps);

    // ----------- optional secondary transfer simulation (still wallet-submit shape) -----------
    let new_owner = Address::generate(&env);
    buyer.sign_and_send_transfer(&client, &new_owner, &token_id);
    assert_eq!(client.owner_of(&token_id), new_owner);
}

#[test]
fn test_freighter_wallet_simulation_custom_asset_mint_and_pay_royalty() {
    let env = Env::default();
    env.mock_all_auths();

    let admin = Address::generate(&env);
    let contract_id = env.register(ClipsNftContract, ());
    let client = ClipsNftContractClient::new(&env, &contract_id);
    client.init(&admin);

    let backend = test_helpers::setup();
    let pubkey = BytesN::from_array(&env, &backend.keypair.verifying_key().to_bytes());
    client.set_signer(&admin, &pubkey);

    let creator_wallet_addr = Address::generate(&env);
    let buyer_wallet_addr = Address::generate(&env);
    let creator = FreighterMock::connect(&env, creator_wallet_addr);
    let buyer = FreighterMock::connect(&env, buyer_wallet_addr);

    // Deploy and mint a custom SEP-0041 token to buyer.
    let token_admin = Address::generate(&env);
    let asset = env
        .register_stellar_asset_contract_v2(token_admin.clone())
        .address();
    let token_client = token::StellarAssetClient::new(&env, &asset);
    token_client.mint(&buyer_wallet_addr, &5_000_000_i128);

    // Mint with custom-asset royalty config.
    let clip_id = 778u32;
    let metadata_uri = String::from_str(&env, "ipfs://QmFreighterClip778");

    let mut recipients = Vec::new(&env);
    recipients.push_back(RoyaltyRecipient {
        recipient: creator.get_address(),
        basis_points: 500,
    });

    let royalty = Royalty {
        recipients,
        asset_address: Some(asset.clone()),
    };

    let backend_signature = sign_mint(
        &env,
        &backend.keypair,
        &creator.get_address(),
        clip_id,
        &metadata_uri,
    );

    let token_id = creator.sign_and_send_mint(&client, &backend_signature, clip_id, &metadata_uri, &royalty);
    assert_eq!(token_id, 1);

    // Pay royalty from buyer.
    let sale_price = 1_000_000_i128;
    let pre_creator_balance = token::TokenClient::new(&env, &asset).balance(&creator.get_address());

    buyer.sign_and_send_pay_royalty(&client, &token_id, &sale_price);

    let post_creator_balance = token::TokenClient::new(&env, &asset).balance(&creator.get_address());
    assert!(post_creator_balance > pre_creator_balance);

    // Ensure royalty receiver config was stored as expected.
    let stored = client.get_royalty(&token_id);
    assert_eq!(stored.recipients.len(), 2);
    assert_eq!(stored.asset_address, Some(asset));
}

