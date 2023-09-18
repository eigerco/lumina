use celestia_types::ExtendedHeader;
use dashmap::mapref::entry::Entry;
use dashmap::DashMap;
use tendermint::Hash;
use thiserror::Error;
use tracing::{error, info};

use crate::exchange::ExtendedHeaderExt;

#[derive(Debug)]
pub struct Store {
    headers: DashMap<Hash, ExtendedHeader>,
    height_to_hash: DashMap<u64, Hash>,
}

#[derive(Error, Debug, PartialEq)]
pub enum InsertError {
    // TODO: do we care about the distinction
    #[error("Hash {0} already exists in store")]
    HashExists(Hash),
    #[error("Height {0} already exists in store")]
    HeightExists(u64),
}

#[derive(Error, Debug, PartialEq)]
pub enum ReadError {
    // TODO: should we roll internal errors into one
    #[error("Store in inconsistent state, lost head")]
    LostStoreHead,
    #[error("Store in inconsistent state, height->hash mapping exists, but hash doesn't")]
    LostHash,
    #[error("Failed to convert height from usize to u64, should not happen")]
    HeadHeightConversionError,

    #[error("Header not found in store")]
    NotFound,
}

impl Store {
    pub fn new() -> Self {
        Store {
            headers: DashMap::new(),
            height_to_hash: DashMap::new(),
        }
    }

    pub fn get_head_height(&self) -> usize {
        self.height_to_hash.len()
    }

    pub fn append_header(&self, header: ExtendedHeader) -> Result<(), InsertError> {
        let hash = header.hash();
        let height = header.height(); // TODO: should Store be responsible for making sure we have
                                      // all 1..=head headers?

        // lock both maps to ensure consistency
        // this shouldn't deadlock as long as we don't hold references across awaits if any
        // https://github.com/xacrimon/dashmap/issues/233
        let hash_entry = self.headers.entry(hash);
        let height_entry = self.height_to_hash.entry(height.into());

        if matches!(hash_entry, Entry::Occupied(_)) {
            return Err(InsertError::HashExists(hash));
        }
        if matches!(height_entry, Entry::Occupied(_)) {
            return Err(InsertError::HeightExists(height.into()));
        }

        info!("Will insert {hash} at {height}");
        hash_entry.insert(header);
        height_entry.insert(hash);

        Ok(())
    }

    pub fn get_head(&self) -> Result<ExtendedHeader, ReadError> {
        let head_height = match self.height_to_hash.len() {
            0 => return Err(ReadError::NotFound),
            height => match u64::try_from(height) {
                Ok(h) => h,
                Err(e) => {
                    error!("Failed to convert {height} from usize to u64, should not happen: {e}");
                    return Err(ReadError::HeadHeightConversionError);
                }
            },
        };

        let Some(head_hash) = self.height_to_hash.get(&head_height).map(|v| *v) else {
            error!("height_to_hash[height_to_hash.len()] not found, store is inconsistent");
            return Err(ReadError::LostStoreHead);
        };

        match self.headers.get(&head_hash) {
            Some(v) => Ok(v.clone()),
            None => {
                error!("Header with hash {head_hash} for height {head_height} missing");
                Err(ReadError::LostHash)
            }
        }
    }

    pub fn get_by_hash(&self, hash: &Hash) -> Result<ExtendedHeader, ReadError> {
        match self.headers.get(hash) {
            Some(v) => Ok(v.clone()),
            None => Err(ReadError::NotFound),
        }
    }

    pub fn get_by_height(&self, height: u64) -> Result<ExtendedHeader, ReadError> {
        let Some(hash) = self.height_to_hash.get(&height).map(|h| *h) else {
            return Err(ReadError::NotFound);
        };

        match self.headers.get(&hash) {
            Some(h) => Ok(h.clone()),
            None => {
                error!("Lost hash {hash} at height {height}");
                Err(ReadError::LostHash)
            }
        }
    }

    #[doc(hidden)]
    pub fn test_filled_store(height: u64) -> Self {
        let s = Store::new();

        // block height is 1-indexed
        for height in 1..=height {
            s.append_header(ExtendedHeader::with_height(height))
                .expect("inserting test data failed");
        }

        s
    }
}

impl Default for Store {
    fn default() -> Self {
        Store::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use celestia_types::ExtendedHeader;
    use tendermint::block::Height;
    use tendermint::Hash;

    #[test]
    fn test_empty_store() {
        let s = Store::new();
        assert_eq!(s.get_head_height(), 0);
        assert_eq!(s.get_head(), Err(ReadError::NotFound));
        assert_eq!(s.get_by_height(1), Err(ReadError::NotFound));
        assert_eq!(
            s.get_by_hash(&Hash::Sha256([0; 32])),
            Err(ReadError::NotFound)
        );
    }

    #[test]
    fn test_read_write() {
        let s = Store::new();
        let header = ExtendedHeader::with_height(1);
        s.append_header(header.clone()).unwrap();
        assert_eq!(s.get_head_height(), 1);
        assert_eq!(s.get_head().unwrap(), header);
        assert_eq!(s.get_by_height(1).unwrap(), header);
        assert_eq!(s.get_by_hash(&header.hash()).unwrap(), header);
    }

    #[test]
    fn test_pregenerated_data() {
        let s = Store::test_filled_store(100);
        assert_eq!(s.get_head_height(), 100);
        let head = s.get_head().unwrap();
        assert_eq!(s.get_by_height(100), Ok(head));
        assert_eq!(s.get_by_height(101), Err(ReadError::NotFound));

        let header = s.get_by_height(54).unwrap();
        assert_eq!(s.get_by_hash(&header.hash()), Ok(header));
    }

    #[test]
    fn test_duplicate_insert() {
        let s = Store::test_filled_store(100);
        let header = ExtendedHeader::with_height(101);
        assert_eq!(s.append_header(header.clone()), Ok(()));
        assert_eq!(
            s.append_header(header.clone()),
            Err(InsertError::HashExists(header.hash()))
        );
    }

    #[test]
    fn test_overwrite_height() {
        let s = Store::test_filled_store(100);
        let insert_existing_result = s.append_header(ExtendedHeader::with_height(30));
        assert_eq!(insert_existing_result, Err(InsertError::HeightExists(30)));
    }

    #[test]
    fn test_overwrite_hash() {
        let s = Store::test_filled_store(100);
        let mut dup_header = s.get_by_height(33).unwrap();
        dup_header.header.height = Height::from(101u32);
        let insert_existing_result = s.append_header(dup_header.clone());
        assert_eq!(
            insert_existing_result,
            Err(InsertError::HashExists(dup_header.hash()))
        );
    }
}
