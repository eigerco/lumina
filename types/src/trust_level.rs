//! Primitives for the trust level of consensus commits.

use crate::{VerificationError, verification_error};

/// A default trust level for the optimistic commit verification.
pub const DEFAULT_TRUST_LEVEL: TrustLevelRatio = TrustLevelRatio::new(1, 3);

/// The representation of the trust level.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustLevelRatio {
    numerator: u64,
    denominator: u64,
}

impl TrustLevelRatio {
    /// Create a new [`TrustLevelRatio`].
    pub const fn new(numerator: u64, denominator: u64) -> Self {
        TrustLevelRatio {
            numerator,
            denominator,
        }
    }

    /// Get the numerator component of the fraction.
    pub fn numerator(&self) -> u64 {
        self.numerator
    }

    /// Get the denominator component of the fraction.
    pub fn denominator(&self) -> u64 {
        self.denominator
    }

    /// Get the amount of voting power needed to satisfy this trust level.
    ///
    /// # Errors
    ///
    /// This function will return an error if the operation overflows or if denominator is 0.
    pub fn voting_power_needed(
        &self,
        total_voting_power: impl Into<u64>,
    ) -> Result<u64, VerificationError> {
        self.numerator
            .checked_mul(total_voting_power.into())
            .ok_or_else(|| {
                verification_error!("u64 overflow while calculating voting power needed")
            })?
            .checked_div(self.denominator)
            .ok_or_else(|| {
                verification_error!("division error while calculating voting power needed")
            })
    }
}
