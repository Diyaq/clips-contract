/**
 * Transfer Example — ClipCash NFT (Soroban)
 *
 * Demonstrates how to transfer an NFT from one wallet to another,
 * with optional royalty payment on sale.
 *
 * Prerequisites:
 *   - Sender must own the token (is_soulbound must be false)
 *   - For paid transfers (sale_price > 0) the buyer must have sufficient
 *     balance in the royalty asset and also sign the transaction
 */

import { Keypair, Networks, rpc } from "@stellar/stellar-sdk";
import { Client } from "../clips_nft/src/index";

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const CONTRACT_ID = "CABC...YOUR_CONTRACT_ID";
const NETWORK_PASSPHRASE = Networks.TESTNET;
const RPC_URL = "https://soroban-testnet.stellar.org";

const senderSecret = "S...SENDER_SECRET_KEY";
const senderKeypair = Keypair.fromSecret(senderSecret);
const senderAddress = senderKeypair.publicKey();

const recipientAddress = "G...RECIPIENT_PUBLIC_KEY";

// Token to transfer (returned by mint())
const tokenId = 1;

// ---------------------------------------------------------------------------
// Free transfer (no royalty payment)
// ---------------------------------------------------------------------------

async function freeTransfer(): Promise<void> {
  const client = new Client({
    contractId: CONTRACT_ID,
    networkPassphrase: NETWORK_PASSPHRASE,
    rpcUrl: RPC_URL,
    publicKey: senderAddress,
    signTransaction: async (tx) => {
      const transaction = rpc.assembleTransaction(tx).build();
      transaction.sign(senderKeypair);
      return transaction.toXDR();
    },
  });

  console.log(`Transferring token ${tokenId} from ${senderAddress} to ${recipientAddress}…`);

  const tx = await client.transfer({
    from: senderAddress,
    to: recipientAddress,
    token_id: tokenId,
    sale_price: BigInt(0), // 0 = free transfer, no royalties collected
    payment_asset: null,
  });

  await tx.signAndSend();
  console.log("Transfer complete.");
}

// ---------------------------------------------------------------------------
// Paid transfer — royalties are deducted from buyer and sent to recipients
// ---------------------------------------------------------------------------

async function paidTransfer(): Promise<void> {
  // For paid transfers the buyer (recipient) must also sign.
  // Use a multi-sig or auth-all approach in Soroban.
  const buyerKeypair = Keypair.fromSecret("S...BUYER_SECRET");

  const client = new Client({
    contractId: CONTRACT_ID,
    networkPassphrase: NETWORK_PASSPHRASE,
    rpcUrl: RPC_URL,
    publicKey: senderAddress,
    signTransaction: async (tx) => {
      // Both sender and buyer must authorize
      const transaction = rpc.assembleTransaction(tx).build();
      transaction.sign(senderKeypair);
      transaction.sign(buyerKeypair);
      return transaction.toXDR();
    },
  });

  // First query royalty info so the buyer knows how much to approve
  const royaltyTx = await client.royalty_info({
    token_id: tokenId,
    sale_price: BigInt(1_000_000), // sale price in asset's smallest unit
  });
  const info = royaltyTx.result;
  console.log(
    `Royalty for this sale: ${info.royalty_amount} (asset: ${info.asset_address ?? "XLM"})`
  );

  const transferTx = await client.transfer({
    from: senderAddress,
    to: buyerKeypair.publicKey(),
    token_id: tokenId,
    sale_price: BigInt(1_000_000),
    payment_asset: info.asset_address ?? null,
  });

  await transferTx.signAndSend();
  console.log("Paid transfer complete, royalties distributed.");
}

freeTransfer().catch(console.error);
