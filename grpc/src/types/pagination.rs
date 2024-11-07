use celestia_proto::cosmos::base::query::v1beta1::pagination::PageRequest as RawPageRequest;
use celestia_proto::cosmos::base::query::v1beta1::pagination::PageResponse as RawPageResponse;

use crate::tonic::types::FromGrpcResponse;
use crate::tonic::Error;

pub enum KeyOrOffset {
    Key(Vec<u8>),
    Offset(u64),
}

pub struct PageRequest {
    query_start: KeyOrOffset,
    limit: Option<u64>,
    count_total: bool,
    reverse: bool,
}

impl From<PageRequest> for RawPageRequest {
    fn from(item: PageRequest) -> RawPageRequest {
        RawPageRequest {

        }
    }
}

pub struct PageResponse {
    pub next_key: Option<Vec<u8>>,
    pub total: Option<u64>,
}
