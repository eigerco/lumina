use std::fmt::{self, Debug};
use std::future::Future;
use std::pin::Pin;

use crate::tx::{DocSigner, SignDoc, Signature, SignatureError};
use crate::{Error, Result};

pub(crate) fn height_i64(height: u64) -> Result<i64> {
    height.try_into().map_err(|_| Error::InvalidHeight(height))
}

/// Workaround for dispatching `DocSigner`
pub(crate) struct DispatchedDocSigner(Box<dyn DispatchedDocSignerTrait>);

impl DispatchedDocSigner {
    pub(crate) fn new<S>(signer: S) -> DispatchedDocSigner
    where
        S: DispatchedDocSignerTrait,
    {
        DispatchedDocSigner(Box::new(signer))
    }
}

impl DocSigner for DispatchedDocSigner {
    async fn try_sign(&self, doc: SignDoc) -> Result<Signature, SignatureError> {
        self.0.try_sign(doc).await
    }
}

pub(crate) trait DispatchedDocSignerTrait: Sync + Send + 'static {
    fn try_sign<'a>(
        &'a self,
        doc: SignDoc,
    ) -> Pin<Box<dyn Future<Output = Result<Signature, SignatureError>> + Send + 'a>>;
}

impl<T> DispatchedDocSignerTrait for T
where
    T: DocSigner + Sync + Send + 'static,
{
    fn try_sign<'a>(
        &'a self,
        doc: SignDoc,
    ) -> Pin<Box<dyn Future<Output = Result<Signature, SignatureError>> + Send + 'a>> {
        Box::pin(DocSigner::try_sign(self, doc))
    }
}

impl Debug for DispatchedDocSigner {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("DispatchedDocSigner { .. }")
    }
}
