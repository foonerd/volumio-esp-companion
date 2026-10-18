//! The HAL's settings store over NVS.
//!
//! One namespace in the default NVS partition, string values under the
//! runtime's keys. NVS names are at most fifteen bytes; the runtime's keys
//! are spelled to fit, and a longer key is refused here rather than
//! truncated, so a typo cannot silently become a different setting.

use companion_hal::ConfigStore;
use esp_idf_svc::nvs::{EspDefaultNvs, EspDefaultNvsPartition, EspNvs};
use esp_idf_svc::sys::EspError;

/// The longest name NVS accepts, without the terminating NUL.
pub const MAX_KEY_LEN: usize = 15;

/// Why a store operation failed.
#[derive(Debug)]
pub enum NvsError {
    /// The key is longer than NVS allows.
    KeyTooLong(String),
    /// NVS refused.
    Esp(EspError),
}

impl std::fmt::Display for NvsError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::KeyTooLong(k) => write!(f, "key {k:?} is longer than {MAX_KEY_LEN} bytes"),
            Self::Esp(e) => write!(f, "nvs: {e}"),
        }
    }
}

impl From<EspError> for NvsError {
    fn from(e: EspError) -> Self {
        Self::Esp(e)
    }
}

/// A namespace in the default NVS partition.
pub struct NvsStore {
    nvs: EspDefaultNvs,
}

impl NvsStore {
    /// Open `namespace` read-write in the default partition.
    pub fn open(partition: EspDefaultNvsPartition, namespace: &str) -> Result<Self, EspError> {
        Ok(Self {
            nvs: EspNvs::new(partition, namespace, true)?,
        })
    }
}

fn check(key: &str) -> Result<(), NvsError> {
    if key.len() > MAX_KEY_LEN {
        return Err(NvsError::KeyTooLong(key.into()));
    }
    Ok(())
}

impl ConfigStore for NvsStore {
    type Error = NvsError;

    fn get(&self, key: &str) -> Result<Option<String>, Self::Error> {
        check(key)?;
        let Some(len) = self.nvs.str_len(key)? else {
            return Ok(None);
        };
        // The length NVS reports includes the NUL.
        let mut buf = vec![0u8; len.max(1)];
        Ok(self
            .nvs
            .get_str(key, &mut buf)?
            .map(|s| s.trim_end_matches('\0').to_string()))
    }

    fn set(&mut self, key: &str, value: &str) -> Result<(), Self::Error> {
        check(key)?;
        self.nvs.set_str(key, value)?;
        Ok(())
    }

    fn remove(&mut self, key: &str) -> Result<(), Self::Error> {
        check(key)?;
        self.nvs.remove(key)?;
        Ok(())
    }
}
