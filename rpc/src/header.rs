use celestia_types::ExtendedHeader;
use jsonrpsee::proc_macros::rpc;

#[rpc(client)]
pub trait Header {
    #[method(name = "header.LocalHead")]
    async fn header_local_head(&self) -> Result<ExtendedHeader, Error>;

    #[method(name = "header.GetByHeight")]
    async fn header_get_by_height(&self, height: u64) -> Result<ExtendedHeader, Error>;

    #[method(name = "header.WaitForHeight")]
    async fn header_wait_for_height(&self, height: u64) -> Result<ExtendedHeader, Error>;
}
