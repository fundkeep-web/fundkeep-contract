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
    /// and adds it to the goal's `current_amount`. `caller` must be the
    /// goal's owner. Auto-unlocks the goal in the same call if the target is
    /// reached.
    pub fn deposit(env: Env, caller: Address, goal_id: u32, amount: i128) -> Result<(), Error> {
        caller.require_auth();

        if amount <= 0 {
            return Err(Error::InvalidAmount);
        }

        let mut goal = load_goal(&env, goal_id)?;

        if goal.withdrawn {
            return Err(Error::AlreadyWithdrawn);
        }
        if caller != goal.owner {
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

    /// Transfers the goal's full `current_amount` back to `caller`, who must
    /// be the goal's owner. Only permitted once the goal is unlocked.
    pub fn withdraw(env: Env, caller: Address, goal_id: u32) -> Result<(), Error> {
        caller.require_auth();

        let mut goal = load_goal(&env, goal_id)?;

        if caller != goal.owner {
            return Err(Error::Unauthorized);
        }
        if goal.withdrawn {
            return Err(Error::AlreadyWithdrawn);
        }
        if !goal.unlocked {
            return Err(Error::NotUnlocked);
        }

        let amount = goal.current_amount;
        token::TokenClient::new(&env, &goal.token).transfer(
            &env.current_contract_address(),
            &caller,
            &amount,
        );

        goal.current_amount = 0;
        goal.withdrawn = true;

        save_goal(&env, goal_id, &goal);

        events::Withdraw {
            goal_id,
            owner: caller,
            amount,
        }
        .publish(&env);

        Ok(())
    }

    /// Publicly readable. Returns the full on-chain state of a goal.
    pub fn get_goal(env: Env, goal_id: u32) -> Result<SavingsGoal, Error> {
        load_goal(&env, goal_id)
    }

    /// Sets the protocol treasury address to collect early withdrawal penalties.
    pub fn set_treasury(env: Env, admin: Address, treasury: Address) -> Result<(), Error> {
        admin.require_auth();
        env.storage().instance().set(&DataKey::Treasury, &treasury);
        bump_instance(&env);
        Ok(())
    }

    /// Sets the early withdrawal penalty rate in basis points (e.g., 500 = 5%).
    /// Maximum allowed is 5000 bps (50%).
    pub fn set_penalty_bps(env: Env, admin: Address, penalty_bps: u32) -> Result<(), Error> {
        admin.require_auth();
        if penalty_bps > 5000 {
            return Err(Error::PenaltyRateTooHigh);
        }
        env.storage().instance().set(&DataKey::PenaltyBps, &penalty_bps);
        bump_instance(&env);
        Ok(())
    }

    /// Allows the goal owner to withdraw locked funds before the goal is unlocked
    /// by accepting a protocol penalty fee. The penalty is transferred to the treasury
    /// and the remaining net funds are transferred to the owner.
    pub fn early_withdraw(env: Env, caller: Address, goal_id: u32) -> Result<(), Error> {
        caller.require_auth();

        let mut goal = load_goal(&env, goal_id)?;

        if caller != goal.owner {
            return Err(Error::Unauthorized);
        }
        if goal.withdrawn {
            return Err(Error::AlreadyWithdrawn);
        }
        if goal.current_amount <= 0 {
            return Err(Error::NoFundsToWithdraw);
        }

        let penalty_bps: u32 = env
            .storage()
            .instance()
            .get(&DataKey::PenaltyBps)
            .unwrap_or(500); // default 5% (500 bps)

        let total_amount = goal.current_amount;
        let penalty_amount = (total_amount * (penalty_bps as i128)) / 10000;
        let net_amount = total_amount - penalty_amount;

        let contract_address = env.current_contract_address();
        let token_client = token::TokenClient::new(&env, &goal.token);

        // Transfer penalty to treasury if configured and penalty > 0
        if penalty_amount > 0 {
            if let Some(treasury) = env.storage().instance().get::<_, Address>(&DataKey::Treasury) {
                token_client.transfer(&contract_address, &treasury, &penalty_amount);
            }
        }

        // Transfer net remaining amount to goal owner
        if net_amount > 0 {
            token_client.transfer(&contract_address, &caller, &net_amount);
        }

        goal.current_amount = 0;
        goal.withdrawn = true;

        save_goal(&env, goal_id, &goal);

        events::EarlyWithdrawal {
            goal_id,
            owner: caller,
            net_amount,
            penalty_amount,
        }
        .publish(&env);

        Ok(())
    }
}
