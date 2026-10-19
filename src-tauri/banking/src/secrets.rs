use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use zeroize::{Zeroize, ZeroizeOnDrop, Zeroizing};

#[derive(Serialize, Deserialize, Zeroize, ZeroizeOnDrop)]
pub struct Credentials {
    pub application_id: String,
    pub private_key: String,
}

pub trait SecretStore: Send + Sync {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>>;
    fn put(&self, name: &str, value: &[u8]) -> Result<()>;
}

pub struct SystemSecrets;

#[cfg(target_os = "macos")]
impl SecretStore for SystemSecrets {
    fn get(&self, name: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        match security_framework::passwords::get_generic_password(
            "com.racker.zero.banking.v1",
            name,
        ) {
            Ok(bytes) => Ok(Some(Zeroizing::new(bytes))),
            Err(e) if e.code() == -25300 => Ok(None),
            Err(_) => Err(Error::Keychain),
        }
    }

    fn put(&self, name: &str, value: &[u8]) -> Result<()> {
        security_framework::passwords::set_generic_password(
            "com.racker.zero.banking.v1",
            name,
            value,
        )
        .map_err(|_| Error::Keychain)
    }
}

#[cfg(not(target_os = "macos"))]
impl SecretStore for SystemSecrets {
    fn get(&self, _: &str) -> Result<Option<Zeroizing<Vec<u8>>>> {
        Err(Error::Unsupported)
    }
    fn put(&self, _: &str, _: &[u8]) -> Result<()> {
        Err(Error::Unsupported)
    }
}
