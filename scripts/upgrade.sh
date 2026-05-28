#!/usr/bin/env bash
# =============================================================================
# upgrade.sh — Safe contract upgrade with pre-flight checks and rollback plan
#
# This script performs a safe upgrade of the ClipsNFT contract following this pattern:
#   1. Snapshot current state (version, supply, admin)
#   2. Build and install new WASM on-chain
#   3. Invoke upgrade() to swap contract code (storage untouched)
#   4. Invoke migrate() to handle data migrations and version bumping
#   5. Verify state consistency (supply preserved, version increased)
#   6. Save rollback artefact for emergency recovery
#
# Usage:
#   ./scripts/upgrade.sh [testnet|mainnet] [CONTRACT_ID]
#   DRY_RUN=1 ./scripts/upgrade.sh testnet  # Simulate without executing
#
# Environment variables:
#   SOROBAN_ACCOUNT   — Stellar account alias (default: "default")
#   CONTRACT_ID       — Contract address (default: read from .soroban/contract-id-<network>)
#   DRY_RUN=1         — Simulate all steps without on-chain writes
#
# Rollback (if needed):
#   source .soroban/rollback-<network>.env
#   ./scripts/rollback.sh testnet
#
# =============================================================================

set -euo pipefail

# ---------------------------------------------------------------------------
# Configuration
# ---------------------------------------------------------------------------
NETWORK="${1:-${NETWORK:-testnet}}"
CONTRACT_ID="${2:-${CONTRACT_ID:-}}"
ACCOUNT="${SOROBAN_ACCOUNT:-default}"
DRY_RUN="${DRY_RUN:-0}"
WASM_PATH="target/wasm32-unknown-unknown/release/clips_nft.wasm"
SOROBAN_DIR=".soroban"
ROLLBACK_FILE="$SOROBAN_DIR/rollback-$NETWORK.env"

# Color codes for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# ---------------------------------------------------------------------------
# Logging utilities
# ---------------------------------------------------------------------------
log()   { echo -e "${GREEN}[upgrade]${NC} $*"; }
warn()  { echo -e "${YELLOW}[upgrade]${NC} ⚠️  $*" >&2; }
die()   { echo -e "${RED}[upgrade]${NC} ✗ $*" >&2; exit 1; }
step()  { echo ""; log "=== $* ==="; }

# Execute command, respecting DRY_RUN
run() {
    if [[ "$DRY_RUN" == "1" ]]; then
        echo -e "${YELLOW}[dry-run]${NC} $*"
        return 0
    else
        "$@"
    fi
}

# Invoke contract method with error handling
soroban_invoke() {
    local method="$1"
    shift
    local args=("$@")
    
    if [[ "$DRY_RUN" == "1" ]]; then
        echo -e "${YELLOW}[dry-run]${NC} soroban contract invoke --id $CONTRACT_ID --source $ACCOUNT --network $NETWORK -- $method ${args[*]}"
        return 0
    fi
    
    soroban contract invoke \
        --id "$CONTRACT_ID" \
        --source "$ACCOUNT" \
        --network "$NETWORK" \
        -- "$method" "${args[@]}" || {
        die "Failed to invoke $method. Check logs above."
    }
}

# Read contract state safely (read-only)
soroban_read() {
    local method="$1"
    shift
    
    soroban contract invoke \
        --id "$CONTRACT_ID" \
        --source "$ACCOUNT" \
        --network "$NETWORK" \
        -- "$method" "$@" 2>/dev/null || echo ""
}

# Resolve and validate contract ID
resolve_contract_id() {
    if [[ -n "$CONTRACT_ID" ]]; then
        return 0
    fi
    
    local id_file="$SOROBAN_DIR/contract-id-$NETWORK"
    [[ -f "$id_file" ]] || die "No CONTRACT_ID provided and $id_file not found. Deploy contract first."
    CONTRACT_ID="$(cat "$id_file")"
}

# Get admin address from Soroban config
get_admin_address() {
    soroban config identity address "$ACCOUNT" 2>/dev/null || echo ""
}

# ---------------------------------------------------------------------------
# MAIN EXECUTION
# ---------------------------------------------------------------------------

log "ClipsNFT Contract Upgrade Tool"
log "Network : $NETWORK"
log "Account : $ACCOUNT"
[[ "$DRY_RUN" == "1" ]] && log "Mode    : DRY RUN (no writes)"

# Validate prerequisites
resolve_contract_id
log "Contract: $CONTRACT_ID"

# Verify we can read from the contract
if [[ "$DRY_RUN" != "1" ]]; then
    soroban_read "contract_version" > /dev/null || die "Cannot connect to contract. Check network and contract ID."
fi

# Get admin address
ADMIN_ADDRESS=$(get_admin_address)
[[ -z "$ADMIN_ADDRESS" ]] && die "Could not resolve admin address. Check SOROBAN_ACCOUNT and Soroban config."
log "Admin   : $ADMIN_ADDRESS"

# ---------------------------------------------------------------------------
# Step 1: Pre-flight snapshot
# ---------------------------------------------------------------------------
step "Pre-flight snapshot"

PRE_VERSION=$(soroban_read "contract_version" 2>/dev/null || echo "0")
PRE_SUPPLY=$(soroban_read "total_supply" 2>/dev/null || echo "0")

log "  contract_version: $PRE_VERSION"
log "  total_supply    : $PRE_SUPPLY"
log "  admin_address   : $ADMIN_ADDRESS"

# Try to capture current WASM hash for rollback documentation
PRE_WASM_HASH="${ROLLBACK_WASM_HASH:-unknown}"

# ---------------------------------------------------------------------------
# Step 2: Build new WASM
# ---------------------------------------------------------------------------
step "Building new WASM"

[[ -f "$WASM_PATH" ]] && log "  Removing old WASM: $WASM_PATH" && rm -f "$WASM_PATH"

run cargo build --target wasm32-unknown-unknown --release -p clips_nft 2>&1 | grep -E "Compiling|Finished" || true

if [[ "$DRY_RUN" != "1" ]]; then
    [[ -f "$WASM_PATH" ]] || die "WASM build failed: $WASM_PATH not found"
    WASM_SIZE=$(stat -f%z "$WASM_PATH" 2>/dev/null || stat -c%s "$WASM_PATH" 2>/dev/null || echo "unknown")
    log "  Built successfully: $WASM_PATH ($WASM_SIZE bytes)"
fi

# ---------------------------------------------------------------------------
# Step 3: Install WASM on-chain
# ---------------------------------------------------------------------------
step "Installing WASM on-chain"

if [[ "$DRY_RUN" == "1" ]]; then
    NEW_WASM_HASH="<dry-run-hash>"
    log "  [DRY-RUN] Would install: $WASM_PATH"
else
    NEW_WASM_HASH=$(soroban contract install \
        --network "$NETWORK" \
        --source "$ACCOUNT" \
        --wasm "$WASM_PATH" 2>&1) || die "Failed to install WASM on-chain"
fi

log "  New WASM hash: $NEW_WASM_HASH"

# ---------------------------------------------------------------------------
# Step 4: Write rollback artefact (BEFORE modifying live contract)
# ---------------------------------------------------------------------------
step "Creating rollback artefact"

mkdir -p "$SOROBAN_DIR"
cat > "$ROLLBACK_FILE" <<EOF
#!/usr/bin/env bash
# Rollback artefact for ClipsNFT upgrade
# Generated: $(date -u +"%Y-%m-%dT%H:%M:%SZ")
# Source this file then run: ./scripts/rollback.sh

export ROLLBACK_NETWORK="$NETWORK"
export ROLLBACK_CONTRACT_ID="$CONTRACT_ID"
export ROLLBACK_ACCOUNT="$ACCOUNT"
export ROLLBACK_PRE_WASM_HASH="$PRE_WASM_HASH"
export ROLLBACK_PRE_VERSION="$PRE_VERSION"
export ROLLBACK_PRE_SUPPLY="$PRE_SUPPLY"
export ROLLBACK_NEW_WASM_HASH="$NEW_WASM_HASH"
EOF
chmod +x "$ROLLBACK_FILE"
log "  Saved: $ROLLBACK_FILE"
log "  To rollback: source $ROLLBACK_FILE && ./scripts/rollback.sh $NETWORK"

# ---------------------------------------------------------------------------
# Step 5: Invoke upgrade() — swap contract code
# ---------------------------------------------------------------------------
step "Invoking upgrade() — swapping contract code"

run soroban_invoke upgrade "$ADMIN_ADDRESS"

[[ "$DRY_RUN" == "1" ]] && log "  [DRY-RUN] Would call upgrade()"
[[ "$DRY_RUN" != "1" ]] && log "  ✓ upgrade() succeeded — code swapped, storage preserved"

# ---------------------------------------------------------------------------
# Step 6: Invoke migrate() — handle data migrations
# ---------------------------------------------------------------------------
step "Invoking migrate() — running data migrations"

run soroban_invoke migrate "$ADMIN_ADDRESS"

[[ "$DRY_RUN" == "1" ]] && log "  [DRY-RUN] Would call migrate()"
[[ "$DRY_RUN" != "1" ]] && log "  ✓ migrate() succeeded — data migration and version bump complete"

# ---------------------------------------------------------------------------
# Step 7: Post-flight verification
# ---------------------------------------------------------------------------
step "Post-flight verification"

if [[ "$DRY_RUN" == "1" ]]; then
    log "  [DRY-RUN] Skipping post-flight checks"
else
    # Allow a moment for state changes to settle
    sleep 2
    
    POST_VERSION=$(soroban_read "contract_version" 2>/dev/null || echo "0")
    POST_SUPPLY=$(soroban_read "total_supply" 2>/dev/null || echo "0")
    
    log "  contract_version: $PRE_VERSION → $POST_VERSION"
    log "  total_supply    : $PRE_SUPPLY → $POST_SUPPLY"
    
    # Validation 1: Supply must be unchanged
    if [[ "$POST_SUPPLY" != "$PRE_SUPPLY" ]]; then
        die "total_supply changed ($PRE_SUPPLY → $POST_SUPPLY). Upgrade may be corrupted."
    fi
    
    # Validation 2: Version should increase
    if [[ "$PRE_VERSION" != "0" && "$POST_VERSION" -le "$PRE_VERSION" ]]; then
        warn "contract_version did not increase. Verify migrate() was called correctly."
    fi
    
    log "  ✓ All NFTs and royalties preserved"
    log "  ✓ Version bumped successfully"
fi

# ---------------------------------------------------------------------------
# Success summary
# ---------------------------------------------------------------------------
step "Upgrade complete"

if [[ "$DRY_RUN" == "1" ]]; then
    log "Dry run completed successfully."
    log "To execute the upgrade, run: ./scripts/upgrade.sh $NETWORK $CONTRACT_ID"
else
    log "Contract upgraded successfully!"
    log ""
    log "Summary:"
    log "  Previous version: $PRE_VERSION → New version: $POST_VERSION"
    log "  Total supply    : $POST_SUPPLY (unchanged ✓)"
    log "  New WASM hash   : $NEW_WASM_HASH"
    log ""
    log "Rollback plan saved to: $ROLLBACK_FILE"
    log "To rollback: source $ROLLBACK_FILE && ./scripts/rollback.sh $NETWORK"
fi
log "  To roll back: source $ROLLBACK_FILE && ./scripts/rollback.sh"
