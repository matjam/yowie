use thiserror::Error;

/// Errors from parsing dice notation.
#[derive(Debug, Clone, PartialEq, Eq, Error)]
pub enum RngError {
    /// The text is not in `NdM`, `dM`, `NdM+K` or `NdM-K` form.
    #[error("invalid dice notation {0:?}")]
    InvalidDice(String),
    /// The number of dice or sides is zero or exceeds the supported limit.
    #[error("dice count or sides out of range in {0:?}")]
    DiceOutOfRange(String),
}
