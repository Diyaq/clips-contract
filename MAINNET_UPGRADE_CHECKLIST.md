# ClipsNFT Mainnet Upgrade Checklist

This checklist ensures every mainnet upgrade is safe, documented, and reversible.

Use this before every production upgrade. Keep a completed copy for audit trail.

---

## Pre-Upgrade Phase (T-7 Days)

### Code Review & Testing

- [ ] Feature/bugfix branch reviewed by 2+ team members
- [ ] All tests pass: `cargo test`
- [ ] No compiler warnings
- [ ] Security audit completed (if critical changes)
- [ ] Migration tests pass: `cargo test --test upgrade_migration`
- [ ] Integration tests pass: `cargo test --test integration`
- [ ] Load tests completed (if applicable)

### Documentation

- [ ] UPGRADE.md reviewed and updated
- [ ] Changelog updated with new features/fixes
- [ ] Migration guide written (if data transformation needed)
- [ ] Known issues documented
- [ ] Rollback plan documented

### Team Coordination

- [ ] Upgrade date/time scheduled
- [ ] Team availability confirmed (dev, ops, support)
- [ ] Communication plan created
- [ ] Stakeholders notified
- [ ] Customer communication drafted (if needed)
- [ ] Escalation contacts confirmed

### Infrastructure Check

- [ ] Soroban RPC monitored and healthy
- [ ] Network bandwidth adequate
- [ ] Admin account has sufficient XLM (minimum 5 XLM)
- [ ] Backup account with admin permissions ready
- [ ] Monitoring/alerting configured

---

## Testnet Phase (T-3 Days)

### Environment Setup

- [ ] Testnet contract deployed
- [ ] Recent testnet data mirrors mainnet (if possible)
- [ ] Admin account configured for testnet
- [ ] All scripts tested on testnet

### Pre-Upgrade Validation

- [ ] Record testnet state:
  - contract_version: ___
  - total_supply: ___
  - number of NFTs: ___
  - sample royalties intact: ☐

### Upgrade Execution

- [ ] Dry-run successful: `DRY_RUN=1 ./scripts/upgrade.sh testnet`
- [ ] Actual upgrade executed: `./scripts/upgrade.sh testnet`
- [ ] No errors in output
- [ ] Post-flight validation passed
- [ ] All 3 post-upgrade validations passed:
  - [ ] contract_version increased to expected value
  - [ ] total_supply unchanged
  - [ ] sample NFTs still retrievable

### Post-Upgrade Testing

- [ ] Mint new NFTs: works correctly
- [ ] Transfer NFTs: works correctly
- [ ] Query royalties: works correctly
- [ ] Approve/allowance: works correctly
- [ ] All new features working
- [ ] No performance regression
- [ ] No unusual gas consumption

### Monitoring

- [ ] Monitor for 24 hours
- [ ] Check contract events for anomalies
- [ ] Monitor network activity
- [ ] Verify no user complaints
- [ ] Run smoke tests every 6 hours

### Sign-Off

- [ ] QA lead: _________________________ Date: _____
- [ ] DevOps lead: _________________________ Date: _____
- [ ] Tech lead: _________________________ Date: _____

---

## Pre-Mainnet Phase (T-1 Day)

### Final Checks

- [ ] Build release WASM for mainnet:
  ```bash
  cargo build --target wasm32-unknown-unknown --release -p clips_nft
  ```
- [ ] Verify WASM binary created
- [ ] Mainnet contract ID verified: _______________
- [ ] Mainnet admin account verified
- [ ] Mainnet RPC health verified
- [ ] Network conditions normal (no congestion)

### Last-Minute Reviews

- [ ] Tech lead: Final code review ☐
- [ ] DevOps lead: Final script review ☐
- [ ] Product: Final feature verification ☐
- [ ] Security: Any final concerns? ☐

### Communication

- [ ] Send customer notification (if applicable)
- [ ] Status page updated (maintenance window noted)
- [ ] Team Slack notification sent
- [ ] Escalation numbers verified with on-call

### Prepare Rollback

- [ ] Previous WASM hash documented: ___________
- [ ] Previous version documented: ___
- [ ] Previous supply documented: ___
- [ ] Rollback script tested on testnet
- [ ] Rollback procedure reviewed by ops team

---

## Mainnet Upgrade Phase (Day of Upgrade)

### Pre-Upgrade (Start of Window)

**Time Started: ___:___ UTC**

- [ ] Lock mainnet (pause contract if applicable)
- [ ] Announce maintenance window to users
- [ ] Team assembled and ready
- [ ] Escalation contacts on alert status
- [ ] Recording terminal output for audit

### Upgrade Steps (Estimated 2-5 min)

**Step 1: Pre-flight snapshot**
- [ ] Version: ___
- [ ] Supply: ___
- [ ] Admin: ___

**Step 2: Build & Install**
- [ ] Build successful
- [ ] WASM installed: Hash ___________

**Step 3: Upgrade**
- [ ] upgrade() called successfully
- [ ] Code swapped

**Step 4: Migrate**
- [ ] migrate() called successfully
- [ ] Version bumped

**Step 5: Verification**
- [ ] Post-version: ___
- [ ] Post-supply: ___
- [ ] Supply preserved: ☐
- [ ] Version increased: ☐

### Immediate Post-Upgrade (First 10 Minutes)

- [ ] No unusual error rates
- [ ] Contract responsive
- [ ] RPC responding normally
- [ ] Alert systems not triggered
- [ ] Tech lead gives approval to proceed

**Upgrade Status**: ☐ SUCCESS / ☐ ROLLBACK INITIATED

If rollback needed, execute immediately:
```bash
source .soroban/rollback-mainnet.env
./scripts/rollback.sh mainnet
```

### User-Facing Validation (First Hour)

- [ ] Sample NFT transfers working
- [ ] Royalty payments working
- [ ] Wallet integrations working
- [ ] No user-reported issues
- [ ] API endpoints responding

### Operations & Monitoring (First 24 Hours)

Assign monitoring shifts:
- Hour 0-4: ________________________
- Hour 4-8: ________________________
- Hour 8-12: ________________________
- Hour 12-24: ________________________

Monitoring checklist (every hour):
- [ ] Contract accessible
- [ ] Normal transaction rates
- [ ] No unusual error logs
- [ ] RPC health good
- [ ] Alerts not triggered
- [ ] Customer support queue normal

---

## Post-Upgrade Phase (Days 1-7)

### Daily Checklist

**Day 1:**
- [ ] No critical issues
- [ ] No customer complaints
- [ ] Performance normal
- [ ] All systems stable
- [ ] Rollback artefact backed up

**Day 2-3:**
- [ ] Continued stability
- [ ] Integration tests passing
- [ ] Load test results good
- [ ] No data anomalies
- [ ] Documentation updated

**Day 4-7:**
- [ ] All metrics normal
- [ ] No performance regression
- [ ] Feature adoption good
- [ ] Ready to archive rollback artefact
- [ ] Post-mortem (if any issues) completed

### Success Criteria

All of these must be true:
- [ ] Upgrade completed without errors
- [ ] No production incidents attributed to upgrade
- [ ] All NFTs preserved and accessible
- [ ] All royalties intact and functioning
- [ ] Contract version correct
- [ ] No data loss
- [ ] No performance degradation
- [ ] Users report no issues

### Sign-Off

- [ ] Product Manager: _________________________ Date: _____
- [ ] Ops Team Lead: _________________________ Date: _____
- [ ] Tech Lead: _________________________ Date: _____

---

## Artifact Retention

After upgrade completes, retain these files:

| File | Duration | Location | Purpose |
|------|----------|----------|---------|
| Upgrade transcript | 30 days | /archive/upgrades/ | Audit trail |
| WASM binary | 30 days | /archive/builds/ | Reproducibility |
| rollback-mainnet.env | 7 days | .soroban/ | Emergency recovery |
| Migration details | Forever | /docs/migrations/ | Historical record |

---

## Post-Mortem (if issues occurred)

If any issues occurred during/after upgrade:

- [ ] Incident report filed
- [ ] Root cause analysis completed
- [ ] Timeline documented
- [ ] Fixes implemented
- [ ] Preventive measures identified
- [ ] Team debriefing completed
- [ ] Lessons learned documented
- [ ] Process improvements made

---

## Notes & Observations

**Upgrade executed by**: ___________________________
**Start time**: ___:___ UTC
**End time**: ___:___ UTC
**Total duration**: ____ minutes

**Issues encountered**:
_________________________________________________________________
_________________________________________________________________

**Lessons learned**:
_________________________________________________________________
_________________________________________________________________

**Improvements for next upgrade**:
_________________________________________________________________
_________________________________________________________________

**Additional notes**:
_________________________________________________________________
_________________________________________________________________

---

## Appendix: Emergency Contacts

| Role | Name | Phone | Slack |
|------|------|-------|-------|
| Tech Lead | | | |
| DevOps Lead | | | |
| On-Call Eng | | | |
| Prod Manager | | | |
| Support Lead | | | |

## Appendix: Useful Links

- **Stellar Expert**: https://stellar.expert/explorer/mainnet
- **RPC Status**: https://status.stellar.org/
- **Docs**: https://developers.stellar.org/docs/smart-contracts
- **This Repo**: https://github.com/clipcash/clips-contract

---

**Checklist Version**: 1.0
**Last Updated**: May 28, 2026
**Next Review Date**: ___________

**PRINT THIS CHECKLIST AND KEEP A COPY FOR EVERY MAINNET UPGRADE**

---

## Sign-Off

Project Manager: _________________________ Date: _____

Authorized to proceed with mainnet upgrade: ☐ YES / ☐ NO

Comments:
_________________________________________________________________
_________________________________________________________________
