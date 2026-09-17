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

// ── group savings goals (Issue #2) ────────────────────────────────────────

#[test]
fn group_goal_multiple_depositors_and_proportional_withdraw() {
    let ctx = setup();
    let creator = Address::generate(&ctx.env);
    let user_a = Address::generate(&ctx.env);
    let user_b = Address::generate(&ctx.env);
    let stranger = Address::generate(&ctx.env);

    ctx.token_admin.mint(&user_a, &(100 * USDC_DECIMALS));
    ctx.token_admin.mint(&user_b, &(100 * USDC_DECIMALS));

    let deadline = future_deadline(&ctx, 30 * DAY);
    let target = 80 * USDC_DECIMALS;

    let goal_id = ctx.client.create_group_goal(
        &creator,
        &ctx.token_address,
        &target,
        &deadline,
    );

    let goal = ctx.client.get_goal(&goal_id);
    assert!(goal.is_group);
    assert!(!goal.unlocked);

    // user_a deposits 50 USDC
    ctx.client.deposit(&user_a, &goal_id, &(50 * USDC_DECIMALS));
    assert_eq!(ctx.client.get_contribution(&goal_id, &user_a), 50 * USDC_DECIMALS);
    assert_eq!(ctx.client.get_contribution(&goal_id, &user_b), 0);

    let goal = ctx.client.get_goal(&goal_id);
    assert_eq!(goal.current_amount, 50 * USDC_DECIMALS);
    assert!(!goal.unlocked);

    // user_b deposits 30 USDC, reaching target (80 USDC) -> unlocks
    ctx.client.deposit(&user_b, &goal_id, &(30 * USDC_DECIMALS));
    assert_eq!(ctx.client.get_contribution(&goal_id, &user_b), 30 * USDC_DECIMALS);

    let goal = ctx.client.get_goal(&goal_id);
    assert_eq!(goal.current_amount, 80 * USDC_DECIMALS);
    assert!(goal.unlocked);

    // stranger (0 contribution) attempts to withdraw -> Unauthorized
    let stranger_res = ctx.client.try_withdraw(&stranger, &goal_id);
    assert_eq!(stranger_res, Err(Ok(Error::Unauthorized)));

    // user_a withdraws their 50 USDC share
    ctx.client.withdraw(&user_a, &goal_id);
    assert_eq!(ctx.token.balance(&user_a), 100 * USDC_DECIMALS);
    assert_eq!(ctx.client.get_contribution(&goal_id, &user_a), 0);

    let goal = ctx.client.get_goal(&goal_id);
    assert_eq!(goal.current_amount, 30 * USDC_DECIMALS);
    assert!(!goal.withdrawn);

    // user_a attempts second withdraw -> Unauthorized (contribution is now 0)
    let double_withdraw = ctx.client.try_withdraw(&user_a, &goal_id);
    assert_eq!(double_withdraw, Err(Ok(Error::Unauthorized)));

    // user_b withdraws their 30 USDC share
    ctx.client.withdraw(&user_b, &goal_id);
    assert_eq!(ctx.token.balance(&user_b), 100 * USDC_DECIMALS);
    assert_eq!(ctx.client.get_contribution(&goal_id, &user_b), 0);

    let goal = ctx.client.get_goal(&goal_id);
    assert_eq!(goal.current_amount, 0);
    assert!(goal.withdrawn);
}

#[test]
fn single_owner_goal_is_not_group_and_rejects_third_party() {
    let ctx = setup();
    let owner = Address::generate(&ctx.env);
    let other = Address::generate(&ctx.env);
    ctx.token_admin.mint(&other, &(100 * USDC_DECIMALS));

    let deadline = future_deadline(&ctx, 30 * DAY);
    let goal_id = ctx.client.create_goal(
        &owner,
        &ctx.token_address,
        &(100 * USDC_DECIMALS),
        &deadline,
    );

    let goal = ctx.client.get_goal(&goal_id);
    assert!(!goal.is_group);

    let res = ctx.client.try_deposit(&other, &goal_id, &(10 * USDC_DECIMALS));
    assert_eq!(res, Err(Ok(Error::Unauthorized)));
}
