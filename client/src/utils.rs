use std::pin::Pin;
use std::sync::Arc;

use celestia_grpc::{DocSigner, SignDoc};
use k256::ecdsa::Signature;
use k256::ecdsa::signature::Error as SignatureError;

use crate::{Error, Result};

pub(crate) fn height_i64(height: u64) -> Result<i64> {
    height.try_into().map_err(|_| Error::InvalidHeight(height))
}

/// Workaround for dispatching `DocSigner`
pub(crate) struct DispatchedDocSigner(Box<dyn DispatchedDocSignerTrait>);

impl DispatchedDocSigner {
    pub(crate) fn new<S>(signer: S) -> DispatchedDocSigner
    where
        S: DispatchedDocSignerTrait + 'static,
    {
        DispatchedDocSigner(Box::new(signer))
    }
}

impl DocSigner for DispatchedDocSigner {
    async fn try_sign(&self, doc: SignDoc) -> Result<Signature, SignatureError> {
        self.0.try_sign(doc).await
    }
}

pub(crate) trait DispatchedDocSignerTrait {
    fn try_sign<'a>(
        &'a self,
        doc: SignDoc,
    ) -> Pin<Box<dyn Future<Output = Result<Signature, SignatureError>> + 'a>>;
}

impl<T> DispatchedDocSignerTrait for T
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
