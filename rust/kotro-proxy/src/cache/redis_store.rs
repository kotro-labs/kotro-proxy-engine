use std::time::Duration;
use redis::AsyncCommands;
use std::sync::Arc;
use tokio::runtime::Handle;
use tokio::sync::Mutex;

use crate::cache::entry::Entry;
use crate::cache::store::{StoreError, StoreOptions};

#[derive(Clone)]
pub struct RedisStore {
    client: redis::Client,
    ttl: Duration,
    compress: bool,
}

impl RedisStore {
    pub fn new(url: &str, opts: StoreOptions) -> Result<Self, StoreError> {
        let client = redis::Client::open(url).map_err(|e| StoreError::Redis(e.to_string()))?;
        Ok(Self {
            client,
            ttl: opts.ttl,
            compress: opts.enable_compression,
        })
    }

    pub fn get(&self, key: &str) -> Result<Option<Entry>, StoreError> {
        let client = self.client.clone();
        let key_str = key.to_string();
        
        // This is called from an async context, but `Store::get` is currently sync in Kotro.
        // We use tokio::task::block_in_place to execute the redis async call if needed,
        // or just use a connection manager. Since Kotro Store API is sync, we use the blocking client.
        
        let mut con = client.get_connection().map_err(|e| StoreError::Redis(e.to_string()))?;
        
        let payload: Option<Vec<u8>> = redis::cmd("GET")
            .arg(&key_str)
            .query(&mut con)
            .map_err(|e| StoreError::Redis(e.to_string()))?;
            
        let Some(payload) = payload else {
            return Ok(None);
        };
        
        // We store the raw compressed payload or JSON directly depending on encoding,
        // but for Redis, storing JSON + TTL natively in Redis is best. 
        // We will decode standard payload here using Kotro encoding for simplicity.
        
        let now_nano = crate::cache::encoding::expires_at_nano(Duration::ZERO); // unused for decode
        let (decoded, expired) = crate::cache::encoding::decode_stored_value(&payload, now_nano);
        
        if expired {
            let _ = redis::cmd("DEL").arg(&key_str).query::<()>(&mut con);
            return Ok(None);
        }
        
        let Some(decoded) = decoded else {
            return Ok(None);
        };
        
        let entry: Entry = serde_json::from_slice(&decoded)?;
        Ok(Some(entry))
    }

    pub fn put(&self, entry: Entry) -> Result<(), StoreError> {
        let payload = serde_json::to_vec(&entry)?;
        let stored = crate::cache::encoding::encode_stored_value(crate::cache::encoding::expires_at_nano(self.ttl), &payload, self.compress);
        
        let mut con = self.client.get_connection().map_err(|e| StoreError::Redis(e.to_string()))?;
        
        if self.ttl.is_zero() {
            redis::cmd("SET")
                .arg(entry.key.as_str())
                .arg(stored.as_slice())
                .query::<()>(&mut con)
                .map_err(|e| StoreError::Redis(e.to_string()))?;
        } else {
            redis::cmd("SETEX")
                .arg(entry.key.as_str())
                .arg(self.ttl.as_secs())
                .arg(stored.as_slice())
                .query::<()>(&mut con)
                .map_err(|e| StoreError::Redis(e.to_string()))?;
        }
        
        Ok(())
    }

    pub fn delete(&self, key: &str) -> Result<(), StoreError> {
        let mut con = self.client.get_connection().map_err(|e| StoreError::Redis(e.to_string()))?;
        redis::cmd("DEL")
            .arg(key)
            .query::<()>(&mut con)
            .map_err(|e| StoreError::Redis(e.to_string()))?;
        Ok(())
    }

    pub fn count(&self) -> Result<usize, StoreError> {
        let mut con = self.client.get_connection().map_err(|e| StoreError::Redis(e.to_string()))?;
        let count: usize = redis::cmd("DBSIZE").query(&mut con).unwrap_or(0);
        Ok(count)
    }
}
