use std::collections::HashMap;
use std::sync::Arc;

use async_trait::async_trait;
use tokio::sync::RwLock;

use crate::domain::services::{SecretStore, SecretStoreError};

pub struct MemorySecretStore {
    secrets: Arc<RwLock<HashMap<String, String>>>,
}

impl Default for MemorySecretStore {
    fn default() -> Self {
        Self::new()
    }
}

impl MemorySecretStore {
    pub fn new() -> Self {
        Self {
            secrets: Arc::new(RwLock::new(HashMap::new())),
        }
    }
}

#[async_trait]
impl SecretStore for MemorySecretStore {
    async fn put_secret(&self, key: &str, value: &str) -> Result<(), SecretStoreError> {
        self.secrets
            .write()
            .await
            .insert(key.to_string(), value.to_string());
        Ok(())
    }

    async fn get_secret(&self, key: &str) -> Result<Option<String>, SecretStoreError> {
        Ok(self.secrets.read().await.get(key).cloned())
    }

    async fn delete_secret(&self, key: &str) -> Result<(), SecretStoreError> {
        self.secrets.write().await.remove(key);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn stores_reads_and_deletes_secrets() {
        let store = MemorySecretStore::default();

        assert_eq!(store.get_secret("atlassian").await.unwrap(), None);

        store.put_secret("atlassian", "token-value").await.unwrap();
        assert_eq!(
            store.get_secret("atlassian").await.unwrap().as_deref(),
            Some("token-value")
        );

        store.delete_secret("atlassian").await.unwrap();
        assert_eq!(store.get_secret("atlassian").await.unwrap(), None);
    }
}
