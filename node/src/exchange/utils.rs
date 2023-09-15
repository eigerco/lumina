use celestia_proto::p2p::pb::header_request::Data;
use celestia_proto::p2p::pb::{HeaderRequest, HeaderResponse, StatusCode};
use celestia_types::consts::HASH_SIZE;
use celestia_types::{ExtendedHeader, Hash};
use tendermint_proto::Protobuf;

use crate::exchange::ExchangeError;

pub(super) trait HeaderRequestExt {
    fn with_origin(origin: u64, amount: u64) -> HeaderRequest;
    fn with_hash(hash: Hash) -> HeaderRequest;
    fn is_valid(&self) -> bool;
    fn is_head_request(&self) -> bool;
}

impl HeaderRequestExt for HeaderRequest {
    fn with_origin(origin: u64, amount: u64) -> HeaderRequest {
        HeaderRequest {
            amount,
            data: Some(Data::Origin(origin)),
        }
    }

    fn with_hash(hash: Hash) -> HeaderRequest {
        HeaderRequest {
            amount: 1,
            data: Some(Data::Hash(hash.as_bytes().to_vec())),
        }
    }

    fn is_valid(&self) -> bool {
        match (&self.data, self.amount) {
            (None, _) | (_, 0) => false,
            (Some(Data::Origin(0)), amount) if amount > 1 => false,
            (Some(Data::Hash(hash)), amount) if hash.len() != HASH_SIZE || amount > 1 => false,
            _ => true,
        }
    }

    fn is_head_request(&self) -> bool {
        matches!((&self.data, self.amount), (Some(Data::Origin(0)), 1))
    }
}

pub(super) trait HeaderResponseExt {
    fn to_extended_header(&self) -> Result<ExtendedHeader, ExchangeError>;
}

impl HeaderResponseExt for HeaderResponse {
    fn to_extended_header(&self) -> Result<ExtendedHeader, ExchangeError> {
        match self.status_code() {
            StatusCode::Invalid => Err(ExchangeError::InvalidResponse),
            StatusCode::NotFound => Err(ExchangeError::HeaderNotFound),
            StatusCode::Ok => {
                ExtendedHeader::decode(&self.body[..]).map_err(|_| ExchangeError::InvalidResponse)
            }
        }
    }
}

pub(super) trait ExtendedHeaderExt {
    fn to_header_response(&self) -> HeaderResponse;
}

impl ExtendedHeaderExt for ExtendedHeader {
    fn to_header_response(&self) -> HeaderResponse {
        HeaderResponse {
            body: self.encode_vec().unwrap(),
            status_code: StatusCode::Ok.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_request_no_data() {
        let req = HeaderRequest {
            data: None,
            amount: 1,
        };
        assert!(!req.is_valid());

        let req = HeaderRequest {
            data: None,
            amount: 2,
        };
        assert!(!req.is_valid());
    }

    #[test]
    fn header_request_zero_amount() {
        let req = HeaderRequest {
            data: Some(Data::Origin(1)),
            amount: 0,
        };
        assert!(!req.is_valid());

        let req = HeaderRequest {
            data: Some(Data::Hash([0xff; 32].to_vec())),
            amount: 0,
        };
        assert!(!req.is_valid());
    }

    #[test]
    fn header_request_head() {
        let req = HeaderRequest {
            data: Some(Data::Origin(0)),
            amount: 1,
        };
        assert!(req.is_valid());
        assert!(req.is_head_request());

        let req = HeaderRequest {
            data: Some(Data::Origin(0)),
            amount: 2,
        };
        assert!(!req.is_valid());
        assert!(!req.is_head_request());
    }

    #[test]
    fn header_request_hash() {
        let req = HeaderRequest {
            data: Some(Data::Hash([0xff; 32].to_vec())),
            amount: 1,
        };
        assert!(req.is_valid());

        let req = HeaderRequest {
            data: Some(Data::Hash([0xff; 32].to_vec())),
            amount: 2,
        };
        assert!(!req.is_valid());

        let req = HeaderRequest {
            data: Some(Data::Hash(Vec::new())),
            amount: 1,
        };
        assert!(!req.is_valid());

        let req = HeaderRequest {
            data: Some(Data::Hash([0xff; 31].to_vec())),
            amount: 1,
        };
        assert!(!req.is_valid());

        let req = HeaderRequest {
            data: Some(Data::Hash([0xff; 33].to_vec())),
            amount: 1,
        };
        assert!(!req.is_valid());
    }
}