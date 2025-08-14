use idb::{Database, DatabaseEvent, Factory, KeyRange, ObjectStoreParams, Query, TransactionMode};
use js_sys::Uint8Array;
use libp2p_identity::Keypair;
use send_wrapper::SendWrapper;
use serde_wasm_bindgen::{from_value, to_value};
use tracing::{info, trace, warn};
use wasm_bindgen::{JsCast, JsValue};

use crate::lock::{Error as LockError, NamedLock, NamedLockGuard};

const DB_NAME: &str = "identity_registry";
const DB_VERSION: u32 = 1;

const KEY_STORE_NAME: &str = "🗝️";

pub struct KeyRegistry {
    db: SendWrapper<Database>,
}

#[derive(thiserror::Error, Debug)]
pub enum Error {
    /// IndexedDb contains invalid data
    #[error("could not deserialise keypair data from db")]
    InvalidKeypairData,

    /// IndexedDb error coming from idb
    #[error("idb error: {0}")]
    IdbError(#[from] idb::Error),

    #[error("key already locked")]
    KeyLocked,

    #[error("could not load LockManager")]
    LockManagerUnavailable(crate::error::Error),
}

pub type Result<T, E = Error> = std::result::Result<T, E>;

impl KeyRegistry {
    pub async fn new() -> Result<Self> {
        let factory = Factory::new()?;

        let mut open_request = factory.open(DB_NAME, Some(DB_VERSION))?;

        open_request.on_upgrade_needed(|event| {
            let database = event.database().expect("database on event");

            let mut store_params = ObjectStoreParams::new();
            store_params.auto_increment(true);

            let _store = database
                .create_object_store(KEY_STORE_NAME, store_params)
                .expect("create ok");
        });

        let db = open_request.await?;

        Ok(Self {
            db: SendWrapper::new(db),
        })
    }

    async fn get_first_unused_key(&self) -> Result<Option<(Keypair, NamedLockGuard)>> {
        const KEY_BATCH_SIZE: usize = 512;
        let tx = self
            .db
            .transaction(&[KEY_STORE_NAME], TransactionMode::ReadWrite)?;

        let store = tx.object_store(KEY_STORE_NAME)?;

        let mut offset = 0;

        loop {
            let key_range = KeyRange::bound(
                &to_value(&offset).expect("successful conversion"),
                &to_value(&(offset + KEY_BATCH_SIZE)).expect("successful conversion"),
                Some(false),
                Some(true),
            )
            .expect("valid key range");

            let keys = store
                .get_all(Some(Query::KeyRange(key_range)), None)?
                .await?;

            if keys.is_empty() {
                return Ok(None);
            }

            for key in keys {
                match decode_keypair(key) {
                    Ok(keypair) => match try_lock_key(&keypair).await {
                        Ok(guard) => return Ok(Some((keypair, guard))),
                        Err(Error::KeyLocked) => info!("Key already used"), // TODO: add peerid log?
                        Err(e) => warn!("Unexpected error acquiring the lock: {e}"),
                    },
                    Err(e) => warn!("Could not deserialize key: {e}"),
                };
            }

            offset += KEY_BATCH_SIZE;
        }
    }

    pub async fn get_key(&self) -> Result<(Keypair, NamedLockGuard)> {
        match self.get_first_unused_key().await {
            Ok(k) => {
                if let Some(key_and_lock) = k {
                    return Ok(key_and_lock);
                }
            }
            Err(e) => {
                info!("error loading keys from IndexedDb: {e}");
            }
        }

        info!("Persisting new keypair to indexeddb");

        let keypair = Keypair::generate_ed25519();
        let guard = try_lock_key(&keypair)
            .await
            .expect("newly generated key to be unique");

        let tx = self
            .db
            .transaction(&[KEY_STORE_NAME], TransactionMode::ReadWrite)?;

        let store = tx.object_store(KEY_STORE_NAME)?;
        store.add(&encode_keypair(&keypair), None)?.await?;

        tx.commit()?;

        Ok((keypair, guard))
    }

    pub async fn get_ephemeral_key() -> (Keypair, NamedLockGuard) {
        let keypair = Keypair::generate_ed25519();
        let guard = try_lock_key(&keypair)
            .await
            .expect("newly generated key to be unique");

        (keypair, guard)
    }

    pub async fn lock_key(keypair: &Keypair) -> Result<NamedLockGuard> {
        try_lock_key(keypair).await
    }
}

fn encode_keypair(keypair: &Keypair) -> JsValue {
    let bytes = keypair.to_protobuf_encoding().expect("valid key encoding");
    let array = Uint8Array::from(bytes.as_ref());
    array.into()
}

fn decode_keypair(value: JsValue) -> Result<Keypair> {
    let array = value
        .dyn_into::<Uint8Array>()
        .map_err(|_| Error::InvalidKeypairData)?;
    Keypair::from_protobuf_encoding(&array.to_vec()).map_err(|_| Error::InvalidKeypairData)
}

async fn try_lock_key(keypair: &Keypair) -> Result<NamedLockGuard> {
    let peer_id = keypair.public().to_peer_id().to_base58();

    let lock = NamedLock::create(peer_id)?;

    Ok(lock.try_lock().await?)
}

impl From<LockError> for Error {
    fn from(value: LockError) -> Self {
        match value {
            LockError::WouldBlock => Error::KeyLocked,
            LockError::LockManagerUnavailable(e) => Error::LockManagerUnavailable(e.into()),
        }
    }
}
