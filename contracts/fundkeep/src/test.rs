use crate::{Error, FundKeepContract, FundKeepContractClient};
use soroban_sdk::{
    testutils::{Address as _, Ledger},
    token, Address, Env,
};

const DAY: u64 = 24 * 60 * 60;
const USDC_DECIMALS: i128 = 10_000_000; // 7 decimals

struct TestCtx<'a> {
    env: Env,
    client: FundKeepContractClient<'a>,
    token: token::TokenClient<'a>,
    token_admin: token::StellarAssetClient<'a>,
    token_address: Address,
}

fn setup<'a>() -> TestCtx<'a> {
    let env = Env::default();
    env.mock_all_auths();
    env.ledger().set_timestamp(1_700_000_000);

    let contract_id = env.register(FundKeepContract, ());
    let client = FundKeepContractClient::new(&env, &contract_id);

    let token_admin_address = Address::generate(&env);
    let sac = env.register_stellar_asset_contract_v2(token_admin_address);
    let token_address = sac.address();
    let token = token::TokenClient::new(&env, &token_address);
    let token_admin = token::StellarAssetClient::new(&env, &token_address);

    TestCtx {
        env,
        client,
        token,
        token_admin,
        token_address,
    }
}

fn future_deadline(ctx: &TestCtx, secs_from_now: u64) -> u64 {
    ctx.env.ledger().timestamp() + secs_from_now
}

// ── create_goal ──────────────────────────────────────────────────────────

#[test]
fn create_goal_with_valid_params_returns_zero() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let deadline = future_deadline(&ctx, 30 * DAY);

    let goal_id = ctx.client.create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &deadline,
    );

    assert_eq!(goal_id, 0);

    let goal = ctx.client.get_goal(&goal_id);
    assert_eq!(goal.owner, owner);
    assert_eq!(goal.token, ctx.token_address);
    assert_eq!(goal.target_amount, 100 * USDC_DECIMALS);
    assert_eq!(goal.current_amount, 0);
    assert!(!goal.unlocked);
    assert!(!goal.withdrawn);

    // Second goal gets the next sequential ID.
    let second_id =
        ctx.client
            .create_goal(&owner, &ctx.token_address, &(50 * USDC_DECIMALS), &deadline);
    assert_eq!(second_id, 1);
}

#[test]
fn create_goal_with_zero_target_fails() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let deadline = future_deadline(&ctx, 30 * DAY);

    let result = ctx
        .client
        .try_create_goal(&owner, &ctx.token_address, &0, &deadline);

    assert_eq!(result, Err(Ok(Error::InvalidAmount)));
}

#[test]
fn create_goal_with_past_deadline_fails() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let past_deadline = ctx.env.ledger().timestamp() - 1;

    let result = ctx.client.try_create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &past_deadline,
    );

    assert_eq!(result, Err(Ok(Error::InvalidDeadline)));
}

// ── deposit ──────────────────────────────────────────────────────────────

#[test]
fn deposit_under_target_keeps_goal_locked() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let deadline = future_deadline(&ctx, 30 * DAY);
    ctx.token_admin.mint(&owner, &(1_000 * USDC_DECIMALS));

    let goal_id = ctx.client.create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &deadline,
    );

    ctx.client.deposit(&owner, &goal_id, &(40 * USDC_DECIMALS));

    let goal = ctx.client.get_goal(&goal_id);
    assert_eq!(goal.current_amount, 40 * USDC_DECIMALS);
    assert!(!goal.unlocked);
    assert_eq!(ctx.token.balance(&owner), 960 * USDC_DECIMALS);
}

#[test]
fn deposit_crossing_target_unlocks_in_same_call() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let deadline = future_deadline(&ctx, 30 * DAY);
    ctx.token_admin.mint(&owner, &(1_000 * USDC_DECIMALS));

    let goal_id = ctx.client.create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &deadline,
    );

    ctx.client.deposit(&owner, &goal_id, &(60 * USDC_DECIMALS));
    ctx.client.deposit(&owner, &goal_id, &(60 * USDC_DECIMALS));

    let goal = ctx.client.get_goal(&goal_id);
    assert_eq!(goal.current_amount, 120 * USDC_DECIMALS);
    assert!(goal.unlocked);
}

#[test]
fn deposit_from_non_owner_fails() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let stranger = Address::generate(&ctx.env);
    let deadline = future_deadline(&ctx, 30 * DAY);
    ctx.token_admin.mint(&stranger, &(1_000 * USDC_DECIMALS));

    let goal_id = ctx.client.create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &deadline,
    );

    let result = ctx
        .client
        .try_deposit(&stranger, &goal_id, &(10 * USDC_DECIMALS));

    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

#[test]
fn deposit_after_withdrawn_fails() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let deadline = future_deadline(&ctx, 30 * DAY);
    ctx.token_admin.mint(&owner, &(1_000 * USDC_DECIMALS));

    let goal_id = ctx.client.create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &deadline,
    );
    ctx.client.deposit(&owner, &goal_id, &(100 * USDC_DECIMALS));
    ctx.client.withdraw(&owner, &goal_id);

    let result = ctx
        .client
        .try_deposit(&owner, &goal_id, &(10 * USDC_DECIMALS));

    assert_eq!(result, Err(Ok(Error::AlreadyWithdrawn)));
}

// ── withdraw ─────────────────────────────────────────────────────────────

#[test]
fn withdraw_while_locked_fails() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let deadline = future_deadline(&ctx, 30 * DAY);
    ctx.token_admin.mint(&owner, &(1_000 * USDC_DECIMALS));

    let goal_id = ctx.client.create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &deadline,
    );
    ctx.client.deposit(&owner, &goal_id, &(10 * USDC_DECIMALS));

    let result = ctx.client.try_withdraw(&owner, &goal_id);

    assert_eq!(result, Err(Ok(Error::NotUnlocked)));
}

#[test]
fn withdraw_after_target_unlock_succeeds() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let deadline = future_deadline(&ctx, 30 * DAY);
    ctx.token_admin.mint(&owner, &(1_000 * USDC_DECIMALS));

    let goal_id = ctx.client.create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &deadline,
    );
    ctx.client.deposit(&owner, &goal_id, &(100 * USDC_DECIMALS));

    ctx.client.withdraw(&owner, &goal_id);

    let goal = ctx.client.get_goal(&goal_id);
    assert!(goal.withdrawn);
    assert_eq!(goal.current_amount, 0);
    assert_eq!(ctx.token.balance(&owner), 1_000 * USDC_DECIMALS);
}

#[test]
fn second_withdraw_on_same_goal_fails() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let deadline = future_deadline(&ctx, 30 * DAY);
    ctx.token_admin.mint(&owner, &(1_000 * USDC_DECIMALS));

    let goal_id = ctx.client.create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &deadline,
    );
    ctx.client.deposit(&owner, &goal_id, &(100 * USDC_DECIMALS));
    ctx.client.withdraw(&owner, &goal_id);

    let result = ctx.client.try_withdraw(&owner, &goal_id);

    assert_eq!(result, Err(Ok(Error::AlreadyWithdrawn)));
}

#[test]
fn withdraw_from_non_owner_fails() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let stranger = Address::generate(&ctx.env);
    let deadline = future_deadline(&ctx, 30 * DAY);
    ctx.token_admin.mint(&owner, &(1_000 * USDC_DECIMALS));

    let goal_id = ctx.client.create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &deadline,
    );
    ctx.client.deposit(&owner, &goal_id, &(100 * USDC_DECIMALS));

    let result = ctx.client.try_withdraw(&stranger, &goal_id);

    assert_eq!(result, Err(Ok(Error::Unauthorized)));
}

// ── check_deadline ───────────────────────────────────────────────────────

#[test]
fn check_deadline_before_deadline_is_noop() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let deadline = future_deadline(&ctx, 30 * DAY);

    let goal_id = ctx.client.create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &deadline,
    );

    ctx.client.check_deadline(&goal_id);

    let goal = ctx.client.get_goal(&goal_id);
    assert!(!goal.unlocked);
}

#[test]
fn check_deadline_after_deadline_unlocks() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let deadline = future_deadline(&ctx, DAY);

    let goal_id = ctx.client.create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &deadline,
    );

    ctx.env.ledger().set_timestamp(deadline + 1);
    ctx.client.check_deadline(&goal_id);

    let goal = ctx.client.get_goal(&goal_id);
    assert!(goal.unlocked);
}

#[test]
fn withdraw_after_deadline_unlock_succeeds() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let deadline = future_deadline(&ctx, DAY);
    ctx.token_admin.mint(&owner, &(1_000 * USDC_DECIMALS));

    let goal_id = ctx.client.create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &deadline,
    );
    ctx.client.deposit(&owner, &goal_id, &(30 * USDC_DECIMALS));

    ctx.env.ledger().set_timestamp(deadline + 1);
    ctx.client.check_deadline(&goal_id);
    ctx.client.withdraw(&owner, &goal_id);

    let goal = ctx.client.get_goal(&goal_id);
    assert!(goal.withdrawn);
    // Deposited 30, target was never hit, but a passed deadline unlocks
    // whatever was saved — the full 1,000 starting balance comes back.
    assert_eq!(ctx.token.balance(&owner), 1_000 * USDC_DECIMALS);
}

// ── get_goal ─────────────────────────────────────────────────────────────

#[test]
fn get_goal_for_missing_id_fails() {
    let ctx = setup();

    let result = ctx.client.try_get_goal(&999);

    assert_eq!(result, Err(Ok(Error::GoalNotFound)));
}

// ── boundary tests for check_deadline ─────────────────────────────────────

#[test]
fn check_deadline_exactly_one_second_before_is_noop() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let deadline = future_deadline(&ctx, 100);

    let goal_id = ctx.client.create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &deadline,
    );

    // Exactly 1 second before deadline: timestamp == deadline - 1
    ctx.env.ledger().set_timestamp(deadline - 1);
    ctx.client.check_deadline(&goal_id);

    let goal = ctx.client.get_goal(&goal_id);
    assert!(!goal.unlocked, "Goal must remain locked when timestamp < deadline");
}

#[test]
fn check_deadline_exact_timestamp_boundary_unlocks() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let deadline = future_deadline(&ctx, 100);

    let goal_id = ctx.client.create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &deadline,
    );

    // Exactly at the boundary: timestamp == deadline
    ctx.env.ledger().set_timestamp(deadline);
    ctx.client.check_deadline(&goal_id);

    let goal = ctx.client.get_goal(&goal_id);
    assert!(goal.unlocked, "Goal must unlock at timestamp == deadline");
}

#[test]
fn check_deadline_after_already_unlocked_is_idempotent() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let deadline = future_deadline(&ctx, 100);

    let goal_id = ctx.client.create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &deadline,
    );

    // Unlock at boundary
    ctx.env.ledger().set_timestamp(deadline);
    ctx.client.check_deadline(&goal_id);
    assert!(ctx.client.get_goal(&goal_id).unlocked);

    // Second call well past deadline
    ctx.env.ledger().set_timestamp(deadline + 500);
    ctx.client.check_deadline(&goal_id);

    let goal_after = ctx.client.get_goal(&goal_id);
    assert!(goal_after.unlocked);
    assert!(!goal_after.withdrawn);
}

