use soroban_sdk::{contracttype, Address};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Goal(u32),
    GoalCounter,
    Treasury,
    PenaltyBps,
}

#[contracttype]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SavingsGoal {
    pub owner: Address,
    pub token: Address,
    pub target_amount: i128,
    pub current_amount: i128,
    pub deadline: u64,
    pub unlocked: bool,
    pub withdrawn: bool,
}
