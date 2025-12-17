// TODO: remove this
#![allow(unused)]

use bytes::{Buf, BufMut, BytesMut};
use celestia_proto::shwap::{
    Row as RawRow, RowNamespaceData as RawRowNamespaceData, Sample as RawSample, Share as RawShare,
};
use celestia_types::consts::appconsts::SHARE_SIZE;
use celestia_types::eds::{EdsId, ExtendedDataSquare};
use celestia_types::namespace_data::{NamespaceData, NamespaceDataId};
use celestia_types::row::{ROW_ID_SIZE, Row, RowId};
use celestia_types::sample::{Sample, SampleId};
use celestia_types::{DataAvailabilityHeader, ExtendedHeader};
use integer_encoding::VarInt;
use prost::Message;
use std::fmt::Display;

type Result<T, E = CodecError> = std::result::Result<T, E>;

#[derive(Debug, thiserror::Error)]
pub(crate) enum CodecError {
    #[error("Request decode failed: {0}")]
    RequestDecodeFailed(String),
    #[error("Response decode failed: {0}")]
    ResponseDecodeFailed(String),
    #[error("Response verification failed: {0}")]
    ResponseVerificationFailed(String),
}

impl CodecError {
    fn request_decode_failed(s: impl Display) -> CodecError {
        CodecError::RequestDecodeFailed(s.to_string())
    }

    fn response_decode_failed(s: impl Display) -> CodecError {
        CodecError::ResponseDecodeFailed(s.to_string())
    }

    fn response_verification_failed(s: impl Display) -> CodecError {
        CodecError::ResponseVerificationFailed(s.to_string())
    }
}

pub(crate) trait RequestCodec: Send + Sized {
    fn encode(&self) -> Vec<u8>;
    fn decode(raw_data: &[u8]) -> Result<Self>;
}

pub(crate) trait ResponseCodec: Send + Sized {
    type Request;

    fn encode(&self) -> Vec<u8>;

    fn decode_and_verify(
        raw_data: &[u8],
        req: &Self::Request,
        header: &ExtendedHeader,
    ) -> Result<Self>;
}

impl RequestCodec for RowId {
    fn encode(&self) -> Vec<u8> {
        let mut bytes = BytesMut::new();
        self.encode(&mut bytes);
        bytes.into()
    }

    fn decode(raw_data: &[u8]) -> Result<RowId> {
        RowId::decode(raw_data).map_err(CodecError::request_decode_failed)
    }
}

impl ResponseCodec for Row {
    type Request = RowId;

    fn encode(&self) -> Vec<u8> {
        let raw = RawRow::from(self.clone());
        raw.encode_length_delimited_to_vec()
    }

    fn decode_and_verify(raw_data: &[u8], req: &RowId, header: &ExtendedHeader) -> Result<Row> {
        let raw_row = RawRow::decode_length_delimited(raw_data)
            .map_err(CodecError::response_decode_failed)?;

        let row =
            Row::from_raw(req.to_owned(), raw_row).map_err(CodecError::response_decode_failed)?;

        row.verify(req.to_owned(), &header.dah)
            .map_err(CodecError::response_verification_failed)?;

        Ok(row)
    }
}

impl RequestCodec for SampleId {
    fn encode(&self) -> Vec<u8> {
        let mut bytes = BytesMut::new();
        self.encode(&mut bytes);
        bytes.into()
    }

    fn decode(raw_data: &[u8]) -> Result<SampleId> {
        SampleId::decode(raw_data).map_err(CodecError::request_decode_failed)
    }
}

impl ResponseCodec for Sample {
    type Request = SampleId;

    fn encode(&self) -> Vec<u8> {
        let raw = RawSample::from(self.clone());
        raw.encode_length_delimited_to_vec()
    }

    fn decode_and_verify(
        raw_data: &[u8],
        req: &SampleId,
        header: &ExtendedHeader,
    ) -> Result<Sample> {
        let raw_sample = RawSample::decode_length_delimited(raw_data)
            .map_err(CodecError::response_decode_failed)?;

        let sample = Sample::from_raw(req.to_owned(), raw_sample)
            .map_err(CodecError::response_decode_failed)?;

        sample
            .verify(req.to_owned(), &header.dah)
            .map_err(CodecError::response_verification_failed)?;

        Ok(sample)
    }
}

impl RequestCodec for EdsId {
    fn encode(&self) -> Vec<u8> {
        let mut bytes = BytesMut::new();
        self.encode(&mut bytes);
        bytes.into()
    }

    fn decode(raw_data: &[u8]) -> Result<EdsId> {
        EdsId::decode(raw_data).map_err(CodecError::request_decode_failed)
    }
}

impl ResponseCodec for ExtendedDataSquare {
    type Request = EdsId;

    fn encode(&self) -> Vec<u8> {
        let ods_width = self.square_width() / 2;
        let mut bytes =
            BytesMut::with_capacity(usize::from(ods_width) * usize::from(ods_width) * SHARE_SIZE);

        for row in 0..ods_width {
            for col in 0..ods_width {
                let share = self.share(row, col).expect("Invalid square_width");
                debug_assert!(!share.is_parity());
                bytes.put_slice(&share.data()[..]);
            }
        }

        bytes.into()
    }

    fn decode_and_verify(
        raw_data: &[u8],
        req: &EdsId,
        header: &ExtendedHeader,
    ) -> Result<ExtendedDataSquare> {
        if raw_data.len() == 0 {
            return Err(CodecError::response_decode_failed("Empty raw data"));
        }

        if raw_data.len() % SHARE_SIZE != 0 {
            return Err(CodecError::response_decode_failed(
                "Number of shares not divisible by SHARE_SIZE",
            ));
        }

        let mut ods_shares = Vec::new();

        for raw_share in raw_data.chunks(SHARE_SIZE) {
            ods_shares.push(raw_share.to_vec());
        }

        let app_version = header
            .app_version()
            .map_err(CodecError::response_decode_failed)?;

        let eds = ExtendedDataSquare::from_ods(ods_shares, app_version)
            .map_err(CodecError::response_decode_failed)?;

        let dah = DataAvailabilityHeader::from_eds(&eds);

        if dah.hash() != header.dah.hash() {
            return Err(CodecError::response_decode_failed(
                "EDS verification failed: Hash missmatch",
            ));
        }

        Ok(eds)
    }
}

impl RequestCodec for NamespaceDataId {
    fn encode(&self) -> Vec<u8> {
        let mut bytes = BytesMut::new();
        self.encode(&mut bytes);
        bytes.into()
    }

    fn decode(raw_data: &[u8]) -> Result<NamespaceDataId> {
        NamespaceDataId::decode(raw_data).map_err(CodecError::request_decode_failed)
    }
}

impl ResponseCodec for NamespaceData {
    type Request = NamespaceDataId;

    fn encode(&self) -> Vec<u8> {
        let mut bytes = BytesMut::new();

        for row in self.rows() {
            let raw_row = RawRowNamespaceData::from(row.to_owned());

            let encoded_len = raw_row.encoded_len();
            let varint_len = encoded_len.required_space();
            bytes.reserve(varint_len + encoded_len);

            raw_row
                .encode_length_delimited(&mut bytes)
                .expect("need more capacity");
        }

        bytes.into()
    }

    fn decode_and_verify(
        mut raw_data: &[u8],
        req: &NamespaceDataId,
        header: &ExtendedHeader,
    ) -> Result<NamespaceData> {
        let mut raw_rows = Vec::new();

        while !raw_data.is_empty() {
            let raw_row = RawRowNamespaceData::decode_length_delimited(&mut raw_data)
                .map_err(CodecError::response_decode_failed)?;

            raw_rows.push(raw_row);
        }

        let ns_data =
            NamespaceData::from_raw(*req, raw_rows).map_err(CodecError::response_decode_failed)?;

        ns_data
            .verify(*req, &header.dah)
            .map_err(CodecError::response_verification_failed)?;

        Ok(ns_data)
    }
}
