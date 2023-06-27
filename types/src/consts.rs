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
    // celestia-app/pkg/appconsts/v1/app_consts
    pub mod v1 {
        pub const SQUARE_SIZE_UPPER_BOUND: usize = 128;
    }

    // celestia-app/pkg/appconsts/global_consts
    pub mod global_consts {
        pub const MIN_SQUARE_SIZE: usize = 1;
    }
}

// celestia-app/pkg/da/data_availability_header
pub mod data_availability_header {
    pub const MAX_EXTENDED_SQUARE_WIDTH: usize = super::appconsts::v1::SQUARE_SIZE_UPPER_BOUND * 2;
    pub const MIN_EXTENDED_SQUARE_WIDTH: usize =
        super::appconsts::global_consts::MIN_SQUARE_SIZE * 2;
}
