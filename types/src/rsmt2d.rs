use serde::{Deserialize, Serialize};

/// The data matrix in Celestia blocks extended with parity data.
///
/// It is created by a fixed size chunks of data, called [`Share`]s.
/// Each share is a cell of the [`ExtendedDataSquare`].
///
/// # Structure
///
/// The [`ExtendedDataSquare`] consists of four quadrants. The first
/// quadrant (upper-left) is the original data submitted to the network,
/// referred to as `OriginalDataSquare`. The rest three quadrants are
/// the parity data encoded row-wise or column-wise using reed-solomon
/// `codec` specified in `EDS`.
///
/// The below diagram shows how the `EDS` is constructed. First, the 2nd
/// and 3rd quadrants are created by computing reed-solomon parity data
/// of the original data square, row-wise for 2nd and column-wise for
/// 3rd quadrant. Then, the 4th quadrant is computed either row-wise
/// from 3rd or column-wise from 2nd quadrant.
///
/// ```text
///  ---------------------------
/// |             |             |
/// |           --|->           |
/// |      1    --|->    2      |
/// |           --|->           |
/// |    | | |    |             |
///  -------------+-------------
/// |    v v v    |             |
/// |           --|->           |
/// |      3    --|->    4      |
/// |           --|->           |
/// |             |             |
///  ---------------------------
/// ```
///
/// # Data availability
///
/// The [`DataAvailabilityHeader`] is created by computing [`Nmt`] merkle
/// roots of each row and column of [`ExtendedDataSquare`].
/// By putting those together there are some key
/// properties those have in terms of data availability.
///
/// Thanks to the parity data, to make original data unrecoverable, a malicious
/// actor would need to hide more than a half of data from each row and column.
/// If we take `k` as the width of the `OriginalDataSquare` then the attacker
/// would need to hide more than `(k + 1)^2` data from the [`ExtendedDataSquare`].
/// For the `EDS` that is 4 width an attacker would need to hide more than 50% of
/// all the shares and that value approaches the 25% as the square grows.
///
/// This allows for really efficient data sampling, as the sampling node can reach
/// very high confidence that whole data is available by taking only a few samples.
///
/// # Example
///
/// This example shows rebuilding the merkle trees for each row of the EDS and compares
/// them with the root hashes stored in data availability header.
///
/// ```no_run
/// use celestia_types::nmt::{Namespace, NamespacedSha2Hasher, Nmt};
/// use celestia_types::Share;
/// use nmt_rs::NamespaceMerkleHasher;
/// # use celestia_types::{ExtendedDataSquare, ExtendedHeader};
/// # fn get_header(_: usize) -> ExtendedHeader {
/// #     unimplemented!()
/// # }
/// # fn get_eds(_: usize) -> ExtendedDataSquare {
/// #     unimplemented!()
/// # }
///
/// let block_height = 15;
/// let header = get_header(block_height);
/// let eds = get_eds(block_height);
/// let width = header.dah.square_len();
///
/// // for each row of the data square, build an nmt
/// for (y, row) in eds.data_square.chunks(width).enumerate() {
///     let mut nmt = Nmt::with_hasher(NamespacedSha2Hasher::with_ignore_max_ns(true));
///
///     for (x, leaf) in row.iter().enumerate() {
///         if x < width / 2 && y < width / 2 {
///             // the `OriginalDataSquare` part of the `EDS`
///             let share = Share::from_raw(leaf).unwrap();
///             let ns = share.namespace();
///             nmt.push_leaf(share.as_ref(), *ns).unwrap();
///         } else {
///             // the parity data computed using `eds.codec`
///             nmt.push_leaf(leaf, *Namespace::PARITY_SHARE).unwrap();
///         }
///     }
///
///     // check if the root corresponds to the one from the dah
///     let root = nmt.root();
///     assert_eq!(root, header.dah.row_root(y).unwrap());
/// }
/// ```
///
/// [`Nmt`]: crate::nmt::Nmt
/// [`Share`]: crate::share::Share
/// [`DataAvailabilityHeader`]: crate::DataAvailabilityHeader
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExtendedDataSquare {
    /// The raw data of the EDS.
    #[serde(with = "tendermint_proto::serializers::bytes::vec_base64string")]
    pub data_square: Vec<Vec<u8>>,
    /// The codec used to encode parity shares.
    pub codec: String,
}
