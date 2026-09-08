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

/// The instrument control-software version, from two independent sources (FR-004a).
#[derive(Debug, Clone, Default, Serialize)]
pub struct InstrumentSoftware {
    /// File-version resource of `<install_dir>\<Product>.exe` — the version installed now.
    pub version: Option<String>,
    /// Version stamped in the header of the newest `*.pqlog` — the version that last ran.
    pub log_version: Option<String>,
}

impl InstrumentSoftware {
    pub fn is_empty(&self) -> bool {
        self.version.is_none() && self.log_version.is_none()
    }
}

/// Everything identity-related, resolved once at cycle start.
#[derive(Debug, Clone)]
pub struct Identity {
    pub machine_id: String,
    pub serial: Serial,
    pub os: OsInfo,
    pub instrument_sw: InstrumentSoftware,
}

impl Identity {
    pub fn resolve(product: Product) -> Identity {
        Identity {
            machine_id: machine_id(),
            serial: instrument_serial(&product.serial_file()),
            os: os_info(),
            instrument_sw: InstrumentSoftware {
                version: control_exe_version(&product.control_exe()),
                log_version: newest_pqlog_version(&product.logs_dir()),
            },
        }
    }
}

/// `major.minor.build.revision` from a Windows file-version resource (`version.dll`), or
/// `None` if the file is missing / has no version info.
pub fn control_exe_version(path: &Path) -> Option<String> {
    #[cfg(windows)]
    {
        use std::os::windows::ffi::OsStrExt;

        #[link(name = "version")]
        extern "system" {
            fn GetFileVersionInfoSizeW(f: *const u16, h: *mut u32) -> u32;
            fn GetFileVersionInfoW(
                f: *const u16,
                h: u32,
                len: u32,
                data: *mut core::ffi::c_void,
            ) -> i32;
            fn VerQueryValueW(
                block: *const core::ffi::c_void,
                sub: *const u16,
                out: *mut *mut core::ffi::c_void,
                len: *mut u32,
            ) -> i32;
        }

        if !path.is_file() {
            return None;
        }
        let wide: Vec<u16> = path
            .as_os_str()
            .encode_wide()
            .chain(std::iter::once(0))
            .collect();
        // SAFETY: standard version.dll usage; buffers are sized by the API itself.
        unsafe {
            let mut handle = 0u32;
            let size = GetFileVersionInfoSizeW(wide.as_ptr(), &mut handle);
            if size == 0 {
                return None;
            }
            let mut buf = vec![0u8; size as usize];
            if GetFileVersionInfoW(wide.as_ptr(), 0, size, buf.as_mut_ptr().cast()) == 0 {
                return None;
            }
            let root: [u16; 2] = [0x005C, 0x0000]; // "\\\0"
            let mut ptr: *mut core::ffi::c_void = std::ptr::null_mut();
            let mut len = 0u32;
            if VerQueryValueW(buf.as_ptr().cast(), root.as_ptr(), &mut ptr, &mut len) == 0
                || ptr.is_null()
                || (len as usize) < 16
            {
                return None;
            }
            // VS_FIXEDFILEINFO: [sig, strucVer, fileVerMS, fileVerLS, ...] (u32 each)
            let fixed = ptr as *const u32;
            let ms = *fixed.add(2);
            let ls = *fixed.add(3);
            Some(format!(
                "{}.{}.{}.{}",
                ms >> 16,
                ms & 0xFFFF,
                ls >> 16,
                ls & 0xFFFF
            ))
        }
    }
    #[cfg(not(windows))]
    {
        let _ = path;
        None
    }
}

/// Version field from the header line of the newest `*.pqlog` in `logs_dir`.
/// Header shape: `PQDCLog;<local timestamp>;<version>` — take the 3rd `;` field.
pub fn newest_pqlog_version(logs_dir: &Path) -> Option<String> {
    use std::io::{BufRead, BufReader};

    let newest = std::fs::read_dir(logs_dir)
        .ok()?
        .flatten()
        .filter(|e| {
            e.path()
                .extension()
                .and_then(|s| s.to_str())
                .is_some_and(|s| s.eq_ignore_ascii_case("pqlog"))
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .max_by_key(|(mtime, _)| *mtime)
        .map(|(_, p)| p)?;

    let file = std::fs::File::open(&newest).ok()?;
    let mut first = String::new();
    BufReader::new(file).read_line(&mut first).ok()?;
    let v = first.trim().split(';').nth(2)?.trim();
    if v.is_empty() || !v.starts_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some(v.to_string())
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
                log::warn!(
                    "{} contained no serial token; serial unknown",
                    path.display()
                );
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
    let (version, build) = {
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
        (version, build)
    };
    #[cfg(not(windows))]
    let (version, build) = ("non-windows".to_string(), "0".to_string());

    OsInfo {
        version,
        build,
        arch,
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

    #[test]
    fn pqlog_version_picks_newest_and_parses_header() {
        let dir = std::env::temp_dir().join(format!("pqu_pqlog_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(
            dir.join("Log_20250101_000000.pqlog"),
            "PQDCLog;2025-01-01 00:00:00 (GMT+02);1.0.0.1000\nmore lines\n",
        )
        .unwrap();
        // newest by mtime
        std::thread::sleep(std::time::Duration::from_millis(10));
        std::fs::write(
            dir.join("Log_20260626_111203.pqlog"),
            "PQDCLog;2026-06-26 11:12:03 (GMT+02);1.0.0.2094\nI;Plugins:Plugin system\n",
        )
        .unwrap();
        std::fs::write(dir.join("notes.txt"), "ignored").unwrap();

        assert_eq!(newest_pqlog_version(&dir), Some("1.0.0.2094".to_string()));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn pqlog_version_none_when_absent_or_unparseable() {
        let missing = std::env::temp_dir().join("pqu_no_such_logs_dir_xyz");
        assert_eq!(newest_pqlog_version(&missing), None);

        let dir = std::env::temp_dir().join(format!("pqu_pqlog_bad_{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("x.pqlog"), "not;a;PQDC header shape\n").unwrap();
        // 3rd field is "PQDC header shape" -> not digit-led -> None
        assert_eq!(newest_pqlog_version(&dir), None);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn control_exe_version_none_for_missing_file() {
        let missing = std::env::temp_dir().join("pqu_no_such_exe.exe");
        assert_eq!(control_exe_version(&missing), None);
    }
}
