import { Buffer } from "buffer";
import { AssembledTransaction, Client as ContractClient, ClientOptions as ContractClientOptions, MethodOptions, Result } from "@stellar/stellar-sdk/contract";
import type { u32, u64, i128, Option } from "@stellar/stellar-sdk/contract";
export * from "@stellar/stellar-sdk";
export * as contract from "@stellar/stellar-sdk/contract";
export * as rpc from "@stellar/stellar-sdk/rpc";
/** On-chain token identifier — alias for u32. */
export type TokenId = u32;
/** Network configurations keyed by network name. */
export declare const networks: {
    readonly testnet: {
        readonly networkPassphrase: "Test SDF Network ; September 2015";
        readonly contractId: "";
        readonly rpcUrl: "https://soroban-testnet.stellar.org";
    };
    readonly mainnet: {
        readonly networkPassphrase: "Public Global Stellar Network ; September 2015";
        readonly contractId: "";
        readonly rpcUrl: "https://soroban-mainnet.stellar.org";
    };
};
export type NetworkName = keyof typeof networks;
/**
 * Create a ready-to-use contract client for the given network.
 *
 * @example
 * ```ts
 * const client = createClient("testnet", { publicKey: walletAddress });
 * const supply = await (await client.total_supply()).result;
 * ```
 */
export declare function createClient(network: NetworkName, options?: Partial<ContractClientOptions>): Client;
/**
 * Custom errors for the NFT contract
 */
export declare const Errors: {
    /**
     * Operation not authorized
     */
    1: {
        message: string;
    };
    /**
     * Invalid token ID
     */
    2: {
        message: string;
    };
    /**
     * Token already minted
     */
    3: {
        message: string;
    };
    /**
     * Royalty too high (max 10000 basis points = 100%)
     */
    4: {
        message: string;
    };
    /**
     * Invalid recipient
     */
    5: {
        message: string;
    };
    /**
     * Sale price must be greater than zero
     */
    6: {
        message: string;
    };
    /**
     * Contract is paused — minting and transfers are blocked
     */
    7: {
        message: string;
    };
    /**
     * Backend signature over the mint payload is invalid
     */
    8: {
        message: string;
    };
    /**
     * No backend signer public key has been registered yet
     */
    9: {
        message: string;
    };
    /**
     * Royalty split is invalid
     */
    10: {
        message: string;
    };
    /**
     * Token is soulbound (non-transferable)
     */
    11: {
        message: string;
    };
    /**
     * Royalty calculation would overflow
     */
    12: {
        message: string;
    };
    /**
     * Clip is blacklisted
     */
    13: {
        message: string;
    };
    /**
     * Caller is not authorized to approve
     */
    14: {
        message: string;
    };
    /**
     * Withdrawal is still locked (24h safety delay)
     */
    15: {
        message: string;
    };
    /**
     * No active withdrawal request found
     */
    16: {
        message: string;
    };
};
/**
 * Storage keys
 *
 * Key sizing notes:
 * - Enum variants with no payload (Admin, NextTokenId, Paused) are 1-word keys.
 * - Variants with a u32 payload (Token, ClipIdMinted) are
 * 2-word keys — the smallest possible for per-token entries.
 */
export type DataKey = {
    tag: "Admin";
    values: void;
} | {
    tag: "NextTokenId";
    values: void;
} | {
    tag: "Paused";
    values: void;
} | {
    tag: "Token";
    values: readonly [TokenId];
} | {
    tag: "ClipIdMinted";
    values: readonly [u32];
} | {
    tag: "Signer";
    values: void;
} | {
    tag: "PlatformRecipient";
    values: void;
} | {
    tag: "TotalGasMint";
    values: void;
} | {
    tag: "CountMint";
    values: void;
} | {
    tag: "TotalGasTransfer";
    values: void;
} | {
    tag: "CountTransfer";
    values: void;
};
export interface Royalty {
    /**
   * Optional SEP-0041 asset contract address.
   * `None` → royalties expected in XLM (native).
   */
    asset_address: Option<string>;
    /**
   * Multi-recipient split. Platform recipient is automatically added with 1%
   * if not present.
   */
    recipients: Array<RoyaltyRecipient>;
}
/**
 * Event emitted when an NFT is burned.
 */
export interface BurnEvent {
    clip_id: u32;
    owner: string;
    token_id: TokenId;
}
/**
 * Event emitted when a new NFT is minted
 */
export interface MintEvent {
    clip_id: u32;
    gas_used: u64;
    metadata_uri: string;
    to: string;
    token_id: TokenId;
}
/**
 * Packs owner address, originating clip_id, metadata, and royalty into a single persistent entry.
 *
 * Combining these fields eliminates the separate `Metadata` and `Royalty`
 * entries that were previously written on every mint.
 */
export interface TokenData {
    /**
   * The off-chain clip identifier this token was minted for.
   */
    clip_id: u32;
    /**
   * Whether this token is soulbound (non-transferable)
   */
    is_soulbound: boolean;
    /**
   * Metadata URI for the token
   */
    metadata_uri: string;
    owner: string;
    /**
   * Royalty configuration
   */
    royalty: Royalty;
}
/**
 * Royalty payment info returned by `royalty_info()`.
 */
export interface RoyaltyInfo {
    /**
   * `None` → pay in XLM; `Some(addr)` → pay in that SEP-0041 token.
   */
    asset_address: Option<string>;
    receiver: string;
    /**
   * Royalty amount in the same denomination as `sale_price`
   */
    royalty_amount: i128;
}
/**
 * Event emitted when NFT ownership changes.
 */
export interface TransferEvent {
    from: string;
    gas_used: u64;
    to: string;
    token_id: TokenId;
}
/**
 * Event emitted when royalty is paid.
 */
export interface RoyaltyPaidEvent {
    amount: i128;
    from: string;
    to: string;
    token_id: TokenId;
}
/**
 * Royalty information stored per token.
 * `asset_address` is `None` for native XLM, or `Some(contract_address)`
 * for any SEP-0041 custom Stellar asset.
 */
export interface RoyaltyRecipient {
    basis_points: u32;
    recipient: string;
}
/**
 * Event emitted when royalty recipient is updated.
 */
export interface RoyaltyRecipientUpdatedEvent {
    new_recipient: string;
    old_recipient: string;
    token_id: TokenId;
}
/**
 * Event emitted when a clip ID is blacklisted by admin.
 */
export interface BlacklistEvent {
    clip_id: u32;
}
export interface Client {
    /**
     * Construct and simulate a burn transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Burn (destroy) an NFT. Only the current owner may burn.
     *
     * Storage removes (persistent): TokenData, ClipIdMinted = **2** (Optimized from 4)
     */
    burn: ({ owner, token_id }: {
        owner: string;
        token_id: TokenId;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate a init transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Initialize the contract with an admin address.
     */
    init: ({ admin }: {
        admin: string;
    }, options?: MethodOptions) => Promise<AssembledTransaction<null>>;
    /**
     * Construct and simulate a mint transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Mint a new NFT for a video clip.
     *
     * Requires a valid Ed25519 `signature` from the registered backend signer
     * over the canonical mint payload, proving the clip exists and belongs to
     * `to`. The payload is:
     *
     * ```text
     * payload = SHA-256(
     * clip_id_le_4_bytes
     * || SHA-256(owner_address_xdr)   // 32 bytes
     * || SHA-256(metadata_uri_bytes)  // 32 bytes
     * )
     * ```
     *
     * Storage writes (persistent): TokenData, Metadata, Royalty, ClipIdMinted = **4**
     * Instance writes: NextTokenId = **1**
     *
     * # Arguments
     * * `to`           - Address that will own the NFT (must match the signed payload)
     * * `clip_id`      - Unique off-chain clip identifier (must match the signed payload)
     * * `metadata_uri` - IPFS or Arweave URI (must match the signed payload)
     * * `royalty`      - Royalty configuration
     * * `is_soulbound` - Whether the token is soulbound (non-transferable)
     * * `signature`    - 64-byte Ed25519 signature from the backend signer
     */
    mint: ({ to, clip_id, metadata_uri, royalty, is_soulbound, signature }: {
        to: string;
        clip_id: u32;
        metadata_uri: string;
        royalty: Royalty;
        is_soulbound: boolean;
        signature: Buffer;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<TokenId>>>;
    /**
     * Construct and simulate a pause transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Pause the contract. Blocks `mint` and `transfer` until unpaused.
     * Only callable by the admin.
     */
    pause: ({ admin }: {
        admin: string;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate a exists transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Returns true if the token exists.
     */
    exists: ({ token_id }: {
        token_id: TokenId;
    }, options?: MethodOptions) => Promise<AssembledTransaction<boolean>>;
    /**
     * Construct and simulate a unpause transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Unpause the contract, re-enabling `mint` and `transfer`.
     * Only callable by the admin.
     */
    unpause: ({ admin }: {
        admin: string;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate a version transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Returns the contract version.
     */
    version: (options?: MethodOptions) => Promise<AssembledTransaction<u32>>;
    /**
     * Construct and simulate a owner_of transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Returns the owner of a given token ID.
     */
    owner_of: ({ token_id }: {
        token_id: TokenId;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<string>>>;
    /**
     * Construct and simulate a transfer transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Transfer NFT ownership from `from` to `to`.
     *
     * Blocked if the token is soulbound (non-transferable).
     * Storage writes (persistent): TokenData = **1**
     *
     * # Arguments
     * * `from`     - Current owner (must authorize)
     * * `to`       - New owner
     * * `token_id` - Token to transfer
     */
    transfer: ({ from, to, token_id }: {
        from: string;
        to: string;
        token_id: TokenId;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate a is_paused transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Returns `true` if the contract is currently paused.
     */
    is_paused: (options?: MethodOptions) => Promise<AssembledTransaction<boolean>>;
    /**
     * Construct and simulate a token_uri transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Returns the metadata URI for a given token ID.
     */
    token_uri: ({ token_id }: {
        token_id: TokenId;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<string>>>;
    /**
     * Construct and simulate a get_signer transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Return the currently registered backend signer public key, if any.
     */
    get_signer: (options?: MethodOptions) => Promise<AssembledTransaction<Option<Buffer>>>;
    /**
     * Construct and simulate a set_signer transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Register (or rotate) the backend Ed25519 public key used to verify
     * clip ownership before minting. Only callable by the admin.
     *
     * # Arguments
     * * `admin`  - Must be the contract admin
     * * `pubkey` - 32-byte Ed25519 public key of the trusted backend signer
     */
    set_signer: ({ admin, pubkey }: {
        admin: string;
        pubkey: Buffer;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate a get_royalty transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Returns the stored `Royalty` struct for a token.
     */
    get_royalty: ({ token_id }: {
        token_id: TokenId;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<Royalty>>>;
    /**
     * Construct and simulate a pay_royalty transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Pay royalties for a token sale using the asset configured in the royalty.
     *
     * Only handles SEP-0041 custom assets. For XLM (`asset_address` is `None`)
     * the marketplace must handle the transfer directly.
     */
    pay_royalty: ({ payer, token_id, sale_price }: {
        payer: string;
        token_id: TokenId;
        sale_price: i128;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate a set_royalty transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Update the royalty configuration for a token. Admin only.
     * Emits RoyaltyRecipientUpdated event when the primary recipient changes.
     */
    set_royalty: ({ admin, token_id, new_royalty }: {
        admin: string;
        token_id: TokenId;
        new_royalty: Royalty;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate a get_metadata transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Alias for `token_uri`, kept for compatibility.
     */
    get_metadata: ({ token_id }: {
        token_id: TokenId;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<string>>>;
    /**
     * Construct and simulate a is_soulbound transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Returns true if the token is soulbound (non-transferable).
     */
    is_soulbound: ({ token_id }: {
        token_id: TokenId;
    }, options?: MethodOptions) => Promise<AssembledTransaction<boolean>>;
    /**
     * Construct and simulate a royalty_info transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Returns the royalty receiver, amount, and payment asset for a given sale price.
     *
     * Uses safe math to prevent overflow. Royalty amount is calculated as:
     * `royalty_amount = sale_price * basis_points / 10000`
     *
     * Safe limits: sale_price should not exceed i128::MAX / 10000 to avoid overflow.
     */
    royalty_info: ({ token_id, sale_price }: {
        token_id: TokenId;
        sale_price: i128;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<RoyaltyInfo>>>;
    /**
     * Construct and simulate a total_supply transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Returns the total number of minted (and not yet burned) tokens.
     *
     * Derived from `NextTokenId - 1` — no separate counter needed.
     */
    total_supply: (options?: MethodOptions) => Promise<AssembledTransaction<u32>>;
    /**
     * Construct and simulate a clip_token_id transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Look up the on-chain token ID for a given clip_id.
     */
    clip_token_id: ({ clip_id }: {
        clip_id: u32;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<TokenId>>>;
    /**
     * Construct and simulate a get_avg_gas_cost transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Returns the average synthetic gas cost for a given operation type.
     * 0 = Mint, 1 = Transfer
     */
    get_avg_gas_cost: ({ op_type }: {
        op_type: u32;
    }, options?: MethodOptions) => Promise<AssembledTransaction<u64>>;
    /**
     * Construct and simulate a blacklist_clip transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Blacklist a clip ID, preventing it from being minted. Only callable by the admin.
     * Emits a Blacklist event.
     */
    blacklist_clip: ({ admin, clip_id }: {
        admin: string;
        clip_id: u32;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate an update_royalty_recipient transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Allow the current royalty recipient to update their address.
     * Only the current primary royalty recipient (index 0) may call this.
     * Emits RoyaltyRecipientUpdated event.
     */
    update_royalty_recipient: ({ caller, token_id, new_recipient }: {
        caller: string;
        token_id: TokenId;
        new_recipient: string;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<void>>>;
    /**
     * Construct and simulate a tokens_of_owner transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Return all token IDs owned by `owner`. Capped at 1000 results.
     */
    tokens_of_owner: ({ owner }: {
        owner: string;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Array<TokenId>>>;
    /**
     * Construct and simulate a calculate_royalty_amount transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Calculate the royalty amount for a given sale price using the token's stored royalty configuration.
     */
    calculate_royalty_amount: ({ token_id, sale_price }: {
        token_id: TokenId;
        sale_price: i128;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<i128>>>;
    /**
     * Construct and simulate a batch_mint transaction. Returns an `AssembledTransaction` object which will have a `result` field containing the result of the simulation. If this transaction changes contract state, you will need to call `signAndSend()` on the returned object.
     * Mint multiple clips in a single transaction.
     */
    batch_mint: ({ to, clip_ids, metadata_uris, royalty, is_soulbound, signatures }: {
        to: string;
        clip_ids: Array<u32>;
        metadata_uris: Array<string>;
        royalty: Royalty;
        is_soulbound: boolean;
        signatures: Array<Buffer>;
    }, options?: MethodOptions) => Promise<AssembledTransaction<Result<Array<TokenId>>>>;
}
export declare class Client extends ContractClient {
    readonly options: ContractClientOptions;
    static deploy<T = Client>(
    /** Options for initializing a Client as well as for calling a method, with extras specific to deploying. */
    options: MethodOptions & Omit<ContractClientOptions, "contractId"> & {
        /** The hash of the Wasm blob, which must already be installed on-chain. */
        wasmHash: Buffer | string;
        /** Salt used to generate the contract's ID. Passed through to {@link Operation.createCustomContract}. Default: random. */
        salt?: Buffer | Uint8Array;
        /** The format used to decode `wasmHash`, if it's provided as a string. */
        format?: "hex" | "base64";
    }): Promise<AssembledTransaction<T>>;
    constructor(options: ContractClientOptions);
    readonly fromJSON: {
        burn: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        init: (json: string) => AssembledTransaction<null>;
        mint: (json: string) => AssembledTransaction<Result<number, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        pause: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        exists: (json: string) => AssembledTransaction<boolean>;
        unpause: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        version: (json: string) => AssembledTransaction<number>;
        owner_of: (json: string) => AssembledTransaction<Result<string, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        transfer: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        is_paused: (json: string) => AssembledTransaction<boolean>;
        token_uri: (json: string) => AssembledTransaction<Result<string, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        get_signer: (json: string) => AssembledTransaction<Option<Buffer>>;
        set_signer: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        get_royalty: (json: string) => AssembledTransaction<Result<Royalty, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        pay_royalty: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        set_royalty: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        get_metadata: (json: string) => AssembledTransaction<Result<string, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        is_soulbound: (json: string) => AssembledTransaction<boolean>;
        royalty_info: (json: string) => AssembledTransaction<Result<RoyaltyInfo, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        total_supply: (json: string) => AssembledTransaction<number>;
        clip_token_id: (json: string) => AssembledTransaction<Result<number, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        get_avg_gas_cost: (json: string) => AssembledTransaction<bigint>;
        blacklist_clip: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        update_royalty_recipient: (json: string) => AssembledTransaction<Result<void, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        tokens_of_owner: (json: string) => AssembledTransaction<number[]>;
        calculate_royalty_amount: (json: string) => AssembledTransaction<Result<bigint, import("@stellar/stellar-sdk/contract").ErrorMessage>>;
        batch_mint: (json: string) => AssembledTransaction<Result<number[], import("@stellar/stellar-sdk/contract").ErrorMessage>>;
    };
}
