use tendermint::block::Height;
use tendermint::signature::SIGNATURE_LENGTH;
use tendermint::Hash;

use crate::consts;

pub type ValidationResult<T> = std::result::Result<T, ValidationError>;

#[derive(Debug, thiserror::Error)]
pub enum ValidationError {
    #[error(
        "Block protocol is incorrect: {0}, expected: {}",
        consts::version::BLOCK_PROTOCOL
    )]
    IncorrectBlockProtocol(u64),

    #[error(
        "Chain Id is too long: {0}, max length: {}",
        consts::genesis::MAX_CHAIN_ID_LEN
    )]
    ChainIdTooLong(String),

    #[error("Height is zero")]
    ZeroHeight,

    #[error("Last block id is missing")]
    MissingLastBlockId,

    #[error("Commit for the nil block")]
    CommitForNilBlock,

    #[error("No signatures in commit")]
    NoSignaturesInCommit,

    #[error("No signature in commit sig")]
    NoSignatureInCommitSig,

    #[error(
        "Signature is of invalid length: {0:?}, expected len: {}",
        SIGNATURE_LENGTH
    )]
    InvalidLengthSignature(Vec<u8>),

    #[error("Columns and rows are of different lengths: rows {0}, cols {1}")]
    NotASquare(usize, usize),

    #[error(
        "Columns and rows are to little: border len {0}, min {}",
        consts::data_availability_header::MIN_EXTENDED_SQUARE_WIDTH
    )]
    TooLittleSquare(usize),

    #[error(
        "Columns and rows are to big: border len {0}, min {}",
        consts::data_availability_header::MAX_EXTENDED_SQUARE_WIDTH
    )]
    TooBigSquare(usize),

    #[error(
        "Validator hash of header different than validator set hash: header {0}, val set {1}, height: {2}"
    )]
    HeaderAndValSetHashMismatch(Hash, Hash, Height),

    #[error("DAH hash of header different than DAH hash: header {0}, dah {1}, height: {2}")]
    HeaderAndDahHashMismatch(Hash, Hash, Height),

    #[error("Height of header different than commit height: header {0}, commit {1}")]
    HeaderAndCommitHeightMismatch(Height, Height),

    #[error(
        "Block hash of header different than commit's block hash: header {0}, block {1}, height: {2}"
    )]
    HeaderAndCommitBlockHashMismatch(Hash, Hash, Height),

    #[error("Validators set is empty")]
    ValidatorsSetEmpty,

    #[error("Validators set proposer is absent")]
    ValidatorsSetProposerMissing,

    #[error("Commit should have equal validators and signatures amounts: vals {0}, sigs {1}")]
    ValidatorsAndSignaturesMismatch(usize, usize),
}

pub trait ValidateBasic {
    fn validate_basic(&self) -> ValidationResult<()>;
}
