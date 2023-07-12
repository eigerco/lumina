// celestia-core/types/genesis
pub mod genesis {
    pub const MAX_CHAIN_ID_LEN: usize = 50;
}

// celestia-core/version/version
pub mod version {
    // BLOCK_PROTOCOL versions all block data structures and processing.
    // This includes validity of blocks and state updates.
    pub const BLOCK_PROTOCOL: u64 = 11;
}

pub mod appconsts {
    pub use global_consts::*;
    pub use v1::*;

    // celestia-app/pkg/appconsts/v1/app_consts
    mod v1 {
        pub const SUBTREE_ROOT_THRESHOLD: u64 = 64;
        pub const SQUARE_SIZE_UPPER_BOUND: usize = 128;
    }

    // celestia-app/pkg/appconsts/global_consts
    mod global_consts {
        use crate::nmt::NS_SIZE;

        /// the size of the namespace.
        pub const NAMESPACE_SIZE: usize = NS_SIZE;

        /// the size of a share in bytes.
        pub const SHARE_SIZE: usize = 512;

        /// the number of bytes reserved for information. The info
        /// byte contains the share version and a sequence start idicator.
        pub const SHARE_INFO_BYTES: usize = 1;

        /// the number of bytes reserved for the sequence length
        /// that is present in the first share of a sequence.
        pub const SEQUENCE_LEN_BYTES: usize = 4;

        /// the first share version format.
        pub const SHARE_VERSION_ZERO: u8 = 0;

        /// the number of bytes reserved for the location of
        /// the first unit (transaction, ISR) in a compact share.
        pub const COMPACT_SHARE_RESERVED_BYTES: usize = 4;

        /// the number of bytes usable for data in the first compact share of a sequence.
        pub const FIRST_COMPACT_SHARE_CONTENT_SIZE: usize = SHARE_SIZE
            - NAMESPACE_SIZE
            - SHARE_INFO_BYTES
            - SEQUENCE_LEN_BYTES
            - COMPACT_SHARE_RESERVED_BYTES;

        /// the number of bytes usable for data in a continuation compact share of a sequence.
        pub const CONTINUATION_COMPACT_SHARE_CONTENT_SIZE: usize =
            SHARE_SIZE - NAMESPACE_SIZE - SHARE_INFO_BYTES - COMPACT_SHARE_RESERVED_BYTES;

        /// the number of bytes usable for data in the first sparse share of a sequence.
        pub const FIRST_SPARSE_SHARE_CONTENT_SIZE: usize =
            SHARE_SIZE - NAMESPACE_SIZE - SHARE_INFO_BYTES - SEQUENCE_LEN_BYTES;

        /// the number of bytes usable for data in a continuation sparse share of a sequence.
        pub const CONTINUATION_SPARSE_SHARE_CONTENT_SIZE: usize =
            SHARE_SIZE - NAMESPACE_SIZE - SHARE_INFO_BYTES;

        /// the smallest original square width.
        pub const MIN_SQUARE_SIZE: usize = 1;

        /// the minimum number of shares allowed in the original data square.
        pub const MIN_SHARE_COUNT: usize = MIN_SQUARE_SIZE * MIN_SQUARE_SIZE;

        /// the maximum value a share version can be.
        pub const MAX_SHARE_VERSION: u8 = 127;
    }
}

// celestia-app/pkg/da/data_availability_header
pub mod data_availability_header {
    pub const MAX_EXTENDED_SQUARE_WIDTH: usize = super::appconsts::SQUARE_SIZE_UPPER_BOUND * 2;
    pub const MIN_EXTENDED_SQUARE_WIDTH: usize = super::appconsts::MIN_SQUARE_SIZE * 2;
}

pub mod cosmos {
    use const_format::concatcp;

    const PREFIX_ACCOUNT: &str = "celestia";
    const PREFIX_PUBLIC: &str = "pub";
    const PREFIX_VALIDATOR: &str = "val";
    const PREFIX_OPERATOR: &str = "oper";
    const PREFIX_CONSENSUS: &str = "cons";

    /// Bech32PrefixAccAddr defines the Bech32 prefix of an account's address.
    pub const BECH32_PREFIX_ACC_ADDR: &str = PREFIX_ACCOUNT;

    /// Bech32PrefixAccPub defines the Bech32 prefix of an account's public key.
    pub const BECH32_PREFIX_ACC_PUB: &str = concatcp!(BECH32_PREFIX_ACC_ADDR, PREFIX_PUBLIC);

    /// Bech32PrefixValAddr defines the Bech32 prefix of a validator's operator address.
    pub const BECH32_PREFIX_VAL_ADDR: &str =
        concatcp!(PREFIX_ACCOUNT, PREFIX_VALIDATOR, PREFIX_OPERATOR);

    /// Bech32PrefixValPub defines the Bech32 prefix of a validator's operator public key.
    pub const BECH32_PREFIX_VAL_PUB: &str = concatcp!(BECH32_PREFIX_VAL_ADDR, PREFIX_PUBLIC);

    /// Bech32PrefixConsAddr defines the Bech32 prefix of a consensus node address.
    pub const BECH32_PREFIX_CONS_ADDR: &str =
        concatcp!(PREFIX_ACCOUNT, PREFIX_VALIDATOR, PREFIX_CONSENSUS);

    /// Bech32PrefixConsPub defines the Bech32 prefix of a consensus node public key.
    pub const BECH32_PREFIX_CONS_PUB: &str = concatcp!(BECH32_PREFIX_CONS_ADDR, PREFIX_PUBLIC);
}
