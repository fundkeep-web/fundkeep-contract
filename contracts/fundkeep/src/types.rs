use soroban_sdk::{contracttype, Address};

#[contracttype]
#[derive(Clone)]
pub enum DataKey {
    Goal(u32),
    GoalCounter,
    GoalContribution(u32, Address),
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
    pub is_group: bool,
}
