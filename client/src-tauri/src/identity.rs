use serde::Serialize;

#[derive(Clone, Debug, PartialEq, Eq, Serialize)]
pub struct WindowsIdentity {
    pub machine_id: String,
    pub account_sid: String,
    pub username: String,
}

pub trait IdentitySource {
    fn identity(&self) -> Result<WindowsIdentity, String>;
}

#[derive(Default)]
pub struct WindowsIdentitySource;

#[cfg(windows)]
impl IdentitySource for WindowsIdentitySource {
    fn identity(&self) -> Result<WindowsIdentity, String> {
        Ok(WindowsIdentity {
            machine_id: machine_id()?,
            account_sid: current_account_sid()?,
            username: std::env::var("USERNAME").unwrap_or_default(),
        })
    }
}

#[cfg(not(windows))]
impl IdentitySource for WindowsIdentitySource {
    fn identity(&self) -> Result<WindowsIdentity, String> {
        Err("Provider Relay desktop client requires Windows identity APIs".into())
    }
}

#[cfg(windows)]
fn machine_id() -> Result<String, String> {
    use windows::{
        core::PCWSTR,
        Win32::{
            Foundation::WIN32_ERROR,
            System::Registry::{RegGetValueW, HKEY_LOCAL_MACHINE, RRF_RT_REG_SZ},
        },
    };

    let sub_key = wide("SOFTWARE\\Microsoft\\Cryptography");
    let value_name = wide("MachineGuid");
    let mut byte_count = 0;

    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR::from_raw(sub_key.as_ptr()),
            PCWSTR::from_raw(value_name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            None,
            Some(&mut byte_count),
        )
    };
    if status != WIN32_ERROR(0) {
        return Err(format!(
            "unable to read Windows machine identifier: {}",
            status.0
        ));
    }

    let mut value = vec![0u16; byte_count as usize / std::mem::size_of::<u16>()];
    let status = unsafe {
        RegGetValueW(
            HKEY_LOCAL_MACHINE,
            PCWSTR::from_raw(sub_key.as_ptr()),
            PCWSTR::from_raw(value_name.as_ptr()),
            RRF_RT_REG_SZ,
            None,
            Some(value.as_mut_ptr().cast()),
            Some(&mut byte_count),
        )
    };
    if status != WIN32_ERROR(0) {
        return Err(format!(
            "unable to read Windows machine identifier: {}",
            status.0
        ));
    }

    string_from_wide(&value).ok_or_else(|| "Windows machine identifier is not valid UTF-16".into())
}

#[cfg(windows)]
fn current_account_sid() -> Result<String, String> {
    use std::ptr;
    use windows::{
        core::PWSTR,
        Win32::{
            Foundation::{CloseHandle, LocalFree, HANDLE, HLOCAL},
            Security::{
                Authorization::ConvertSidToStringSidW, GetTokenInformation, TokenUser, TOKEN_QUERY,
                TOKEN_USER,
            },
            System::Threading::{GetCurrentProcess, OpenProcessToken},
        },
    };

    let mut token = HANDLE::default();
    unsafe {
        OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token)
            .map_err(|error| format!("unable to open current Windows account token: {error}"))?;
    }

    let result = (|| unsafe {
        let mut byte_count = 0;
        let _ = GetTokenInformation(token, TokenUser, None, 0, &mut byte_count);
        if byte_count == 0 {
            return Err("unable to determine current Windows account SID size".into());
        }

        let mut token_user = vec![0u8; byte_count as usize];
        GetTokenInformation(
            token,
            TokenUser,
            Some(token_user.as_mut_ptr().cast()),
            byte_count,
            &mut byte_count,
        )
        .map_err(|error| format!("unable to read current Windows account SID: {error}"))?;

        let token_user = ptr::read_unaligned(token_user.as_ptr().cast::<TOKEN_USER>());
        let mut sid = PWSTR::null();
        ConvertSidToStringSidW(token_user.User.Sid, &mut sid)
            .map_err(|error| format!("unable to format current Windows account SID: {error}"))?;
        let sid_text = sid
            .to_string()
            .map_err(|error| format!("current Windows account SID is not valid UTF-16: {error}"));
        let _ = LocalFree(Some(HLOCAL(sid.0.cast())));
        sid_text
    })();

    unsafe {
        let _ = CloseHandle(token);
    }
    result
}

#[cfg(windows)]
fn wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

#[cfg(windows)]
fn string_from_wide(value: &[u16]) -> Option<String> {
    let end = value.iter().position(|character| *character == 0)?;
    String::from_utf16(&value[..end]).ok()
}
