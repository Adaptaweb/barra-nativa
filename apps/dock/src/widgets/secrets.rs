//! Secrets (tokens, private calendar links) live in the Windows Credential
//! Manager, never in config.json: a generic credential named `Barra/<key>`,
//! the same ones the Tauri Barra saved, so nothing has to be entered again.

use windows::{
    Win32::Security::Credentials::{CRED_TYPE_GENERIC, CREDENTIALW, CredFree, CredReadW},
    core::{HSTRING, PCWSTR},
};

pub fn read(key: &str) -> Option<String> {
    let name = HSTRING::from(format!("Barra/{key}"));
    // SAFETY: CredReadW fills `credential` on success; it is read and then
    // freed with CredFree exactly once.
    unsafe {
        let mut credential: *mut CREDENTIALW = std::ptr::null_mut();
        CredReadW(PCWSTR(name.as_ptr()), CRED_TYPE_GENERIC, None, &mut credential).ok()?;
        let found = &*credential;
        let bytes =
            std::slice::from_raw_parts(found.CredentialBlob, found.CredentialBlobSize as usize);
        let value = String::from_utf8(bytes.to_vec()).ok();
        CredFree(credential.cast());
        value.filter(|v| !v.trim().is_empty())
    }
}
