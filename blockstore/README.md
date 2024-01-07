# Blockstore

An IPLD blockstore capable of holding arbitrary data indexed by CID.

```rust
use blockstore::{Blockstore, InMemoryBlockstore};
use blockstore::block::{Block, CidError};
use cid::Cid;
use multihash_codetable::{Code, MultihashDigest};

const RAW_CODEC: u64 = 0x55;

struct RawBlock(Vec<u8>);

impl Block<64> for RawBlock {
    fn cid(&self) -> Result<Cid, CidError> {
        let hash = Code::Sha2_256.digest(&self.0);
        Ok(Cid::new_v1(RAW_CODEC, hash))
    }

    fn data(&self) -> &[u8] {
        self.0.as_ref()
    }
}

async fn put_and_get() {
    let block = RawBlock(vec![0xde, 0xad]);
    let cid = block.cid().unwrap();

    let blockstore = InMemoryBlockstore::<64>::new();
    blockstore.put(block).await.unwrap();

    let block = blockstore.get(&cid).await.unwrap();

    assert_eq!(block, Some(vec![0xde, 0xad]));
}

tokio::runtime::Builder::new_current_thread()
    .enable_all()
    .build()
    .unwrap()
    .block_on(put_and_get());
```
