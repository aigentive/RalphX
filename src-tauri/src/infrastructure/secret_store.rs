#[cfg(target_os = "macos")]
use security_framework::base::Error as KeychainError;

#[cfg(target_os = "macos")]
use crate::domain::services::{SecretStore, SecretStoreError};

#[cfg(target_os = "macos")]
const RALPHX_KEYCHAIN_SERVICE: &str = "com.ralphx.app";

#[cfg(target_os = "macos")]
pub struct MacosKeychainSecretStore {
    service: String,
}

#[cfg(target_os = "macos")]
impl Default for MacosKeychainSecretStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(target_os = "macos")]
impl MacosKeychainSecretStore {
    pub fn new() -> Self {
        Self {
            service: RALPHX_KEYCHAIN_SERVICE.to_string(),
        }
    }
}

#[cfg(target_os = "macos")]
fn keychain_error(error: KeychainError) -> SecretStoreError {
    SecretStoreError::Unavailable(error.to_string())
}

#[cfg(target_os = "macos")]
#[async_trait::async_trait]
impl SecretStore for MacosKeychainSecretStore {
    async fn put_secret(&self, key: &str, value: &str) -> Result<(), SecretStoreError> {
        let service = self.service.clone();
        let key = key.to_string();
        let value = value.to_string();
        tokio::task::spawn_blocking(move || {
            security_framework::passwords::set_generic_password(&service, &key, value.as_bytes())
                .map_err(keychain_error)
        })
        .await
        .map_err(|error| SecretStoreError::Unavailable(error.to_string()))?
    }

    async fn get_secret(&self, key: &str) -> Result<Option<String>, SecretStoreError> {
        let service = self.service.clone();
        let key = key.to_string();
        tokio::task::spawn_blocking(move || {
            match security_framework::passwords::get_generic_password(&service, &key) {
                Ok(bytes) => String::from_utf8(bytes)
                    .map(Some)
                    .map_err(|error| SecretStoreError::Unavailable(error.to_string())),
                Err(error) if error.code() == -25300 => Ok(None),
                Err(error) => Err(keychain_error(error)),
            }
        })
        .await
        .map_err(|error| SecretStoreError::Unavailable(error.to_string()))?
    }

    async fn delete_secret(&self, key: &str) -> Result<(), SecretStoreError> {
        let service = self.service.clone();
        let key = key.to_string();
        tokio::task::spawn_blocking(move || {
            match security_framework::passwords::delete_generic_password(&service, &key) {
                Ok(()) => Ok(()),
                Err(error) if error.code() == -25300 => Ok(()),
                Err(error) => Err(keychain_error(error)),
            }
        })
        .await
        .map_err(|error| SecretStoreError::Unavailable(error.to_string()))?
    }
}

#[cfg(not(target_os = "macos"))]
pub use crate::infrastructure::memory::MemorySecretStore as MacosKeychainSecretStore;
