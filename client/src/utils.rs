use crate::{Error, Result};

pub(crate) fn height_i64(height: u64) -> Result<i64> {
    height.try_into().map_err(|_| Error::InvalidHeight(height))
}
