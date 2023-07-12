mod address;

pub use self::address::{AccAddress, Address, ValAddress};

pub type Balance = cosmrs::Coin;
pub type Uint = ruint::aliases::U256;
