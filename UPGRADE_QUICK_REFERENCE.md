# ClipsNFT Contract Upgrade — Quick Reference Card

## Pre-Upgrade (Do This First)

- [ ] Ensure network connectivity: `soroban network list`
- [ ] Check account balance: `soroban account balance --account default`
- [ ] Verify contract ID: `cat .soroban/contract-id-testnet`
- [ ] Run all tests: `cargo test` (should all pass)
- [ ] Build release WASM: `cargo build --target wasm32-unknown-unknown --release -p clips_nft`

## Dry-Run (Recommended)

```bash
DRY_RUN=1 ./scripts/upgrade.sh testnet
```

Expected output:
- Pre-flight snapshot (version, supply, admin)
- [DRY-RUN] messages for each step
- No [ERROR] messages
- Summary at end

## Execute Upgrade

```bash
./scripts/upgrade.sh testnet
# OR with explicit contract ID:
./scripts/upgrade.sh testnet CBXY...Z
```

Expected duration: 2-5 minutes depending on network

## What to Monitor During Upgrade

| Step | Expected Output | What Could Go Wrong |
|------|-----------------|---------------------|
| Pre-flight | Version, supply, admin printed | "Cannot connect to contract" |
| Build | "Finished release" | "build failed" |
| Install WASM | "New WASM hash: ABC..." | "Failed to install" |
| upgrade() | ✓ upgrade() succeeded | "Failed to invoke upgrade" |
| migrate() | ✓ migrate() succeeded | "Failed to invoke migrate" |
| Post-flight | Version bumped, supply unchanged | "supply changed" |

## Post-Upgrade Validation

```bash
# Check version increased
soroban contract invoke --id CBXY... --network testnet -- contract_version

# Check supply unchanged
soroban contract invoke --id CBXY... --network testnet -- total_supply

# Test a basic operation (e.g., get user tokens)
soroban contract invoke --id CBXY... --network testnet -- \
  tokens_of_owner --owner GXXX... --limit 10 --offset 0
```

## If Something Goes Wrong

### Supply Changed During Upgrade

**Action**: IMMEDIATE ROLLBACK

```bash
source .soroban/rollback-testnet.env
./scripts/rollback.sh testnet
# Type: ROLLBACK
```

Then contact development team.

### Timeout / Hang

**Action**: Check RPC status, retry

```bash
# Check RPC health
curl https://soroban-testnet.stellar.org:443/health

# Retry upgrade
./scripts/upgrade.sh testnet
```

### Version Didn't Bump

**Action**: Manually call migrate()

```bash
ADMIN_ADDR=$(soroban config identity address default)
soroban contract invoke --id CBXY... --source default --network testnet -- \
  migrate --admin "$ADMIN_ADDR"
```

### Rollback Failed

**Action**: Contact development team with:
- Error message from rollback script
- .soroban/backup-*.env file
- Recent transaction hashes from Stellar Expert

## Rollback (Emergency Only)

```bash
source .soroban/rollback-testnet.env
./scripts/rollback.sh testnet
```

You will be prompted to type "ROLLBACK" to confirm.

## Environment Variables

```bash
# Use different account
SOROBAN_ACCOUNT=admin ./scripts/upgrade.sh testnet

# Override contract ID
./scripts/upgrade.sh testnet CBXY...Z

# Dry run
DRY_RUN=1 ./scripts/upgrade.sh testnet

# Multiple together
SOROBAN_ACCOUNT=admin DRY_RUN=1 ./scripts/upgrade.sh mainnet
```

## Key Numbers to Track

Record these before and after upgrade:

| Metric | Before | After | Status |
|--------|--------|-------|--------|
| contract_version | ___ | ___ | ☐ Increased |
| total_supply | ___ | ___ | ☐ Unchanged |
| admin_address | ___ | ___ | ☐ Unchanged |
| WASM hash | ___ | ___ | ☐ New hash |

## Logs & Artifacts

| File | Purpose | Keep For |
|------|---------|----------|
| `.soroban/rollback-testnet.env` | Emergency recovery | 7 days |
| `.soroban/backup-*.env` | State backup (on rollback) | 7 days |
| Terminal output | Upgrade transcript | 30 days |
| Contract events | Audit trail | Archive |

## Success Criteria

✅ All these must be true:

- [ ] No ERROR messages in output
- [ ] Post-flight validation passed
- [ ] contract_version increased
- [ ] total_supply unchanged (same value before/after)
- [ ] All NFTs still accessible
- [ ] Royalty system still works
- [ ] New features (if any) working

## Failure Recovery

1. **Identify the issue** (see above)
2. **Rollback if needed**: `source rollback-*.env && ./scripts/rollback.sh testnet`
3. **Investigate** the root cause
4. **Fix in code** and test locally
5. **Re-upgrade** from step 1

## Contacts

- Development Team: [contact info]
- Stellar RPC Status: https://status.stellar.org/
- Smart Contract Help: https://developers.stellar.org/docs/smart-contracts

## Useful Commands

```bash
# Get contract state
soroban contract invoke --id $CONTRACT_ID --network testnet -- contract_info

# Check specific user's NFTs
soroban contract invoke --id $CONTRACT_ID --network testnet -- \
  tokens_of_owner --owner $USER_ADDRESS --limit 10 --offset 0

# Get royalty for a token
soroban contract invoke --id $CONTRACT_ID --network testnet -- \
  get_royalty --token_id 1

# View transaction on explorer
# Open: https://stellar.expert/explorer/testnet/tx/[HASH]
```

## Notes

```
Upgrade timestamp: _______________
Executed by: _______________
Network: _______________
Contract ID: _______________

Notes:
_________________________________________________________________
_________________________________________________________________
_________________________________________________________________
```

---

**Print this card and keep it during upgrades!**

**Last Updated**: May 28, 2026
