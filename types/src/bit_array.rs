use bitvec::{order::Msb0, vec::BitVec};

use celestia_proto::cosmos::crypto::multisig::v1beta1::CompactBitArray;

use crate::Error;

#[derive(Debug, Clone, PartialEq)]
pub struct BitVector(pub BitVec<u8, Msb0>);

impl TryFrom<CompactBitArray> for BitVector {
    type Error = Error;

    fn try_from(value: CompactBitArray) -> Result<Self, Self::Error> {
        let num_bits = (value.elems.len() - 1) * 8 + value.extra_bits_stored as usize;
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

#[cfg(test)]
mod tests {
    use super::*;

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
}
