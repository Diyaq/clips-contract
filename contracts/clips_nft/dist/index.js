import { Buffer } from "buffer";
import { Client as ContractClient, Spec as ContractSpec, } from "@stellar/stellar-sdk/contract";
export * from "@stellar/stellar-sdk";
export * as contract from "@stellar/stellar-sdk/contract";
export * as rpc from "@stellar/stellar-sdk/rpc";
if (typeof window !== "undefined") {
    //@ts-ignore Buffer exists
    window.Buffer = window.Buffer || Buffer;
}
/** Network configurations keyed by network name. */
export const networks = {
    testnet: {
        networkPassphrase: "Test SDF Network ; September 2015",
        contractId: "",
        rpcUrl: "https://soroban-testnet.stellar.org",
    },
    mainnet: {
        networkPassphrase: "Public Global Stellar Network ; September 2015",
        contractId: "",
        rpcUrl: "https://soroban-mainnet.stellar.org",
    },
};
/**
 * Create a ready-to-use contract client for the given network.
 *
 * @example
 * ```ts
 * const client = createClient("testnet", { publicKey: walletAddress });
 * const supply = await (await client.total_supply()).result;
 * ```
 */
export function createClient(network, options) {
    const { contractId, networkPassphrase, rpcUrl } = networks[network];
    return new Client({ contractId, networkPassphrase, rpcUrl, ...options });
}
/**
 * Custom errors for the NFT contract
 */
export const Errors = {
    /**
     * Operation not authorized
     */
    1: { message: "Unauthorized" },
    /**
     * Invalid token ID
     */
    2: { message: "InvalidTokenId" },
    /**
     * Token already minted
     */
    3: { message: "TokenAlreadyMinted" },
    /**
     * Royalty too high (max 10000 basis points = 100%)
     */
    4: { message: "RoyaltyTooHigh" },
    /**
     * Invalid recipient
     */
    5: { message: "InvalidRecipient" },
    /**
     * Sale price must be greater than zero
     */
    6: { message: "InvalidSalePrice" },
    /**
     * Contract is paused — minting and transfers are blocked
     */
    7: { message: "ContractPaused" },
    /**
     * Backend signature over the mint payload is invalid
     */
    8: { message: "InvalidSignature" },
    /**
     * No backend signer public key has been registered yet
     */
    9: { message: "SignerNotSet" },
    /**
     * Royalty split is invalid
     */
    10: { message: "InvalidRoyaltySplit" },
    /**
     * Token is soulbound (non-transferable)
     */
    11: { message: "SoulboundTransferBlocked" },
    /**
     * Royalty calculation would overflow
     */
    12: { message: "RoyaltyOverflow" },
    /**
     * Clip is blacklisted
     */
    13: { message: "ClipBlacklisted" },
    /**
     * Caller is not authorized to approve
     */
    14: { message: "NotAuthorizedToApprove" },
    /**
     * Withdrawal is still locked (24h safety delay)
     */
    15: { message: "WithdrawalStillLocked" },
    /**
     * No active withdrawal request found
     */
    16: { message: "NoWithdrawalRequest" }
};
export class Client extends ContractClient {
    options;
    static async deploy(
    /** Options for initializing a Client as well as for calling a method, with extras specific to deploying. */
    options) {
        return ContractClient.deploy(null, options);
    }
    constructor(options) {
        super(new ContractSpec(["AAAABAAAACJDdXN0b20gZXJyb3JzIGZvciB0aGUgTkZUIGNvbnRyYWN0AAAAAAAAAAAABUVycm9yAAAAAAAADAAAABhPcGVyYXRpb24gbm90IGF1dGhvcml6ZWQAAAAMVW5hdXRob3JpemVkAAAAAQAAABBJbnZhbGlkIHRva2VuIElEAAAADkludmFsaWRUb2tlbklkAAAAAAACAAAAFFRva2VuIGFscmVhZHkgbWludGVkAAAAElRva2VuQWxyZWFkeU1pbnRlZAAAAAAAAwAAADBSb3lhbHR5IHRvbyBoaWdoIChtYXggMTAwMDAgYmFzaXMgcG9pbnRzID0gMTAwJSkAAAAOUm95YWx0eVRvb0hpZ2gAAAAAAAQAAAARSW52YWxpZCByZWNpcGllbnQAAAAAAAAQSW52YWxpZFJlY2lwaWVudAAAAAUAAAAkU2FsZSBwcmljZSBtdXN0IGJlIGdyZWF0ZXIgdGhhbiB6ZXJvAAAAEEludmFsaWRTYWxlUHJpY2UAAAAGAAAAOENvbnRyYWN0IGlzIHBhdXNlZCDigJQgbWludGluZyBhbmQgdHJhbnNmZXJzIGFyZSBibG9ja2VkAAAADkNvbnRyYWN0UGF1c2VkAAAAAAAHAAAAMkJhY2tlbmQgc2lnbmF0dXJlIG92ZXIgdGhlIG1pbnQgcGF5bG9hZCBpcyBpbnZhbGlkAAAAAAAQSW52YWxpZFNpZ25hdHVyZQAAAAgAAAA0Tm8gYmFja2VuZCBzaWduZXIgcHVibGljIGtleSBoYXMgYmVlbiByZWdpc3RlcmVkIHlldAAAAAxTaWduZXJOb3RTZXQAAAAJAAAAGFJveWFsdHkgc3BsaXQgaXMgaW52YWxpZAAAABNJbnZhbGlkUm95YWx0eVNwbGl0AAAAAAoAAAAlVG9rZW4gaXMgc291bGJvdW5kIChub24tdHJhbnNmZXJhYmxlKQAAAAAAABhTb3VsYm91bmRUcmFuc2ZlckJsb2NrZWQAAAALAAAAIlJveWFsdHkgY2FsY3VsYXRpb24gd291bGQgb3ZlcmZsb3cAAAAAAA9Sb3lhbHR5T3ZlcmZsb3cAAAAADA==",
            "AAAAAgAAAOJTdG9yYWdlIGtleXMKCktleSBzaXppbmcgbm90ZXM6Ci0gRW51bSB2YXJpYW50cyB3aXRoIG5vIHBheWxvYWQgKEFkbWluLCBOZXh0VG9rZW5JZCwgUGF1c2VkKSBhcmUgMS13b3JkIGtleXMuCi0gVmFyaWFudHMgd2l0aCBhIHUzMiBwYXlsb2FkIChUb2tlbiwgQ2xpcElkTWludGVkKSBhcmUKMi13b3JkIGtleXMg4oCUIHRoZSBzbWFsbGVzdCBwb3NzaWJsZSBmb3IgcGVyLXRva2VuIGVudHJpZXMuAAAAAAAAAAAAB0RhdGFLZXkAAAAACwAAAAAAAAAxQ29udHJhY3QgYWRtaW5pc3RyYXRvciBhZGRyZXNzIChpbnN0YW5jZSBzdG9yYWdlKQAAAAAAAAVBZG1pbgAAAAAAAAAAAACBTW9ub3RvbmljYWxseSBpbmNyZWFzaW5nIHRva2VuIElEIGNvdW50ZXIgKGluc3RhbmNlIHN0b3JhZ2UpLgpgdG90YWxfc3VwcGx5ID0gTmV4dFRva2VuSWQgLSAxYCDigJQgbm8gc2VwYXJhdGUgVG9rZW5Db3VudCBuZWVkZWQuAAAAAAAAC05leHRUb2tlbklkAAAAAAAAAAAdUGF1c2UgZmxhZyAoaW5zdGFuY2Ugc3RvcmFnZSkAAAAAAAAGUGF1c2VkAAAAAAABAAAATFBhY2tlZCBvd25lciArIGNsaXBfaWQgKyBtZXRhZGF0YSArIHJveWFsdHkgZm9yIGEgdG9rZW4gKHBlcnNpc3RlbnQgc3RvcmFnZSkAAAAFVG9rZW4AAAAAAAABAAAH0AAAAAdUb2tlbklkAAAAAAEAAAA2RGVkdXAgZ3VhcmQ6IGNsaXBfaWQg4oaSIHRva2VuX2lkIChwZXJzaXN0ZW50IHN0b3JhZ2UpAAAAAAAMQ2xpcElkTWludGVkAAAAAQAAAAQAAAAAAAAAQ0VkMjU1MTkgcHVibGljIGtleSBvZiB0aGUgdHJ1c3RlZCBiYWNrZW5kIHNpZ25lciAoaW5zdGFuY2Ugc3RvcmFnZSkAAAAABlNpZ25lcgAAAAAAAAAAADJQbGF0Zm9ybSByZWNpcGllbnQgdXNlZCBmb3IgZGVmYXVsdCAxJSByb3lhbHR5IGN1dAAAAAAAEVBsYXRmb3JtUmVjaXBpZW50AAAAAAAAAAAAADZUb3RhbCBzeW50aGV0aWMgZ2FzIHVzZWQgaW4gbWludGluZyAoaW5zdGFuY2Ugc3RvcmFnZSkAAAAAAAxUb3RhbEdhc01pbnQAAAAAAAAAM1RvdGFsIG51bWJlciBvZiBzdWNjZXNzZnVsIG1pbnRzIChpbnN0YW5jZSBzdG9yYWdlKQAAAAAJQ291bnRNaW50AAAAAAAAAAAAADhUb3RhbCBzeW50aGV0aWMgZ2FzIHVzZWQgaW4gdHJhbnNmZXJzIChpbnN0YW5jZSBzdG9yYWdlKQAAABBUb3RhbEdhc1RyYW5zZmVyAAAAAAAAADdUb3RhbCBudW1iZXIgb2Ygc3VjY2Vzc2Z1bCB0cmFuc2ZlcnMgKGluc3RhbmNlIHN0b3JhZ2UpAAAAAA1Db3VudFRyYW5zZmVyAAAA",
            "AAAAAQAAAAAAAAAAAAAAB1JveWFsdHkAAAAAAgAAAFhPcHRpb25hbCBTRVAtMDA0MSBhc3NldCBjb250cmFjdCBhZGRyZXNzLgpgTm9uZWAg4oaSIHJveWFsdGllcyBleHBlY3RlZCBpbiBYTE0gKG5hdGl2ZSkuAAAADWFzc2V0X2FkZHJlc3MAAAAAAAPoAAAAEwAAAFhNdWx0aS1yZWNpcGllbnQgc3BsaXQuIFBsYXRmb3JtIHJlY2lwaWVudCBpcyBhdXRvbWF0aWNhbGx5IGFkZGVkIHdpdGggMSUKaWYgbm90IHByZXNlbnQuAAAACnJlY2lwaWVudHMAAAAAA+oAAAfQAAAAEFJveWFsdHlSZWNpcGllbnQ=",
            "AAAAAQAAACRFdmVudCBlbWl0dGVkIHdoZW4gYW4gTkZUIGlzIGJ1cm5lZC4AAAAAAAAACUJ1cm5FdmVudAAAAAAAAAMAAAAAAAAAB2NsaXBfaWQAAAAABAAAAAAAAAAFb3duZXIAAAAAAAATAAAAAAAAAAh0b2tlbl9pZAAAB9AAAAAHVG9rZW5JZAA=",
            "AAAAAQAAACZFdmVudCBlbWl0dGVkIHdoZW4gYSBuZXcgTkZUIGlzIG1pbnRlZAAAAAAAAAAAAAlNaW50RXZlbnQAAAAAAAAFAAAAAAAAAAdjbGlwX2lkAAAAAAQAAAAAAAAACGdhc191c2VkAAAABgAAAAAAAAAMbWV0YWRhdGFfdXJpAAAAEAAAAAAAAAACdG8AAAAAABMAAAAAAAAACHRva2VuX2lkAAAH0AAAAAdUb2tlbklkAA==",
            "AAAAAQAAANxQYWNrcyBvd25lciBhZGRyZXNzLCBvcmlnaW5hdGluZyBjbGlwX2lkLCBtZXRhZGF0YSwgYW5kIHJveWFsdHkgaW50byBhIHNpbmdsZSBwZXJzaXN0ZW50IGVudHJ5LgoKQ29tYmluaW5nIHRoZXNlIGZpZWxkcyBlbGltaW5hdGVzIHRoZSBzZXBhcmF0ZSBgTWV0YWRhdGFgIGFuZCBgUm95YWx0eWAKZW50cmllcyB0aGF0IHdlcmUgcHJldmlvdXNseSB3cml0dGVuIG9uIGV2ZXJ5IG1pbnQuAAAAAAAAAAlUb2tlbkRhdGEAAAAAAAAFAAAAOFRoZSBvZmYtY2hhaW4gY2xpcCBpZGVudGlmaWVyIHRoaXMgdG9rZW4gd2FzIG1pbnRlZCBmb3IuAAAAB2NsaXBfaWQAAAAABAAAADJXaGV0aGVyIHRoaXMgdG9rZW4gaXMgc291bGJvdW5kIChub24tdHJhbnNmZXJhYmxlKQAAAAAADGlzX3NvdWxib3VuZAAAAAEAAAAaTWV0YWRhdGEgVVJJIGZvciB0aGUgdG9rZW4AAAAAAAxtZXRhZGF0YV91cmkAAAAQAAAAAAAAAAVvd25lcgAAAAAAABMAAAAVUm95YWx0eSBjb25maWd1cmF0aW9uAAAAAAAAB3JveWFsdHkAAAAH0AAAAAdSb3lhbHR5AA==",
            "AAAAAQAAADJSb3lhbHR5IHBheW1lbnQgaW5mbyByZXR1cm5lZCBieSBgcm95YWx0eV9pbmZvKClgLgAAAAAAAAAAAAtSb3lhbHR5SW5mbwAAAAADAAAAQ2BOb25lYCDihpIgcGF5IGluIFhMTTsgYFNvbWUoYWRkcilgIOKGkiBwYXkgaW4gdGhhdCBTRVAtMDA0MSB0b2tlbi4AAAAADWFzc2V0X2FkZHJlc3MAAAAAAAPoAAAAEwAAAAAAAAAIcmVjZWl2ZXIAAAATAAAAN1JveWFsdHkgYW1vdW50IGluIHRoZSBzYW1lIGRlbm9taW5hdGlvbiBhcyBgc2FsZV9wcmljZWAAAAAADnJveWFsdHlfYW1vdW50AAAAAAAL",
            "AAAAAQAAAClFdmVudCBlbWl0dGVkIHdoZW4gTkZUIG93bmVyc2hpcCBjaGFuZ2VzLgAAAAAAAAAAAAANVHJhbnNmZXJFdmVudAAAAAAAAAQAAAAAAAAABGZyb20AAAATAAAAAAAAAAhnYXNfdXNlZAAAAAYAAAAAAAAAAnRvAAAAAAATAAAAAAAAAAh0b2tlbl9pZAAAB9AAAAAHVG9rZW5JZAA=",
            "AAAAAAAAAIlCdXJuIChkZXN0cm95KSBhbiBORlQuIE9ubHkgdGhlIGN1cnJlbnQgb3duZXIgbWF5IGJ1cm4uCgpTdG9yYWdlIHJlbW92ZXMgKHBlcnNpc3RlbnQpOiBUb2tlbkRhdGEsIENsaXBJZE1pbnRlZCA9ICoqMioqIChPcHRpbWl6ZWQgZnJvbSA0KQAAAAAAAARidXJuAAAAAgAAAAAAAAAFb3duZXIAAAAAAAATAAAAAAAAAAh0b2tlbl9pZAAAB9AAAAAHVG9rZW5JZAAAAAABAAAD6QAAAAIAAAAD",
            "AAAAAAAAAC5Jbml0aWFsaXplIHRoZSBjb250cmFjdCB3aXRoIGFuIGFkbWluIGFkZHJlc3MuAAAAAAAEaW5pdAAAAAEAAAAAAAAABWFkbWluAAAAAAAAEwAAAAA=",
            "AAAAAAAAA3ZNaW50IGEgbmV3IE5GVCBmb3IgYSB2aWRlbyBjbGlwLgoKUmVxdWlyZXMgYSB2YWxpZCBFZDI1NTE5IGBzaWduYXR1cmVgIGZyb20gdGhlIHJlZ2lzdGVyZWQgYmFja2VuZCBzaWduZXIKb3ZlciB0aGUgY2Fub25pY2FsIG1pbnQgcGF5bG9hZCwgcHJvdmluZyB0aGUgY2xpcCBleGlzdHMgYW5kIGJlbG9uZ3MgdG8KYHRvYC4gVGhlIHBheWxvYWQgaXM6CgpgYGB0ZXh0CnBheWxvYWQgPSBTSEEtMjU2KApjbGlwX2lkX2xlXzRfYnl0ZXMKfHwgU0hBLTI1Nihvd25lcl9hZGRyZXNzX3hkcikgICAvLyAzMiBieXRlcwp8fCBTSEEtMjU2KG1ldGFkYXRhX3VyaV9ieXRlcykgIC8vIDMyIGJ5dGVzCikKYGBgCgpTdG9yYWdlIHdyaXRlcyAocGVyc2lzdGVudCk6IFRva2VuRGF0YSwgTWV0YWRhdGEsIFJveWFsdHksIENsaXBJZE1pbnRlZCA9ICoqNCoqCkluc3RhbmNlIHdyaXRlczogTmV4dFRva2VuSWQgPSAqKjEqKgoKIyBBcmd1bWVudHMKKiBgdG9gICAgICAgICAgICAtIEFkZHJlc3MgdGhhdCB3aWxsIG93biB0aGUgTkZUIChtdXN0IG1hdGNoIHRoZSBzaWduZWQgcGF5bG9hZCkKKiBgY2xpcF9pZGAgICAgICAtIFVuaXF1ZSBvZmYtY2hhaW4gY2xpcCBpZGVudGlmaWVyIChtdXN0IG1hdGNoIHRoZSBzaWduZWQgcGF5bG9hZCkKKiBgbWV0YWRhdGFfdXJpYCAtIElQRlMgb3IgQXJ3ZWF2ZSBVUkkgKG11c3QgbWF0Y2ggdGhlIHNpZ25lZCBwYXlsb2FkKQoqIGByb3lhbHR5YCAgICAgIC0gUm95YWx0eSBjb25maWd1cmF0aW9uCiogYGlzX3NvdWxib3VuZGAgLSBXaGV0aGVyIHRoZSB0b2tlbiBpcyBzb3VsYm91bmQgKG5vbi10cmFuc2ZlcmFibGUpCiogYHNpZ25hdHVyZWAgICAgLSA2NC1ieXRlIEVkMjU1MTkgc2lnbmF0dXJlIGZyb20gdGhlIGJhY2tlbmQgc2lnbmVyAAAAAAAEbWludAAAAAYAAAAAAAAAAnRvAAAAAAATAAAAAAAAAAdjbGlwX2lkAAAAAAQAAAAAAAAADG1ldGFkYXRhX3VyaQAAABAAAAAAAAAAB3JveWFsdHkAAAAH0AAAAAdSb3lhbHR5AAAAAAAAAAAMaXNfc291bGJvdW5kAAAAAQAAAAAAAAAJc2lnbmF0dXJlAAAAAAAD7gAAAEAAAAABAAAD6QAAB9AAAAAHVG9rZW5JZAAAAAAD",
            "AAAAAAAAAFxQYXVzZSB0aGUgY29udHJhY3QuIEJsb2NrcyBgbWludGAgYW5kIGB0cmFuc2ZlcmAgdW50aWwgdW5wYXVzZWQuCk9ubHkgY2FsbGFibGUgYnkgdGhlIGFkbWluLgAAAAVwYXVzZQAAAAAAAAEAAAAAAAAABWFkbWluAAAAAAAAEwAAAAEAAAPpAAAAAgAAAAM=",
            "AAAAAAAAACFSZXR1cm5zIHRydWUgaWYgdGhlIHRva2VuIGV4aXN0cy4AAAAAAAAGZXhpc3RzAAAAAAABAAAAAAAAAAh0b2tlbl9pZAAAB9AAAAAHVG9rZW5JZAAAAAABAAAAAQ==",
            "AAAAAQAAACNFdmVudCBlbWl0dGVkIHdoZW4gcm95YWx0eSBpcyBwYWlkLgAAAAAAAAAAEFJveWFsdHlQYWlkRXZlbnQAAAAEAAAAAAAAAAZhbW91bnQAAAAAAAsAAAAAAAAABGZyb20AAAATAAAAAAAAAAJ0bwAAAAAAEwAAAAAAAAAIdG9rZW5faWQAAAfQAAAAB1Rva2VuSWQA",
            "AAAAAQAAAJJSb3lhbHR5IGluZm9ybWF0aW9uIHN0b3JlZCBwZXIgdG9rZW4uCmBhc3NldF9hZGRyZXNzYCBpcyBgTm9uZWAgZm9yIG5hdGl2ZSBYTE0sIG9yIGBTb21lKGNvbnRyYWN0X2FkZHJlc3MpYApmb3IgYW55IFNFUC0wMDQxIGN1c3RvbSBTdGVsbGFyIGFzc2V0LgAAAAAAAAAAABBSb3lhbHR5UmVjaXBpZW50AAAAAgAAAAAAAAAMYmFzaXNfcG9pbnRzAAAABAAAAAAAAAAJcmVjaXBpZW50AAAAAAAAEw==",
            "AAAAAAAAAFRVbnBhdXNlIHRoZSBjb250cmFjdCwgcmUtZW5hYmxpbmcgYG1pbnRgIGFuZCBgdHJhbnNmZXJgLgpPbmx5IGNhbGxhYmxlIGJ5IHRoZSBhZG1pbi4AAAAHdW5wYXVzZQAAAAABAAAAAAAAAAVhZG1pbgAAAAAAABMAAAABAAAD6QAAAAIAAAAD",
            "AAAAAAAAAB1SZXR1cm5zIHRoZSBjb250cmFjdCB2ZXJzaW9uLgAAAAAAAAd2ZXJzaW9uAAAAAAAAAAABAAAABA==",
            "AAAAAAAAACZSZXR1cm5zIHRoZSBvd25lciBvZiBhIGdpdmVuIHRva2VuIElELgAAAAAACG93bmVyX29mAAAAAQAAAAAAAAAIdG9rZW5faWQAAAfQAAAAB1Rva2VuSWQAAAAAAQAAA+kAAAATAAAAAw==",
            "AAAAAAAAAQZUcmFuc2ZlciBORlQgb3duZXJzaGlwIGZyb20gYGZyb21gIHRvIGB0b2AuCgpCbG9ja2VkIGlmIHRoZSB0b2tlbiBpcyBzb3VsYm91bmQgKG5vbi10cmFuc2ZlcmFibGUpLgpTdG9yYWdlIHdyaXRlcyAocGVyc2lzdGVudCk6IFRva2VuRGF0YSA9ICoqMSoqCgojIEFyZ3VtZW50cwoqIGBmcm9tYCAgICAgLSBDdXJyZW50IG93bmVyIChtdXN0IGF1dGhvcml6ZSkKKiBgdG9gICAgICAgIC0gTmV3IG93bmVyCiogYHRva2VuX2lkYCAtIFRva2VuIHRvIHRyYW5zZmVyAAAAAAAIdHJhbnNmZXIAAAADAAAAAAAAAARmcm9tAAAAEwAAAAAAAAACdG8AAAAAABMAAAAAAAAACHRva2VuX2lkAAAH0AAAAAdUb2tlbklkAAAAAAEAAAPpAAAAAgAAAAM=",
            "AAAAAAAAADNSZXR1cm5zIGB0cnVlYCBpZiB0aGUgY29udHJhY3QgaXMgY3VycmVudGx5IHBhdXNlZC4AAAAACWlzX3BhdXNlZAAAAAAAAAAAAAABAAAAAQ==",
            "AAAAAAAAAC5SZXR1cm5zIHRoZSBtZXRhZGF0YSBVUkkgZm9yIGEgZ2l2ZW4gdG9rZW4gSUQuAAAAAAAJdG9rZW5fdXJpAAAAAAAAAQAAAAAAAAAIdG9rZW5faWQAAAfQAAAAB1Rva2VuSWQAAAAAAQAAA+kAAAAQAAAAAw==",
            "AAAAAAAAAEJSZXR1cm4gdGhlIGN1cnJlbnRseSByZWdpc3RlcmVkIGJhY2tlbmQgc2lnbmVyIHB1YmxpYyBrZXksIGlmIGFueS4AAAAAAApnZXRfc2lnbmVyAAAAAAAAAAAAAQAAA+gAAAPuAAAAIA==",
            "AAAAAAAAAPhSZWdpc3RlciAob3Igcm90YXRlKSB0aGUgYmFja2VuZCBFZDI1NTE5IHB1YmxpYyBrZXkgdXNlZCB0byB2ZXJpZnkKY2xpcCBvd25lcnNoaXAgYmVmb3JlIG1pbnRpbmcuIE9ubHkgY2FsbGFibGUgYnkgdGhlIGFkbWluLgoKIyBBcmd1bWVudHMKKiBgYWRtaW5gICAtIE11c3QgYmUgdGhlIGNvbnRyYWN0IGFkbWluCiogYHB1YmtleWAgLSAzMi1ieXRlIEVkMjU1MTkgcHVibGljIGtleSBvZiB0aGUgdHJ1c3RlZCBiYWNrZW5kIHNpZ25lcgAAAApzZXRfc2lnbmVyAAAAAAACAAAAAAAAAAVhZG1pbgAAAAAAABMAAAAAAAAABnB1YmtleQAAAAAD7gAAACAAAAABAAAD6QAAAAIAAAAD",
            "AAAAAAAAADBSZXR1cm5zIHRoZSBzdG9yZWQgYFJveWFsdHlgIHN0cnVjdCBmb3IgYSB0b2tlbi4AAAALZ2V0X3JveWFsdHkAAAAAAQAAAAAAAAAIdG9rZW5faWQAAAfQAAAAB1Rva2VuSWQAAAAAAQAAA+kAAAfQAAAAB1JveWFsdHkAAAAAAw==",
            "AAAAAAAAAMZQYXkgcm95YWx0aWVzIGZvciBhIHRva2VuIHNhbGUgdXNpbmcgdGhlIGFzc2V0IGNvbmZpZ3VyZWQgaW4gdGhlIHJveWFsdHkuCgpPbmx5IGhhbmRsZXMgU0VQLTAwNDEgY3VzdG9tIGFzc2V0cy4gRm9yIFhMTSAoYGFzc2V0X2FkZHJlc3NgIGlzIGBOb25lYCkKdGhlIG1hcmtldHBsYWNlIG11c3QgaGFuZGxlIHRoZSB0cmFuc2ZlciBkaXJlY3RseS4AAAAAAAtwYXlfcm95YWx0eQAAAAADAAAAAAAAAAVwYXllcgAAAAAAABMAAAAAAAAACHRva2VuX2lkAAAH0AAAAAdUb2tlbklkAAAAAAAAAAAKc2FsZV9wcmljZQAAAAAACwAAAAEAAAPpAAAAAgAAAAM=",
            "AAAAAAAAAIFVcGRhdGUgdGhlIHJveWFsdHkgY29uZmlndXJhdGlvbiBmb3IgYSB0b2tlbi4gQWRtaW4gb25seS4KRW1pdHMgUm95YWx0eVJlY2lwaWVudFVwZGF0ZWQgZXZlbnQgd2hlbiB0aGUgcHJpbWFyeSByZWNpcGllbnQgY2hhbmdlcy4AAAAAAAALc2V0X3JveWFsdHkAAAAAAwAAAAAAAAAFYWRtaW4AAAAAAAATAAAAAAAAAAh0b2tlbl9pZAAAB9AAAAAHVG9rZW5JZAAAAAAAAAAAC25ld19yb3lhbHR5AAAAB9AAAAAHUm95YWx0eQAAAAABAAAD6QAAAAIAAAAD",
            "AAAAAAAAAC5BbGlhcyBmb3IgYHRva2VuX3VyaWAsIGtlcHQgZm9yIGNvbXBhdGliaWxpdHkuAAAAAAAMZ2V0X21ldGFkYXRhAAAAAQAAAAAAAAAIdG9rZW5faWQAAAfQAAAAB1Rva2VuSWQAAAAAAQAAA+kAAAAQAAAAAw==",
            "AAAAAAAAADpSZXR1cm5zIHRydWUgaWYgdGhlIHRva2VuIGlzIHNvdWxib3VuZCAobm9uLXRyYW5zZmVyYWJsZSkuAAAAAAAMaXNfc291bGJvdW5kAAAAAQAAAAAAAAAIdG9rZW5faWQAAAfQAAAAB1Rva2VuSWQAAAAAAQAAAAE=",
            "AAAAAAAAARpSZXR1cm5zIHRoZSByb3lhbHR5IHJlY2VpdmVyLCBhbW91bnQsIGFuZCBwYXltZW50IGFzc2V0IGZvciBhIGdpdmVuIHNhbGUgcHJpY2UuCgpVc2VzIHNhZmUgbWF0aCB0byBwcmV2ZW50IG92ZXJmbG93LiBSb3lhbHR5IGFtb3VudCBpcyBjYWxjdWxhdGVkIGFzOgpgcm95YWx0eV9hbW91bnQgPSBzYWxlX3ByaWNlICogYmFzaXNfcG9pbnRzIC8gMTAwMDBgCgpTYWZlIGxpbWl0czogc2FsZV9wcmljZSBzaG91bGQgbm90IGV4Y2VlZCBpMTI4OjpNQVggLyAxMDAwMCB0byBhdm9pZCBvdmVyZmxvdy4AAAAAAAxyb3lhbHR5X2luZm8AAAACAAAAAAAAAAh0b2tlbl9pZAAAB9AAAAAHVG9rZW5JZAAAAAAAAAAACnNhbGVfcHJpY2UAAAAAAAsAAAABAAAD6QAAB9AAAAALUm95YWx0eUluZm8AAAAAAw==",
            "AAAAAAAAAH9SZXR1cm5zIHRoZSB0b3RhbCBudW1iZXIgb2YgbWludGVkIChhbmQgbm90IHlldCBidXJuZWQpIHRva2Vucy4KCkRlcml2ZWQgZnJvbSBgTmV4dFRva2VuSWQgLSAxYCDigJQgbm8gc2VwYXJhdGUgY291bnRlciBuZWVkZWQuAAAAAAx0b3RhbF9zdXBwbHkAAAAAAAAAAQAAAAQ=",
            "AAAAAAAAADJMb29rIHVwIHRoZSBvbi1jaGFpbiB0b2tlbiBJRCBmb3IgYSBnaXZlbiBjbGlwX2lkLgAAAAAADWNsaXBfdG9rZW5faWQAAAAAAAABAAAAAAAAAAdjbGlwX2lkAAAAAAQAAAABAAAD6QAAB9AAAAAHVG9rZW5JZAAAAAAD",
            "AAAAAAAAAFlSZXR1cm5zIHRoZSBhdmVyYWdlIHN5bnRoZXRpYyBnYXMgY29zdCBmb3IgYSBnaXZlbiBvcGVyYXRpb24gdHlwZS4KMCA9IE1pbnQsIDEgPSBUcmFuc2ZlcgAAAAAAABBnZXRfYXZnX2dhc19jb3N0AAAAAQAAAAAAAAAHb3BfdHlwZQAAAAAEAAAAAQAAAAY=",
            "AAAAAQAAADBFdmVudCBlbWl0dGVkIHdoZW4gcm95YWx0eSByZWNpcGllbnQgaXMgdXBkYXRlZC4AAAAAAAAAHFJveWFsdHlSZWNpcGllbnRVcGRhdGVkRXZlbnQAAAADAAAAAAAAAA1uZXdfcmVjaXBpZW50AAAAAAAAEwAAAAAAAAANb2xkX3JlY2lwaWVudAAAAAAAABMAAAAAAAAACHRva2VuX2lkAAAH0AAAAAdUb2tlbklkAA=="]), options);
        this.options = options;
    }
    fromJSON = {
        burn: (this.txFromJSON),
        init: (this.txFromJSON),
        mint: (this.txFromJSON),
        pause: (this.txFromJSON),
        exists: (this.txFromJSON),
        unpause: (this.txFromJSON),
        version: (this.txFromJSON),
        owner_of: (this.txFromJSON),
        transfer: (this.txFromJSON),
        is_paused: (this.txFromJSON),
        token_uri: (this.txFromJSON),
        get_signer: (this.txFromJSON),
        set_signer: (this.txFromJSON),
        get_royalty: (this.txFromJSON),
        pay_royalty: (this.txFromJSON),
        set_royalty: (this.txFromJSON),
        get_metadata: (this.txFromJSON),
        is_soulbound: (this.txFromJSON),
        royalty_info: (this.txFromJSON),
        total_supply: (this.txFromJSON),
        clip_token_id: (this.txFromJSON),
        get_avg_gas_cost: (this.txFromJSON),
        blacklist_clip: (this.txFromJSON),
        update_royalty_recipient: (this.txFromJSON),
        tokens_of_owner: (this.txFromJSON),
        calculate_royalty_amount: (this.txFromJSON),
        batch_mint: (this.txFromJSON)
    };
}
