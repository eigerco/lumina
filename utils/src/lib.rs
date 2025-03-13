//! Utilities providing platform abstraction used across lumina project

/// async executor platform independent utilities
#[cfg(feature = "executor")]
pub mod executor;
#[cfg(any(test, feature = "test-utils"))]
pub mod test_utils;
/// platform independent timers
#[cfg(feature = "time")]
pub mod time;
/// platform independent cancellation token
#[cfg(feature = "token")]
pub mod token;
