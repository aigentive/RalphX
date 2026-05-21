use async_trait::async_trait;

#[derive(Debug, thiserror::Error)]
pub enum SecretStoreError {
    #[error("secret storage unavailable: {0}")]
    Unavailable(String),
    #[error("secret not found")]
    NotFound,
}

#[async_trait]
pub trait SecretStore: Send + Sync {
    async fn put_secret(&self, key: &str, value: &str) -> Result<(), SecretStoreError>;
    async fn get_secret(&self, key: &str) -> Result<Option<String>, SecretStoreError>;
    async fn delete_secret(&self, key: &str) -> Result<(), SecretStoreError>;
}
