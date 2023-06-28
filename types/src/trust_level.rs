use crate::{verification_error, VerificationError};

pub const DEFAULT_TRUST_LEVEL: TrustLevelRatio = TrustLevelRatio::new(1, 3);

pub struct TrustLevelRatio {
    numerator: u64,
    denominator: u64,
}

impl TrustLevelRatio {
    pub const fn new(numerator: u64, denominator: u64) -> Self {
        TrustLevelRatio {
            numerator,
            denominator,
        }
    }

    pub fn numerator(&self) -> u64 {
        self.numerator
    }

    pub fn denominator(&self) -> u64 {
        self.denominator
    }

    pub fn total_power_needed(
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
