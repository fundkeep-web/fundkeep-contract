#![no_std]

mod errors;
mod events;
mod types;

#[cfg(test)]
mod test;

use soroban_sdk::{contract, contractimpl, token, Address, Env, Symbol};

pub use errors::Error;
pub use types::{DataKey, SavingsGoal};

// TTL bump constants, matching the standard soroban-examples pattern:
// bump 30 days out once the instance's remaining lifetime drops under 1 day.
const DAY_IN_LEDGERS: u32 = 17280;
const INSTANCE_BUMP_AMOUNT: u32 = 30 * DAY_IN_LEDGERS;
const INSTANCE_LIFETIME_THRESHOLD: u32 = INSTANCE_BUMP_AMOUNT - DAY_IN_LEDGERS;

fn bump_instance(env: &Env) {
    env.storage()
        .instance()
        .extend_ttl(INSTANCE_LIFETIME_THRESHOLD, INSTANCE_BUMP_AMOUNT);
}

fn load_goal(env: &Env, goal_id: u32) -> Result<SavingsGoal, Error> {
    env.storage()
        .instance()
        .get(&DataKey::Goal(goal_id))
        .ok_or(Error::GoalNotFound)
}

fn save_goal(env: &Env, goal_id: u32, goal: &SavingsGoal) {
    env.storage().instance().set(&DataKey::Goal(goal_id), goal);
    bump_instance(env);
}

fn load_contribution(env: &Env, goal_id: u32, depositor: &Address) -> i128 {
    env.storage()
        .instance()
        .get(&DataKey::GoalContribution(goal_id, depositor.clone()))
        .unwrap_or(0)
}

fn save_contribution(env: &Env, goal_id: u32, depositor: &Address, amount: i128) {
    env.storage()
        .instance()
        .set(&DataKey::GoalContribution(goal_id, depositor.clone()), &amount);
    bump_instance(env);
}

#[contract]
pub struct FundKeepContract;

#[contractimpl]
impl FundKeepContract {
    /// Creates a new savings goal owned by `owner`, saving `token` toward
    /// `target_amount` until `deadline`. Returns the new goal's ID.
    pub fn create_goal(
        env: Env,
        owner: Address,
        token: Address,
        target_amount: i128,
        deadline: u64,
    ) -> Result<u32, Error> {
        Self::create_goal_internal(env, owner, token, target_amount, deadline, false)
    }

    /// Creates a new group savings goal where any address can contribute toward
    /// `target_amount` until `deadline`. Returns the new goal's ID.
    pub fn create_group_goal(
        env: Env,
        owner: Address,
        token: Address,
        target_amount: i128,
        deadline: u64,
    ) -> Result<u32, Error> {
        Self::create_goal_internal(env, owner, token, target_amount, deadline, true)
    }

    fn create_goal_internal(
        env: Env,
        owner: Address,
        token: Address,
        target_amount: i128,
        deadline: u64,
        is_group: bool,
    ) -> Result<u32, Error> {
        owner.require_auth();

        if target_amount <= 0 {
            return Err(Error::InvalidAmount);
        }
        if deadline <= env.ledger().timestamp() {
            return Err(Error::InvalidDeadline);
        }

        let goal_id: u32 = env
            .storage()
            .instance()
            .get(&DataKey::GoalCounter)
            .unwrap_or(0);

        let goal = SavingsGoal {
            owner: owner.clone(),
            token: token.clone(),
            target_amount,
            current_amount: 0,
            deadline,
            unlocked: false,
            withdrawn: false,
            is_group,
        };

        save_goal(&env, goal_id, &goal);
        env.storage()
            .instance()
            .set(&DataKey::GoalCounter, &(goal_id + 1));
        bump_instance(&env);

        events::GoalCreated {
            goal_id,
            owner,
            token,
            target_amount,
            deadline,
        }
        .publish(&env);

        Ok(goal_id)
    }

    /// Transfers `amount` of the goal's token from `caller` to the contract
    /// and adds it to the goal's `current_amount`.
    /// For single-owner goals, `caller` must be the goal's owner.
    /// For group goals, any address may deposit.
    /// Auto-unlocks the goal in the same call if the target is reached.
    pub fn deposit(env: Env, caller: Address, goal_id: u32, amount: i128) -> Result<(), Error> {
        caller.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let mut goal = load_goal(&env, goal_id)?;

        if goal.withdrawn {
            return Err(Error::AlreadyWithdrawn);
        }
        if !goal.is_group && caller != goal.owner {
            return Err(Error::Unauthorized);
        }

        token::TokenClient::new(&env, &goal.token).transfer(
            &caller,
            env.current_contract_address(),
            &amount,
        );

        goal.current_amount = goal
            .current_amount
            .checked_add(amount)
            .ok_or(Error::InvalidAmount)?;

        if goal.current_amount >= goal.target_amount {
            goal.unlocked = true;
        }

        // Track per-depositor contribution
        let prev_contrib = load_contribution(&env, goal_id, &caller);
        let new_contrib = prev_contrib
            .checked_add(amount)
            .ok_or(Error::InvalidAmount)?;
        save_contribution(&env, goal_id, &caller, new_contrib);

        save_goal(&env, goal_id, &goal);

        events::Deposit {
            goal_id,
            caller,
            amount,
            current_amount: goal.current_amount,
            unlocked: goal.unlocked,
        }
        .publish(&env);

        Ok(())
    }

    /// Publicly callable. If the goal's deadline has passed and it isn't
    /// already unlocked, marks it unlocked. No-op if the deadline hasn't
    /// passed yet.
    pub fn check_deadline(env: Env, goal_id: u32) -> Result<(), Error> {
        let mut goal = load_goal(&env, goal_id)?;

        if goal.withdrawn {
            return Err(Error::AlreadyWithdrawn);
        }

        if !goal.unlocked && env.ledger().timestamp() >= goal.deadline {
            goal.unlocked = true;
            save_goal(&env, goal_id, &goal);
            events::Unlock {
                goal_id,
                via: Symbol::new(&env, "deadline"),
            }
            .publish(&env);
        }

        Ok(())
    }

    /// Transfers the goal's funds back to `caller`. Only permitted once the goal is unlocked.
    /// For single-owner goals, `caller` must be the owner and withdraws the full amount.
    /// For group goals, `caller` receives their contributed share. A depositor who contributed 0 gets 0 (Unauthorized).
    pub fn withdraw(env: Env, caller: Address, goal_id: u32) -> Result<(), Error> {
        caller.require_auth();

        let mut goal = load_goal(&env, goal_id)?;

        if goal.withdrawn {
            return Err(Error::AlreadyWithdrawn);
        }
        if !goal.unlocked {
            return Err(Error::NotUnlocked);
        }

        let amount = if goal.is_group {
            let contrib = load_contribution(&env, goal_id, &caller);
            if contrib <= 0 {
                return Err(Error::Unauthorized);
            }
            save_contribution(&env, goal_id, &caller, 0);
            contrib
        } else {
            if caller != goal.owner {
                return Err(Error::Unauthorized);
            }
            goal.current_amount
        };

        token::TokenClient::new(&env, &goal.token).transfer(
            &env.current_contract_address(),
            &caller,
            &amount,
        );

        goal.current_amount = goal
            .current_amount
            .checked_sub(amount)
            .ok_or(Error::InvalidAmount)?;

        if goal.current_amount == 0 {
            goal.withdrawn = true;
        }

        save_goal(&env, goal_id, &goal);

        events::Withdraw {
            goal_id,
            owner: caller,
            amount,
        }
        .publish(&env);

        Ok(())
    }

    /// Publicly readable. Returns the contribution amount for a specific depositor in a goal.
    pub fn get_contribution(env: Env, goal_id: u32, depositor: Address) -> i128 {
        load_contribution(&env, goal_id, &depositor)
    }

    /// Publicly readable. Returns the full on-chain state of a goal.
    pub fn get_goal(env: Env, goal_id: u32) -> Result<SavingsGoal, Error> {
        load_goal(&env, goal_id)
    }
}
