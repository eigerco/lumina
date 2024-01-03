use crate::consts::appconsts;
use crate::{Error, Result};

/// The part of [`Share`] containing the `version` and `sequence_start` information.
///
/// [`InfoByte`] is a single byte with the following structure:
///  - the first 7 bits are reserved for version information in big endian form
///  - last bit is a `sequence_start` flag. If it's set then this [`Share`] is
///  a first of a sequence, otherwise it's a continuation share.
///
///  [`Share`]: crate::Share
#[repr(transparent)]
pub struct InfoByte(u8);

impl InfoByte {
    /// Create a new [`InfoByte`] with given version and `sequence_start`.
    pub fn new(version: u8, is_sequence_start: bool) -> Result<Self> {
        if version > appconsts::MAX_SHARE_VERSION {
            Err(Error::MaxShareVersionExceeded(version))
        } else {
            let prefix = version << 1;
            let sequence_start = if is_sequence_start { 1 } else { 0 };
            Ok(Self(prefix + sequence_start))
        }
    }

    /// Get the `version`.
    pub fn version(&self) -> u8 {
        self.0 >> 1
    }

    /// Get the `sequence_start` indicator.
    pub fn is_sequence_start(&self) -> bool {
        self.0 % 2 == 1
    }

    /// Convert the [`InfoByte`] to a byte.
    pub fn as_u8(&self) -> u8 {
        self.0
    }
}
