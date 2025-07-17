use bitvec::{order::Msb0, vec::BitVec};

use celestia_proto::cosmos::crypto::multisig::v1beta1::CompactBitArray;

use crate::Error;

/// Vector of bits
#[derive(Debug, Clone, PartialEq)]
pub struct BitVector(pub BitVec<u8, Msb0>);

impl TryFrom<CompactBitArray> for BitVector {
    type Error = Error;

    fn try_from(value: CompactBitArray) -> Result<Self, Self::Error> {
        let num_bytes = value.elems.len();
        if num_bytes == 0 && value.extra_bits_stored != 0 {
            return Err(Error::MalformedCompactBitArray);
        }

        let last_byte_bits = match value.extra_bits_stored {
            0 => 8,
            n @ 1..=7 => n,
            _ => return Err(Error::MalformedCompactBitArray),
        } as usize;
        let num_bits = num_bytes.saturating_sub(1) * 8 + last_byte_bits;
        let mut bit_vec =
            BitVec::<_, Msb0>::try_from_vec(value.elems).map_err(|_| Error::BitarrayTooLarge)?;

        bit_vec.truncate(num_bits);

        Ok(BitVector(bit_vec))
    }
}

impl From<BitVector> for CompactBitArray {
    fn from(mut value: BitVector) -> Self {
        let num_bits = value.0.len();
        // make sure we don't touch uninitialised memory
        value.0.set_uninitialized(false);
        let bytes = value.0.into_vec();
        // N mod 8 must fit u32, this is safe
        let extra_bits_stored = (num_bits % 8) as u32;
        CompactBitArray {
            extra_bits_stored,
            elems: bytes,
        }
    }
}

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
pub use wbg::*;

#[cfg(all(target_arch = "wasm32", feature = "wasm-bindgen"))]
mod wbg {
    use super::BitVector;
    use wasm_bindgen::prelude::*;

    /// Array of bits
    #[wasm_bindgen(getter_with_clone)]
    pub struct JsBitVector(pub Vec<u8>);

    impl From<BitVector> for JsBitVector {
        fn from(value: BitVector) -> Self {
            JsBitVector(
                value
                    .0
                    .iter()
                    .map(|v| if *v { 1 } else { 0 })
                    .collect::<Vec<u8>>(),
            )
        }
    }
}

#[cfg(feature = "uniffi")]
mod uniffi_types {
    use super::BitVector as LuminaBitVector;
    use bitvec::vec::BitVec;
    use uniffi::Record;

    /// Vector of bits
    #[derive(Record)]
    pub struct BitVector {
        bytes: Vec<u8>,
    }

    impl From<BitVector> for LuminaBitVector {
        fn from(value: BitVector) -> Self {
            LuminaBitVector(BitVec::from_vec(value.bytes))
        }
    }

    impl From<LuminaBitVector> for BitVector {
        fn from(value: LuminaBitVector) -> Self {
            BitVector {
                bytes: value.0.into_vec(),
            }
        }
    }

    uniffi::custom_type!(LuminaBitVector, BitVector, {
        remote,
        try_lift: |value| Ok(value.into()),
        lower: |value| value.into()
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use bitvec::prelude::*;

    #[test]
    fn test_empty() {
        let source = CompactBitArray {
            extra_bits_stored: 0,
            elems: vec![],
        };
        let vec = BitVector::try_from(source.clone()).unwrap();
        assert!(vec.0.is_empty());
        let result = CompactBitArray::from(vec);
        assert_eq!(source, result);
    }

    #[test]
    fn byte_aligned() {
        let source = CompactBitArray {
            extra_bits_stored: 0,
            elems: vec![0b00000001, 0b00000010],
        };
        let expected_bits = bits![0, 0, 0, 0, 0, 0, 0, 1, 0, 0, 0, 0, 0, 0, 1, 0];

        let vec = BitVector::try_from(source.clone()).unwrap();
        assert_eq!(vec.0.len(), 16);
        assert_eq!(vec.0, expected_bits);

        let result = CompactBitArray::from(vec);
        assert_eq!(source, result);
    }

    // test relies on the fact that we're setting uninitialised part of bitfield to 0,
    // which we do anyway for safety
    #[test]
    fn with_extra_bytes() {
        let source = CompactBitArray {
            extra_bits_stored: 1,
            elems: vec![0b00000000, 0b10000000],
        };
        let expected_bits = bits![0, 0, 0, 0, 0, 0, 0, 0, 1];

        let vec = BitVector::try_from(source.clone()).unwrap();
        assert_eq!(vec.0.len(), 9);
        assert_eq!(vec.0, expected_bits);

        let result = CompactBitArray::from(vec);
        assert_eq!(source, result);
    }

    #[test]
    fn malformed() {
        let source = CompactBitArray {
            extra_bits_stored: 8,
            elems: vec![0b00000000],
        };
        let error = BitVector::try_from(source.clone()).unwrap_err();
        assert!(matches!(error, Error::MalformedCompactBitArray));

        let source = CompactBitArray {
            extra_bits_stored: 1,
            elems: vec![],
        };
        let error = BitVector::try_from(source.clone()).unwrap_err();
        assert!(matches!(error, Error::MalformedCompactBitArray));
    }
}
