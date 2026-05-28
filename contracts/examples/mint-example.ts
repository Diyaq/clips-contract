/**
 * Mint Example — ClipCash NFT (Soroban)
 *
 * Demonstrates how to mint a single clip NFT using the generated
 * `@clips-contract/clips_nft` TypeScript client.
 *
 * Prerequisites:
 *   - A funded Stellar wallet (Freighter or keypair)
 *   - A backend-signed mint payload (clip_id + metadata_uri + owner + signature)
 *   - The contract deployed and a signer public key registered via `set_signer`
 */

import { Keypair, Networks, rpc } from "@stellar/stellar-sdk";
import { Client, Royalty, RoyaltyRecipient } from "../clips_nft/src/index";

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const CONTRACT_ID = "CABC...YOUR_CONTRACT_ID";       // deployed contract address
const NETWORK_PASSPHRASE = Networks.TESTNET;
const RPC_URL = "https://soroban-testnet.stellar.org";

// Wallet that will own the minted NFT (must sign the transaction)
const ownerSecret = "S...YOUR_SECRET_KEY";
const ownerKeypair = Keypair.fromSecret(ownerSecret);
const ownerAddress = ownerKeypair.publicKey();

// ---------------------------------------------------------------------------
// Mint payload — produced by the ClipCash backend
// ---------------------------------------------------------------------------

const clipId = 42;
const metadataUri = "ipfs://QmExampleMetadataCID";

// 64-byte Ed25519 signature from the backend signer over:
//   SHA-256( clip_id_le4 || SHA-256(XDR(owner)) || SHA-256(UTF-8(metadataUri)) )
const backendSignature = Buffer.from("00".repeat(64), "hex"); // replace with real sig

// ---------------------------------------------------------------------------
// Royalty configuration (5% to creator, 1% platform auto-added by contract)
// ---------------------------------------------------------------------------

const royalty: Royalty = {
  recipients: [
    {
      recipient: ownerAddress,
      basis_points: 500, // 5%
    } satisfies RoyaltyRecipient,
  ],
  asset_address: null, // null = royalties paid in XLM
};

// ---------------------------------------------------------------------------
// Mint
// ---------------------------------------------------------------------------

async function mintClip(): Promise<void> {
  const server = new rpc.Server(RPC_URL);

  const client = new Client({
    contractId: CONTRACT_ID,
    networkPassphrase: NETWORK_PASSPHRASE,
    rpcUrl: RPC_URL,
    publicKey: ownerAddress,
    signTransaction: async (tx) => {
      const transaction = rpc.assembleTransaction(tx).build();
      transaction.sign(ownerKeypair);
      return transaction.toXDR();
    },
  });

  console.log(`Minting clip ${clipId} for owner ${ownerAddress}…`);

  const tx = await client.mint({
    to: ownerAddress,
    clip_id: clipId,
    metadata_uri: metadataUri,
    royalty,
    is_soulbound: false,
    signature: backendSignature,
  });

  const result = await tx.signAndSend();
  const tokenId = result.result;

  console.log(`Minted successfully! Token ID: ${tokenId}`);
}

mintClip().catch(console.error);
