use std::sync::{Arc, Mutex};

pub const CREDENTIAL_TARGET: &str = "provider-relay/device-credential";

pub trait CredentialStore {
    fn load(&self) -> Result<Option<String>, String>;
    fn save(&self, credential: &str) -> Result<(), String>;
    fn delete(&self) -> Result<(), String>;
}

#[derive(Default)]
pub struct WindowsCredentialStore;

#[cfg(windows)]
impl CredentialStore for WindowsCredentialStore {
    fn load(&self) -> Result<Option<String>, String> {
        use std::slice;
        use windows::{
            core::{HRESULT, PCWSTR},
            Win32::{
                Foundation::ERROR_NOT_FOUND,
                Security::Credentials::{CredFree, CredReadW, CREDENTIALW, CRED_TYPE_GENERIC},
            },
        };

        let target = wide(CREDENTIAL_TARGET);
        let mut credential = std::ptr::null_mut::<CREDENTIALW>();
        let result = unsafe {
            CredReadW(
                PCWSTR::from_raw(target.as_ptr()),
                CRED_TYPE_GENERIC,
                None,
                &mut credential,
            )
        };

        if let Err(error) = result {
            return if error.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) {
                Ok(None)
            } else {
                Err(format!("unable to read Windows device credential: {error}"))
            };
        }

        let secret = unsafe {
            let blob = slice::from_raw_parts(
                (*credential).CredentialBlob,
                (*credential).CredentialBlobSize as usize,
            );
            let secret = String::from_utf8(blob.to_vec())
                .map_err(|_| "Windows device credential is not valid UTF-8".to_owned());
            CredFree(credential.cast());
            secret
        }?;
        Ok(Some(secret))
    }

    fn save(&self, credential: &str) -> Result<(), String> {
        use windows::{
            core::PWSTR,
            Win32::Security::Credentials::{
                CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE, CRED_TYPE_GENERIC,
            },
        };

        let mut target = wide(CREDENTIAL_TARGET);
        let mut username = wide("Provider Relay");
        let mut secret = credential.as_bytes().to_vec();
        let entry = CREDENTIALW {
            Type: CRED_TYPE_GENERIC,
            TargetName: PWSTR::from_raw(target.as_mut_ptr()),
            CredentialBlobSize: secret.len() as u32,
            CredentialBlob: secret.as_mut_ptr(),
            Persist: CRED_PERSIST_LOCAL_MACHINE,
            UserName: PWSTR::from_raw(username.as_mut_ptr()),
            ..Default::default()
        };

        unsafe { CredWriteW(&entry, 0) }
            .map_err(|error| format!("unable to save Windows device credential: {error}"))
    }

    fn delete(&self) -> Result<(), String> {
        use windows::{
            core::{HRESULT, PCWSTR},
            Win32::{
                Foundation::ERROR_NOT_FOUND,
                Security::Credentials::{CredDeleteW, CRED_TYPE_GENERIC},
            },
        };

        let target = wide(CREDENTIAL_TARGET);
        let result =
            unsafe { CredDeleteW(PCWSTR::from_raw(target.as_ptr()), CRED_TYPE_GENERIC, None) };
        match result {
            Ok(()) => Ok(()),
            Err(error) if error.code() == HRESULT::from_win32(ERROR_NOT_FOUND.0) => Ok(()),
            Err(error) => Err(format!(
                "unable to delete Windows device credential: {error}"
            )),
        }
    }
}

#[cfg(not(windows))]
impl CredentialStore for WindowsCredentialStore {
    fn load(&self) -> Result<Option<String>, String> {
        Err("Provider Relay desktop client requires Windows Credential Manager".into())
    }

    fn save(&self, _: &str) -> Result<(), String> {
        Err("Provider Relay desktop client requires Windows Credential Manager".into())
    }

    fn delete(&self) -> Result<(), String> {
        Err("Provider Relay desktop client requires Windows Credential Manager".into())
    }
}

#[derive(Clone, Default)]
pub struct MemoryCredentialStore(Arc<Mutex<Option<String>>>);

impl MemoryCredentialStore {
    pub fn with_secret(credential: &str) -> Self {
        Self(Arc::new(Mutex::new(Some(credential.into()))))
    }
}

impl CredentialStore for MemoryCredentialStore {
    fn load(&self) -> Result<Option<String>, String> {
        self.0
            .lock()
            .map(|credential| credential.clone())
            .map_err(|_| "in-memory credential store lock is poisoned".into())
    }

    fn save(&self, credential: &str) -> Result<(), String> {
        *self
            .0
            .lock()
            .map_err(|_| "in-memory credential store lock is poisoned".to_owned())? =
            Some(credential.into());
        Ok(())
    }

    fn delete(&self) -> Result<(), String> {
        *self
            .0
            .lock()
            .map_err(|_| "in-memory credential store lock is poisoned".to_owned())? = None;
        Ok(())
    }
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}
