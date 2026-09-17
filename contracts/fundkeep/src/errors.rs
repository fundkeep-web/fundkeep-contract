use soroban_sdk::contracterror;

#[contracterror]
#[derive(Copy, Clone, Debug, Eq, PartialEq, PartialOrd, Ord)]
#[repr(u32)]
pub enum Error {
    GoalNotFound = 1,
    NotUnlocked = 2,
    AlreadyWithdrawn = 3,
    Unauthorized = 4,
    InvalidAmount = 5,
    InvalidDeadline = 6,
    InvalidToken = 8,
}
