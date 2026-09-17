# fundkeep-contract

The Soroban smart contract behind [FundKeep](https://github.com/Michealshodipo56/fundkeep-app) — a non-custodial savings-goal contract on Stellar. One deployment handles every user and every goal; goals are differentiated by an auto-incrementing `goal_id`.

A goal locks a target amount of a token (USDC on testnet) until either the target is reached or a deadline passes. There is no early withdrawal and no admin override — enforcement is entirely on-chain. See [`fundkeep-app/docs/contract`](https://github.com/Michealshodipo56/fundkeep-app/tree/main/docs/contract) for the full spec this implementation follows.

## Repo Layout

```
contracts/fundkeep/
  src/
    lib.rs      # contract entry points: create_goal, deposit, check_deadline, withdraw, get_goal
    types.rs    # SavingsGoal, DataKey
    errors.rs   # contract error codes
    events.rs   # contractevent definitions (goal_created, deposit, unlock, withdraw)
    test.rs     # unit test suite
scripts/deploy.sh
```

## Requirements

- Rust (stable), with the `wasm32v1-none` target: `rustup target add wasm32v1-none`
- [stellar-cli](https://github.com/stellar/stellar-cli) for deployment

## Build & Test

```bash
cargo test
cargo build --target wasm32v1-none --release
```

## Deploy to Testnet

```bash
stellar keys generate deployer --network testnet --fund
./scripts/deploy.sh deployer
```

The script builds, deploys, and prints the resulting contract ID and Testnet RPC settings. Configure a verified token SAC separately before setting `NEXT_PUBLIC_USDC_CONTRACT_ID` in `fundkeep-app`.

## Contract Interface

| Function | Auth | Description |
|---|---|---|
| `create_goal(owner, token, target_amount, deadline) -> u32` | `owner` | Creates a goal, returns its ID |
| `deposit(caller, goal_id, amount)` | `caller` (must be owner) | Adds funds; auto-unlocks if target reached |
| `check_deadline(goal_id)` | none | Unlocks the goal if its deadline has passed |
| `withdraw(caller, goal_id)` | `caller` (must be owner) | Pays out the full balance once unlocked |
| `get_goal(goal_id) -> SavingsGoal` | none | Reads a goal's state |

Errors: `GoalNotFound`, `NotUnlocked`, `AlreadyWithdrawn`, `Unauthorized`, `InvalidAmount`, `InvalidDeadline`, `ArithmeticOverflow`.

## Instance-Storage TTL & Renewal Strategy

### How TTL Affects Stored Goals
Soroban requires contracts and ledger entries to have an active Time-To-Live (TTL) counter measured in ledgers. In FundKeep, savings goals and the global goal counter are stored directly in **Instance Storage**:
- If instance storage expires without renewal, the contract instance and all associated goals become archived/inactive on the ledger.
- While instance entries can be restored by submitting a Soroban state restoration transaction, goals cannot be interacted with (no deposits, deadline checks, or withdrawals) while expired.

### Current Bump Behavior
The contract uses automatic in-transaction TTL extension via `bump_instance()` on state-mutating operations (`create_goal`, `deposit`, `check_deadline` unlock, and `withdraw`):
- **Constants**:
  - `DAY_IN_LEDGERS = 17,280` ledgers (~24 hours at 5 seconds per ledger).
  - `INSTANCE_BUMP_AMOUNT = 30 * DAY_IN_LEDGERS` (~30 days).
  - `INSTANCE_LIFETIME_THRESHOLD = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS` (~29 days).
- When a user interacts with the contract, `env.storage().instance().extend_ttl(...)` verifies the current TTL. If remaining lifetime is below the threshold, it extends the contract instance lifetime to 30 days out.

### Operational Guidance for Maintainers & Keepers
Because contracts with dormant goals (e.g., long-term savings goals with deadlines months away) might experience periods without user transactions:
1. **Automated Keeper / Cron**: Maintainers should run a periodic keeper service (or cron job) that calls a read/bump transaction or directly submits a ledger footprint TTL extension via `stellar-cli` or Soroban RPC `extendFootprintTtl`:
   ```bash
   stellar contract extend-ttl \
     --id <CONTRACT_ID> \
     --network testnet \
     --ledgers 518400
   ```
2. **Monitoring**: Maintainers and indexers should observe ledger entry expiration timestamps and alert when the remaining lifetime falls below 7 days.
3. **State Restoration**: If instance storage ever enters an archived state, run:
   ```bash
   stellar contract restore \
     --id <CONTRACT_ID> \
     --network testnet
   ```

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Security issues: see [SECURITY.md](SECURITY.md).
