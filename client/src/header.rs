use std::sync::Arc;

use crate::Context;

pub struct HeaderApi {
    ctx: Arc<Context>,
}

impl HeaderApi {
    pub(crate) fn new(ctx: Arc<Context>) -> HeaderApi {
        HeaderApi { ctx }
    }

    /*
    // LocalHead returns the ExtendedHeader of the chain head.
    LocalHead(context.Context) (*header.ExtendedHeader, error)

    // GetByHash returns the header of the given hash from the node's header store.
    GetByHash(ctx context.Context, hash libhead.Hash) (*header.ExtendedHeader, error)
    // GetRangeByHeight returns the given range (from:to) of ExtendedHeaders
    // from the node's header store and verifies that the returned headers are
    // adjacent to each other.
    GetRangeByHeight(
        ctx context.Context,
        from *header.ExtendedHeader,
        to uint64,
    ) ([]*header.ExtendedHeader, error)
    // GetByHeight returns the ExtendedHeader at the given height if it is
    // currently available.
    GetByHeight(context.Context, uint64) (*header.ExtendedHeader, error)
    // WaitForHeight blocks until the header at the given height has been processed
    // by the store or context deadline is exceeded.
    WaitForHeight(context.Context, uint64) (*header.ExtendedHeader, error)

    // SyncState returns the current state of the header Syncer.
    SyncState(context.Context) (sync.State, error)
    // SyncWait blocks until the header Syncer is synced to network head.
    SyncWait(ctx context.Context) error
    // NetworkHead provides the Syncer's view of the current network head.
    NetworkHead(ctx context.Context) (*header.ExtendedHeader, error)

    // Subscribe to recent ExtendedHeaders from the network.
    Subscribe(ctx context.Context) (<-chan *header.ExtendedHeader, error)
    */
}
