use cosmrs::crypto::PublicKey;

#[derive(Debug)]
pub struct AuthParams {
    pub max_memo_characters: u64,
    pub tx_sig_limit: u64,
    pub tx_size_cost_per_byte: u64,
    pub sig_verify_cost_ed25519: u64,
    pub sig_verify_cost_secp256k1: u64,
}

#[derive(Debug)]
pub struct BaseAccount {
    pub address: String,
    pub pub_key: PublicKey,
    pub account_number: u64,
    pub sequence: u64,
}
