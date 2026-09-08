//! Machine + instrument + OS identity for every submission (FR-004, FR-009a, FR-019).

use std::path::Path;

use serde::Serialize;

use crate::product::Product;

pub const ZERO_GUID: &str = "00000000-0000-0000-0000-000000000000";

/// Where the instrument serial came from — reported as `serial_source` (US3 scenario 3).
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Serial {
    /// Read from `LastOpenSerial.txt`.
    Known(String),
    /// File missing / unreadable / empty — submissions still proceed with `"unknown"`.
    Unknown,
}

impl Serial {
    /// The value that goes on the wire: the serial, or the literal `"unknown"` (FR-009a).
    pub fn wire_value(&self) -> &str {
        match self {
            Serial::Known(s) => s,
            Serial::Unknown => "unknown",
        }
    }

    pub fn source(&self) -> &'static str {
        match self {
            Serial::Known(_) => "file",
            Serial::Unknown => "unknown",
        }
    }

    pub fn is_known(&self) -> bool {
        matches!(self, Serial::Known(_))
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct OsInfo {
    pub version: String,
    pub build: String,
    pub arch: String,
}

/// Everything identity-related, resolved once at cycle start.
#[derive(Debug, Clone)]
pub struct Identity {
    pub machine_id: String,
    pub serial: Serial,
    pub os: OsInfo,
}

impl Identity {
    pub fn resolve(product: Product) -> Identity {
        Identity {
            machine_id: machine_id(),
            serial: instrument_serial(&product.serial_file()),
            os: os_info(),
        }
    }
}

/// `HKLM\SOFTWARE\Microsoft\Cryptography\MachineGuid` (64-bit view), zero-GUID fallback.
pub fn machine_id() -> String {
    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
        use winreg::RegKey;

        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let opened = hklm.open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Cryptography",
            KEY_READ | KEY_WOW64_64KEY,
        );
        match opened.and_then(|k| k.get_value::<String, _>("MachineGuid")) {
            Ok(guid) if !guid.trim().is_empty() => guid.trim().to_string(),
            Ok(_) => {
                log::warn!("MachineGuid was empty; using zero GUID");
                ZERO_GUID.to_string()
            }
            Err(e) => {
                log::warn!("could not read MachineGuid ({e}); using zero GUID");
                ZERO_GUID.to_string()
            }
        }
    }
    #[cfg(not(windows))]
    {
        ZERO_GUID.to_string()
    }
}

/// Last whitespace-separated token of `LastOpenSerial.txt` (v1 `getLumiSerial`).
pub fn instrument_serial(path: &Path) -> Serial {
    match std::fs::read_to_string(path) {
        Ok(contents) => match contents.split_whitespace().last() {
            Some(tok) if !tok.is_empty() => Serial::Known(tok.to_string()),
            _ => {
                log::warn!("{} contained no serial token; serial unknown", path.display());
                Serial::Unknown
            }
        },
        Err(e) => {
            log::warn!("could not read {} ({e}); serial unknown", path.display());
            Serial::Unknown
        }
    }
}

/// OS version / build / architecture. On Windows the version+build come from the
/// `CurrentVersion` registry key (works from a service, unlike `GetVersionEx` shims).
pub fn os_info() -> OsInfo {
    let arch = if cfg!(target_arch = "x86_64") {
        "x86_64"
    } else if cfg!(target_arch = "aarch64") {
        "aarch64"
    } else {
        "x86"
    }
    .to_string();

    #[cfg(windows)]
    {
        use winreg::enums::{HKEY_LOCAL_MACHINE, KEY_READ, KEY_WOW64_64KEY};
        use winreg::RegKey;

        let hklm = RegKey::predef(HKEY_LOCAL_MACHINE);
        let cv = hklm.open_subkey_with_flags(
            r"SOFTWARE\Microsoft\Windows NT\CurrentVersion",
            KEY_READ | KEY_WOW64_64KEY,
        );
        let (mut version, mut build) = ("unknown".to_string(), "unknown".to_string());
        if let Ok(key) = cv {
            let major: Option<u32> = key.get_value("CurrentMajorVersionNumber").ok();
            let minor: Option<u32> = key.get_value("CurrentMinorVersionNumber").ok();
            let build_num: Option<String> = key.get_value("CurrentBuildNumber").ok();
            if let Some(b) = &build_num {
                build = b.clone();
            }
            version = match (major, minor, &build_num) {
                (Some(maj), Some(min), Some(b)) => format!("{maj}.{min}.{b}"),
                _ => key
                    .get_value::<String, _>("CurrentVersion")
                    .unwrap_or_else(|_| "unknown".to_string()),
            };
        }
        return OsInfo { version, build, arch };
    }
    #[cfg(not(windows))]
    {
        OsInfo {
            version: "non-windows".to_string(),
            build: "0".to_string(),
            arch,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn serial_wire_and_source() {
        let known = Serial::Known("SN-12345".to_string());
        assert_eq!(known.wire_value(), "SN-12345");
        assert_eq!(known.source(), "file");
        assert!(known.is_known());

        assert_eq!(Serial::Unknown.wire_value(), "unknown");
        assert_eq!(Serial::Unknown.source(), "unknown");
        assert!(!Serial::Unknown.is_known());
    }

    #[test]
    fn serial_reads_last_token() {
        let dir = std::env::temp_dir().join(format!("pqu_serial_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("LastOpenSerial.txt");
        let mut fh = std::fs::File::create(&f).unwrap();
        writeln!(fh, "2026-09-08 08:00:00  SN-98765").unwrap();
        drop(fh);
        assert_eq!(instrument_serial(&f), Serial::Known("SN-98765".to_string()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn serial_absent_is_unknown() {
        let missing = std::env::temp_dir().join("pqu_definitely_missing_serial.txt");
        assert_eq!(instrument_serial(&missing), Serial::Unknown);
    }

    #[test]
    fn os_info_has_a_known_arch() {
        let os = os_info();
        assert!(["x86_64", "aarch64", "x86"].contains(&os.arch.as_str()));
    }
}
