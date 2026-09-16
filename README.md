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

Errors: `GoalNotFound`, `NotUnlocked`, `AlreadyWithdrawn`, `Unauthorized`, `InvalidAmount`, `InvalidDeadline`.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md). Security issues: see [SECURITY.md](SECURITY.md).
