# ClipsNFT Contract Upgrade Guide

This document describes the safe upgrade mechanism for the ClipsNFT contract, including pre-flight checks, data migration, and rollback procedures.

## Overview

The ClipsNFT contract upgrade system uses a two-phase approach:

1. **`upgrade()`** — Swaps the contract code without touching storage
2. **`migrate()`** — Handles data migrations and version bumping

This ensures existing NFTs and royalty information are preserved across versions.

## Architecture

### Storage Separation

Soroban contract storage is separated into three types:

- **Instance Storage** — Persists across upgrades (contract version, admin, total supply, etc.)
- **Persistent Storage** — Persists across upgrades (NFT data, royalties, balances, etc.)
- **Temporary Storage** — Cleared on each upgrade (gas counters, transient caches, etc.)

All critical data (NFTs, royalties, balances) is stored in Persistent storage, so it survives upgrades.

### Version Management

- The contract version is stored in `DataKey::ContractVersion`
- On deployment, the version is initialized to `VERSION` constant
- On each upgrade, `migrate()` bumps the version
- Version changes are emitted as `MigratedEvent`

## Prerequisites

Before upgrading, ensure:

1. **Rust toolchain updated**
   ```bash
   rustup update
   rustup target add wasm32-unknown-unknown
   ```

2. **Soroban CLI installed**
   ```bash
   cargo install --locked stellar-cli
   ```

3. **Account and network configured**
   ```bash
   soroban config identity create default
   soroban config network add testnet --rpc-url https://soroban-testnet.stellar.org:443
   ```

4. **Account has sufficient XLM** for transaction fees (~1 XLM minimum)

## Upgrade Procedure

### Step 1: Prepare and Test

Before touching any live contract, test in your local environment:

```bash
# Run all tests including migration tests
cargo test --test upgrade_migration
cargo test --lib

# Build the release WASM
cargo build --target wasm32-unknown-unknown --release -p clips_nft
```

Ensure all tests pass, especially the migration tests.

### Step 2: Dry Run (Recommended)

Simulate the upgrade without making any on-chain changes:

```bash
export CONTRACT_ID="CBXY..."
DRY_RUN=1 ./scripts/upgrade.sh testnet
```

This will:
- Check current contract state
- Build the new WASM
- Show what would be installed
- Verify post-flight validations would pass

### Step 3: Execute Upgrade

When ready, perform the actual upgrade:

```bash
./scripts/upgrade.sh testnet
```

The script will:

1. **Pre-flight snapshot**
   - Read current version, supply, admin
   - Capture current WASM hash

2. **Build new WASM**
   - Compile contract in release mode
   - Verify WASM was created

3. **Install WASM on-chain**
   - Upload new code to blockchain
   - Capture new WASM hash

4. **Create rollback artefact**
   - Save `.soroban/rollback-testnet.env`
   - Document pre-upgrade state for emergency recovery

5. **Call `upgrade()`**
   - Swap contract code
   - All storage remains intact

6. **Call `migrate()`**
   - Execute any version-specific migrations
   - Bump `ContractVersion`
   - Emit `MigratedEvent`

7. **Post-flight verification**
   - Verify total supply unchanged
   - Confirm version increased
   - Validate all NFTs preserved

### Step 4: Verify Success

After upgrade completes, verify the contract is working correctly:

```bash
# Check current version
soroban contract invoke --id $CONTRACT_ID --network testnet -- contract_version

# Check total supply
soroban contract invoke --id $CONTRACT_ID --network testnet -- total_supply

# Check a specific NFT exists
soroban contract invoke --id $CONTRACT_ID --network testnet -- \
  balance_of \
  --owner GXXX...

# Verify contract info
soroban contract invoke --id $CONTRACT_ID --network testnet -- contract_info
```

Run integration tests:

```bash
cargo test --test integration -- --nocapture
```

## Data Preservation

The upgrade mechanism preserves:

✓ All minted NFTs
✓ NFT ownership and metadata
✓ Royalty information and recipients
✓ Token balances for all users
✓ Admin authorization
✓ Contract configuration (fees, settings)

The only change is the contract code and the version number.

## Migration Framework

### Adding Version-Specific Migrations

When future versions need to transform data, add logic to `migrate()`:

```rust
pub fn migrate(env: Env, admin: Address) -> Result<(), Error> {
    admin.require_auth();

    let current_admin: Address = env.storage().instance().get(&DataKey::Admin)
        .ok_or(Error::Unauthorized)?;
    if current_admin != admin {
        return Err(Error::Unauthorized);
    }

    let from_version: u32 = env.storage().instance().get(&DataKey::ContractVersion)
        .unwrap_or(1);

    if from_version >= VERSION {
        return Ok(());
    }

    // Migration v1 -> v2
    if from_version == 1 {
        // Add custom migration logic here
        // Example: transform old data structure to new format
        // env.storage().persistent().set(...);
    }

    // Migration v2 -> v3 would go here
    if from_version == 2 {
        // ...
    }

    env.storage().instance().set(&DataKey::ContractVersion, &VERSION);
    env.events().publish(
        (symbol_short!("migrate"),),
        MigratedEvent {
            from_version,
            to_version: VERSION,
        },
    );

    Ok(())
}
```

### Key Rules

1. **Preserving Data**: Always read old data before transforming
2. **Backwards Compatibility**: Later code should handle both old and new formats
3. **Idempotency**: `migrate()` should be safe to call multiple times
4. **Logging**: Emit events for audit trails

## Rollback Procedure

If the upgrade introduces critical bugs, you can rollback to the previous version:

### Emergency Rollback

```bash
# Source the rollback artefact created during upgrade
source .soroban/rollback-testnet.env

# Execute rollback
./scripts/rollback.sh testnet
```

The script will:

1. **Confirm your intent** — You must type "ROLLBACK" to proceed
2. **Backup current state** — Save a timestamped backup
3. **Restore previous WASM** — Swap code back to old version
4. **Verify integrity** — Check supply and data consistency

### Rollback Limitations

- ⚠️ Rollback restores the **old code**, not old data
- If the upgrade modified data, you must decide:
  - Keep the new data with old code (may cause inconsistencies)
  - Accept partial data loss by reverting to old backups
- Rollback is for **code failures only**, not data recovery

### Recovery After Rollback

1. **Investigate** — Determine what failed in the upgrade
2. **Fix** — Correct the issue in contract code
3. **Retest** — Run full test suite before re-attempting
4. **Redeploy** — Perform a new upgrade with fixes

## Testing the Upgrade System

### Unit Tests

The migration test suite includes:

- `test_upgrade_and_migrate_preserves_state` — Basic upgrade flow
- `test_nfts_preserved_during_upgrade` — NFT persistence
- `test_royalties_preserved_during_upgrade` — Royalty preservation
- `test_version_bumped_on_migrate` — Version management
- `test_non_admin_cannot_upgrade` — Authorization checks
- `test_contract_info_after_upgrade` — Metadata preservation
- `test_multiple_nfts_with_varied_royalties_preserved` — Complex scenarios
- `test_migrate_idempotent` — Safe re-invocation

Run tests:

```bash
cargo test --test upgrade_migration -- --nocapture
```

### Integration Tests

Before upgrading mainnet, test against testnet or a staging environment:

```bash
# Deploy to testnet
./scripts/deploy-testnet.sh

# Create some test NFTs
cargo test --test integration -- mint_tests

# Perform upgrade
./scripts/upgrade.sh testnet

# Verify
cargo test --test integration -- verify_after_upgrade
```

## Mainnet Upgrade Checklist

- [ ] All tests pass locally (`cargo test`)
- [ ] Testnet upgrade successful
- [ ] Post-upgrade validation passed
- [ ] No reported issues from testnet
- [ ] Dry-run on mainnet verified
- [ ] Stakeholders informed
- [ ] Rollback procedure documented and tested
- [ ] Team available for monitoring (first 24 hours)

## Monitoring After Upgrade

### First Hour

- Monitor transaction logs for errors
- Verify new NFTs can be minted
- Spot-check a few existing NFTs
- Monitor gas usage

### First Day

- Review contract events for any anomalies
- Verify royalty payments still work
- Check user-facing features
- Monitor for performance regressions

### Ongoing

- Keep rollback artefact for 7+ days
- Monitor contract version in wallets
- Plan for next upgrade if needed

## Troubleshooting

### Upgrade Hangs or Times Out

```bash
# Check Soroban RPC status
curl -s https://soroban-testnet.stellar.org:443/health | jq .

# Retry with explicit timeout
timeout 600 ./scripts/upgrade.sh testnet
```

### Supply Changed During Upgrade

This indicates data corruption:

```bash
# Immediately rollback
source .soroban/rollback-testnet.env
./scripts/rollback.sh testnet

# Contact development team with:
# - The backup file (.soroban/backup-*.env)
# - Transaction hashes from the failed upgrade
# - Contract logs
```

### Version Didn't Bump

```bash
# Verify migrate() was called
soroban contract invoke --id $CONTRACT_ID --network testnet -- contract_version

# If still old version, call migrate() again
soroban contract invoke --id $CONTRACT_ID --source default --network testnet -- migrate --admin GXXX...
```

### Can't Rollback

If rollback fails:

1. Determine if the issue is code or data
2. If code: manually reinstall previous WASM hash
3. If data: restore from backup using recovery scripts
4. Contact development team

## Environment Variables

Control upgrade behavior with environment variables:

| Variable | Default | Purpose |
|----------|---------|---------|
| `SOROBAN_ACCOUNT` | `default` | Soroban account alias to use |
| `CONTRACT_ID` | (from file) | Contract address to upgrade |
| `DRY_RUN` | `0` | Simulate without executing (1 to enable) |
| `NETWORK` | `testnet` | Network to upgrade (testnet/mainnet) |

Example:

```bash
DRY_RUN=1 SOROBAN_ACCOUNT=admin NETWORK=mainnet ./scripts/upgrade.sh
```

## FAQs

**Q: Will my NFTs survive the upgrade?**

A: Yes. All persistent storage (NFTs, royalties, balances) is preserved. The only change is the contract code.

**Q: What happens if I lose the rollback artefact?**

A: You can still rollback, but you'll need to manually provide the previous WASM hash. Keep backups for at least 7 days.

**Q: Can I upgrade multiple contracts in sequence?**

A: Yes, but do them one at a time. Upgrade each contract, verify success, then move to the next.

**Q: What if I need to downgrade to an older version (v1 → v0)?**

A: Downgrades are not supported. Always upgrade forward. If you need old code, deploy a separate contract instance.

**Q: How do I know if the upgrade succeeded?**

A: Check:
1. No errors in script output
2. Post-flight validation passed
3. `contract_version` increased
4. `total_supply` unchanged
5. NFTs still accessible

**Q: Can I pause minting during upgrade?**

A: Yes. Call `pause_contract()` before upgrading, then `unpause()` after verification.

## Related Files

- **Contract code**: [clips_nft/src/lib.rs](clips_nft/src/lib.rs)
- **Upgrade script**: [scripts/upgrade.sh](scripts/upgrade.sh)
- **Rollback script**: [scripts/rollback.sh](scripts/rollback.sh)
- **Migration tests**: [clips_nft/tests/upgrade_migration.rs](clips_nft/tests/upgrade_migration.rs)
- **Deployment guide**: [README.md](README.md)

## Support

For issues or questions:

1. Check this guide's Troubleshooting section
2. Review the script logs
3. Check contract events on Stellar Expert
4. Contact the development team

---

**Last Updated**: May 2026
**Compatible With**: ClipsNFT v0.1.0+
**Soroban SDK Version**: 25.3.1+
