// Issue #1, Layer 1: local evidence for programs Mganga does not recognize.
// Read-only and fully offline. Everything here is what Windows already knows
// about an executable but the scanner did not surface before:
//   - its version-resource strings (CompanyName, FileDescription, ProductName)
//   - who Authenticode-signed it, and whether that signature verifies locally
//   - where on disk it lives
//
// The rule from the rest of the codebase holds: if a signal is absent, we say
// nothing about it. None means "no record", never "bad".

use serde::Serialize;
use std::path::Path;

#[derive(Serialize, Clone, Default)]
pub struct FileEvidence {
    /// CompanyName from the version resource. This is the existing "publisher".
    pub company: Option<String>,
    /// FileDescription: a mystery exe often describes itself in plain words.
    pub description: Option<String>,
    /// ProductName from the version resource.
    pub product: Option<String>,
    /// Authenticode signer subject name, if the file carries a signature.
    pub signer: Option<String>,
    /// Whether that signature verifies against local trust roots, checked
    /// without any network call. None means unsigned or could not be checked.
    pub signature_valid: Option<bool>,
    /// True only when an existing file was actually inspected for a signature.
    /// Lets the UI tell "checked, unsigned" apart from "could not check" (e.g.
    /// a Startup-folder shortcut whose target we did not resolve), so Mganga
    /// never claims "unsigned" about a file it never read.
    pub checked_signature: bool,
    /// A plain phrase for a noteworthy install location, e.g. "in Program
    /// Files" or "in a temporary folder". None when the location says nothing.
    pub location: Option<String>,
}

/// The cheap evidence: version-resource strings and the install location.
/// Safe to gather for every entry on a scan.
pub fn cheap(exe_path: &str) -> FileEvidence {
    if exe_path.is_empty() || !Path::new(exe_path).exists() {
        return FileEvidence::default();
    }
    let v = version_strings(exe_path);
    FileEvidence {
        company: v.company,
        description: v.description,
        product: v.product,
        signer: None,
        signature_valid: None,
        checked_signature: false,
        location: classify_location(exe_path),
    }
}

/// The expensive evidence: the Authenticode signer and whether it verifies.
/// Local verification only (no revocation network calls), so it stays offline
/// and fast, but we still reserve it for entries Mganga could not place.
/// Returns (signer name, signature valid). Both None when the file is unsigned.
pub fn authenticode(exe_path: &str) -> (Option<String>, Option<bool>) {
    if exe_path.is_empty() || !Path::new(exe_path).exists() {
        return (None, None);
    }
    match signer_name(exe_path) {
        Some(name) => (Some(name), Some(verify_trust(exe_path))),
        None => (None, None),
    }
}

// ------------------------------------------------------------ version strings

#[derive(Default)]
struct VersionStrings {
    company: Option<String>,
    description: Option<String>,
    product: Option<String>,
}

fn version_strings(exe_path: &str) -> VersionStrings {
    use windows::core::HSTRING;
    use windows::Win32::Storage::FileSystem::{
        GetFileVersionInfoSizeW, GetFileVersionInfoW, VerQueryValueW,
    };

    let mut out = VersionStrings::default();
    unsafe {
        let h = HSTRING::from(exe_path);
        let size = GetFileVersionInfoSizeW(&h, None);
        if size == 0 {
            return out;
        }
        let mut data = vec![0u8; size as usize];
        if GetFileVersionInfoW(&h, None, size, data.as_mut_ptr() as *mut _).is_err() {
            return out;
        }

        // The version resource is keyed by the file's own language/codepage.
        let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
        let mut len = 0u32;
        if !VerQueryValueW(
            data.as_ptr() as *const _,
            &HSTRING::from(r"\VarFileInfo\Translation"),
            &mut ptr,
            &mut len,
        )
        .as_bool()
            || len < 4
        {
            return out;
        }
        let lang = *(ptr as *const u16);
        let codepage = *(ptr as *const u16).add(1);

        let query = |field: &str| -> Option<String> {
            let q = format!(r"\StringFileInfo\{lang:04x}{codepage:04x}\{field}");
            let mut sptr: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut slen = 0u32;
            if !VerQueryValueW(
                data.as_ptr() as *const _,
                &HSTRING::from(q.as_str()),
                &mut sptr,
                &mut slen,
            )
            .as_bool()
                || slen == 0
            {
                return None;
            }
            let wide = std::slice::from_raw_parts(sptr as *const u16, slen as usize);
            let s = String::from_utf16_lossy(wide)
                .trim_end_matches('\0')
                .trim()
                .to_string();
            (!s.is_empty()).then_some(s)
        };

        out.company = query("CompanyName");
        out.description = query("FileDescription");
        out.product = query("ProductName");
    }
    out
}

// -------------------------------------------------------------- authenticode

/// Pull the signer's display name out of the file's embedded PKCS#7 signature.
/// Returns None for unsigned files or on any failure (no claim).
fn signer_name(exe_path: &str) -> Option<String> {
    use windows::core::HSTRING;
    use windows::Win32::Security::Cryptography::{
        CertCloseStore, CertFindCertificateInStore, CertFreeCertificateContext,
        CertGetNameStringW, CryptMsgClose, CryptMsgGetParam, CryptQueryObject, CERT_CONTEXT,
        CERT_FIND_SUBJECT_CERT, CERT_NAME_SIMPLE_DISPLAY_TYPE, CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
        CERT_QUERY_ENCODING_TYPE, CERT_QUERY_FORMAT_FLAG_BINARY, CERT_QUERY_OBJECT_FILE,
        CMSG_SIGNER_CERT_INFO_PARAM, HCERTSTORE, PKCS_7_ASN_ENCODING, X509_ASN_ENCODING,
    };

    unsafe {
        let h = HSTRING::from(exe_path);
        let mut hstore = HCERTSTORE::default();
        // In windows 0.62 the message handle is a bare pointer, not a newtype.
        let mut hmsg: *mut core::ffi::c_void = std::ptr::null_mut();

        if CryptQueryObject(
            CERT_QUERY_OBJECT_FILE,
            h.as_ptr() as *const core::ffi::c_void,
            CERT_QUERY_CONTENT_FLAG_PKCS7_SIGNED_EMBED,
            CERT_QUERY_FORMAT_FLAG_BINARY,
            0,
            None,
            None,
            None,
            Some(&mut hstore),
            Some(&mut hmsg),
            None,
        )
        .is_err()
        {
            return None;
        }

        let result = (|| {
            // The signer's CERT_INFO (issuer + serial) identifies which cert in
            // the store actually signed the file.
            let mut cb = 0u32;
            if CryptMsgGetParam(hmsg, CMSG_SIGNER_CERT_INFO_PARAM, 0, None, &mut cb).is_err()
                || cb == 0
            {
                return None;
            }
            let mut info = vec![0u8; cb as usize];
            if CryptMsgGetParam(
                hmsg,
                CMSG_SIGNER_CERT_INFO_PARAM,
                0,
                Some(info.as_mut_ptr() as *mut core::ffi::c_void),
                &mut cb,
            )
            .is_err()
            {
                return None;
            }

            let encoding =
                CERT_QUERY_ENCODING_TYPE(X509_ASN_ENCODING.0 | PKCS_7_ASN_ENCODING.0);
            let ctx = CertFindCertificateInStore(
                hstore,
                encoding,
                0,
                CERT_FIND_SUBJECT_CERT,
                Some(info.as_ptr() as *const core::ffi::c_void),
                None,
            );
            if ctx.is_null() {
                return None;
            }

            // CertGetNameString with a null buffer returns the size (incl. NUL).
            let len = CertGetNameStringW(ctx, CERT_NAME_SIMPLE_DISPLAY_TYPE, 0, None, None);
            let name = if len > 1 {
                let mut buf = vec![0u16; len as usize];
                CertGetNameStringW(
                    ctx,
                    CERT_NAME_SIMPLE_DISPLAY_TYPE,
                    0,
                    None,
                    Some(&mut buf),
                );
                let s = String::from_utf16_lossy(&buf)
                    .trim_end_matches('\0')
                    .trim()
                    .to_string();
                (!s.is_empty()).then_some(s)
            } else {
                None
            };

            let _ = CertFreeCertificateContext(Some(ctx as *const CERT_CONTEXT));
            name
        })();

        let _ = CertCloseStore(Some(hstore), 0);
        let _ = CryptMsgClose(Some(hmsg as *const core::ffi::c_void));
        result
    }
}

/// Ask WinVerifyTrust whether the embedded signature is trusted. Revocation
/// checks are disabled so this never touches the network.
fn verify_trust(exe_path: &str) -> bool {
    use windows::core::{HSTRING, PCWSTR};
    use windows::Win32::Foundation::{HANDLE, HWND};
    use windows::Win32::Security::WinTrust::{
        WinVerifyTrust, WINTRUST_ACTION_GENERIC_VERIFY_V2, WINTRUST_DATA, WINTRUST_DATA_0,
        WINTRUST_FILE_INFO, WTD_CHOICE_FILE, WTD_REVOKE_NONE, WTD_STATEACTION_CLOSE,
        WTD_STATEACTION_VERIFY, WTD_UI_NONE,
    };

    unsafe {
        let h = HSTRING::from(exe_path);
        let mut file_info = WINTRUST_FILE_INFO {
            cbStruct: std::mem::size_of::<WINTRUST_FILE_INFO>() as u32,
            pcwszFilePath: PCWSTR(h.as_ptr()),
            hFile: HANDLE::default(),
            pgKnownSubject: std::ptr::null_mut(),
        };
        let mut data = WINTRUST_DATA {
            cbStruct: std::mem::size_of::<WINTRUST_DATA>() as u32,
            dwUIChoice: WTD_UI_NONE,
            fdwRevocationChecks: WTD_REVOKE_NONE,
            dwUnionChoice: WTD_CHOICE_FILE,
            dwStateAction: WTD_STATEACTION_VERIFY,
            Anonymous: WINTRUST_DATA_0 {
                pFile: &mut file_info,
            },
            ..Default::default()
        };
        let mut action = WINTRUST_ACTION_GENERIC_VERIFY_V2;

        let status = WinVerifyTrust(
            HWND::default(),
            &mut action,
            &mut data as *mut _ as *mut core::ffi::c_void,
        );

        // Always release the state, regardless of the verdict.
        data.dwStateAction = WTD_STATEACTION_CLOSE;
        let _ = WinVerifyTrust(
            HWND::default(),
            &mut action,
            &mut data as *mut _ as *mut core::ffi::c_void,
        );

        status == 0
    }
}

// ----------------------------------------------------------------- location

/// Translate a path into a short, honest phrase about where the file lives.
/// Only noteworthy locations get a phrase; everything ordinary returns None.
fn classify_location(exe_path: &str) -> Option<String> {
    let p = exe_path.to_lowercase();
    let phrase = if p.contains(r"\windows\system32") || p.contains(r"\windows\syswow64") {
        "in a Windows system folder"
    } else if p.contains(r"\appdata\local\temp\") || p.contains(r"\temp\") || p.contains(r"\tmp\") {
        "in a temporary folder, which honest apps rarely run from"
    } else if p.contains(r"\program files") {
        "in Program Files, where installed software normally lives"
    } else if p.contains(r"\programdata\") {
        "in the shared ProgramData folder"
    } else if p.contains(r"\appdata\") {
        "in your user AppData folder"
    } else {
        return None;
    };
    Some(phrase.to_string())
}
