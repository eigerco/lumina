use std::pin::Pin;
use std::sync::Arc;

use celestia_grpc::{DocSigner, SignDoc};
use k256::ecdsa::signature::Error as SignatureError;
use k256::ecdsa::Signature;

use crate::{Error, Result};

pub(crate) fn height_i64(height: u64) -> Result<i64> {
    height.try_into().map_err(|_| Error::InvalidHeight(height))
}

pub(crate) trait DispatchedDocSigner {
    fn try_sign<'a>(
        &'a self,
        doc: SignDoc,
    ) -> Pin<Box<dyn Future<Output = Result<Signature, SignatureError>> + 'a>>;
}

/*
impl DispatchedDocSigner for Arc<dyn DispatchedDocSigner> {
    fn try_sign<'a>(
        &'a self,
        doc: SignDoc,
    ) -> Pin<Box<dyn Future<Output = Result<Signature, SignatureError>> + 'a>> {
        (&*self).try_sign(doc)
    }
}
*/

impl<T> DispatchedDocSigner for T
where
    T: DocSigner,
{
    fn try_sign<'a>(
        &'a self,
        doc: SignDoc,
    ) -> Pin<Box<dyn Future<Output = Result<Signature, SignatureError>> + 'a>> {
        Box::pin(DocSigner::try_sign(self, doc))
    }
}
