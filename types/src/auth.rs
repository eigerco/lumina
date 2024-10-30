//! TODO:

use cosmrs::crypto::PublicKey;

/// Params defines the parameters for the auth module.
#[derive(Debug)]
pub struct AuthParams {
    /// Maximum number of memo characters
    pub max_memo_characters: u64,
    /// Maximum nubmer of signatures
    pub tx_sig_limit: u64,
    /// Cost per transaction byte
    pub tx_size_cost_per_byte: u64,
    /// Cost to verify ed25519 signature
    pub sig_verify_cost_ed25519: u64,
    /// Cost to verify secp255k1 signature
    pub sig_verify_cost_secp256k1: u64,
}

/// BaseAccount defines a base account type. It contains all the necessary fields
/// for basic account functionality. Any custom account type should extend this
/// type for additional functionality (e.g. vesting).
#[derive(Debug)]
pub struct BaseAccount {
    /// hex encoded hash address of the account
    pub address: String,
    /// Public key associated with the account
    pub pub_key: PublicKey,
    /// TODO:
    pub account_number: u64,
    /// TODO:
    pub sequence: u64,
}
