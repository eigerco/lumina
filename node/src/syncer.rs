use std::sync::Arc;

use tokio::sync::RwLock;

use crate::store::Store;

#[allow(unused)]
pub struct Syncer {
    store: Arc<RwLock<Store>>,
}

impl Syncer {
    pub fn new(store: Arc<RwLock<Store>>) -> Self {
        Syncer { store }
    }
}
