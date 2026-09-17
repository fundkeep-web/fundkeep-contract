use soroban_sdk::{contractevent, Address, String, Symbol};

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct GoalCreated {
    #[topic]
    pub goal_id: u32,
    pub owner: Address,
    pub token: Address,
    pub target_amount: i128,
    pub deadline: u64,
    pub metadata_uri: Option<String>,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Deposit {
    #[topic]
    pub goal_id: u32,
    pub caller: Address,
    pub amount: i128,
    pub current_amount: i128,
    pub unlocked: bool,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Unlock {
    #[topic]
    pub goal_id: u32,
    pub via: Symbol,
}

#[contractevent]
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Withdraw {
    #[topic]
    pub goal_id: u32,
    pub owner: Address,
    pub amount: i128,
}
