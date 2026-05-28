/**
 * Royalty Query Example — ClipCash NFT (Soroban)
 *
 * Read-only snippets that demonstrate how to query royalty information
 * for a token before executing a sale.
 *
 * These calls are simulated (no signing required) and are safe to run
 * from any frontend without a connected wallet.
 */

import { Networks, rpc } from "@stellar/stellar-sdk";
import { Client, Royalty, RoyaltyInfo } from "../clips_nft/src/index";

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const CONTRACT_ID = "CABC...YOUR_CONTRACT_ID";
const NETWORK_PASSPHRASE = Networks.TESTNET;
const RPC_URL = "https://soroban-testnet.stellar.org";

// Read-only client — no wallet needed for simulations
const client = new Client({
  contractId: CONTRACT_ID,
  networkPassphrase: NETWORK_PASSPHRASE,
  rpcUrl: RPC_URL,
});

// ---------------------------------------------------------------------------
// 1. Query royalty_info — EIP-2981-style: receiver + amount for a given price
// ---------------------------------------------------------------------------

async function queryRoyaltyInfo(tokenId: number, salePrice: bigint): Promise<void> {
  const tx = await client.royalty_info({ token_id: tokenId, sale_price: salePrice });
  const info: RoyaltyInfo = tx.result;

  console.log(`=== royalty_info(token=${tokenId}, price=${salePrice}) ===`);
  console.log(`  Primary receiver : ${info.receiver}`);
  console.log(`  Royalty amount   : ${info.royalty_amount}`);
  console.log(`  Payment asset    : ${info.asset_address ?? "XLM (native)"}`);
}

// ---------------------------------------------------------------------------
// 2. Query get_royalty — full split configuration for a token
// ---------------------------------------------------------------------------

async function queryFullRoyalty(tokenId: number): Promise<void> {
  const tx = await client.get_royalty({ token_id: tokenId });
  const royalty: Royalty = tx.result;

  console.log(`\n=== get_royalty(token=${tokenId}) ===`);
  console.log(`  Asset  : ${royalty.asset_address ?? "XLM (native)"}`);
  royalty.recipients.forEach((r, i) => {
    const pct = (r.basis_points / 100).toFixed(2);
    console.log(`  Recipient[${i}]: ${r.recipient}  (${pct}%)`);
  });
}

// ---------------------------------------------------------------------------
// 3. Calculate exact royalty amount using on-chain safe math
// ---------------------------------------------------------------------------

async function calculateRoyalty(tokenId: number, salePrice: bigint): Promise<bigint> {
  const tx = await client.calculate_royalty_amount({ token_id: tokenId, sale_price: salePrice });
  const amount = tx.result;
  console.log(`\n=== calculate_royalty_amount(token=${tokenId}, price=${salePrice}) ===`);
  console.log(`  Total royalty: ${amount}`);
  return amount;
}

// ---------------------------------------------------------------------------
// 4. Check token ownership and soulbound status before attempting a transfer
// ---------------------------------------------------------------------------

async function preTransferCheck(tokenId: number): Promise<void> {
  const [ownerTx, isSoulboundTx] = await Promise.all([
    client.owner_of({ token_id: tokenId }),
    client.is_soulbound({ token_id: tokenId }),
  ]);

  const owner = ownerTx.result;
  const soulbound = isSoulboundTx.result;

  console.log(`\n=== pre-transfer check for token ${tokenId} ===`);
  console.log(`  Owner     : ${owner}`);
  console.log(`  Soulbound : ${soulbound}`);
  if (soulbound) {
    console.warn("  ⚠ Token is soulbound — transfers are blocked.");
  }
}

// ---------------------------------------------------------------------------
// Run all examples
// ---------------------------------------------------------------------------

async function main(): Promise<void> {
  const TOKEN_ID = 1;
  const SALE_PRICE = BigInt(10_000_000); // 10 XLM in stroops

  await queryRoyaltyInfo(TOKEN_ID, SALE_PRICE);
  await queryFullRoyalty(TOKEN_ID);
  await calculateRoyalty(TOKEN_ID, SALE_PRICE);
  await preTransferCheck(TOKEN_ID);
}

main().catch(console.error);
