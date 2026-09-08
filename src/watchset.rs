//! Resolve the per-product `WatchedFileSpec` table to concrete files and read each one
//! whole and self-consistently (FR-009, FR-013, FR-014, FR-015).

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use time::OffsetDateTime;

use crate::config::Config;
use crate::product::{FileKind, FileRoot, WatchedFileSpec};

/// Outcome of trying to read one concrete file this cycle
/// (`data-model.md` → ResolvedWatchedFile.read_result).
#[derive(Debug)]
pub enum ReadResult {
    Ok {
        bytes: Vec<u8>,
        sha256: String,
        mtime: Option<OffsetDateTime>,
    },
    /// Open sharing violation, or size changed between stat and EOF (torn read).
    Locked,
    Absent,
    TooLarge(u64),
}

#[derive(Debug)]
pub struct ResolvedWatchedFile {
    pub file_key: String,
    pub abs_path: PathBuf,
    pub read_result: ReadResult,
}

/// Expand every spec and read each resolved file. Order follows the table; globs expand
/// in directory order.
pub fn resolve_and_read(cfg: &Config) -> Vec<ResolvedWatchedFile> {
    let install_dir = cfg.product.install_dir();
    let data_dir = cfg.product.data_dir();
    let mut out = Vec::new();

    for spec in cfg.product.watched_files() {
        let root = match spec.root {
            FileRoot::InstallDir => &install_dir,
            FileRoot::DataDir => &data_dir,
        };
        match spec.kind {
            FileKind::Fixed => {
                let abs = root.join(spec.rel);
                out.push(read_one(
                    spec.file_key.to_string(),
                    abs,
                    cfg.backup_max_bytes,
                ));
            }
            FileKind::Glob => {
                for (key, abs) in expand_glob(spec, root) {
                    out.push(read_one(key, abs, cfg.backup_max_bytes));
                }
            }
        }
    }
    out
}

/// `dir\*.ext` → every matching file. `file_key` = spec prefix + real filename.
fn expand_glob(spec: &WatchedFileSpec, root: &Path) -> Vec<(String, PathBuf)> {
    // rel is like `*.xml` or `UserSettings\*.xml`
    let rel = spec.rel.replace('\\', "/");
    let (subdir, pattern) = match rel.rsplit_once('/') {
        Some((d, p)) => (root.join(d), p.to_string()),
        None => (root.to_path_buf(), rel),
    };
    let ext = pattern
        .strip_prefix("*.")
        .unwrap_or(&pattern)
        .to_ascii_lowercase();

    let mut matches = Vec::new();
    let Ok(entries) = std::fs::read_dir(&subdir) else {
        return matches;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name_ext = path
            .extension()
            .and_then(|s| s.to_str())
            .map(str::to_ascii_lowercase);
        if name_ext.as_deref() != Some(ext.as_str()) {
            continue;
        }
        let filename = match path.file_name().and_then(|s| s.to_str()) {
            Some(n) => n.to_string(),
            None => continue,
        };
        matches.push((format!("{}{}", spec.file_key, filename), path));
    }
    matches.sort_by(|a, b| a.0.cmp(&b.0));
    matches
}

fn read_one(file_key: String, abs_path: PathBuf, max_bytes: u64) -> ResolvedWatchedFile {
    let read_result = read_consistent(&abs_path, max_bytes);
    ResolvedWatchedFile {
        file_key,
        abs_path,
        read_result,
    }
}

/// Open share-read-only, read fully, and reject a file whose size moved under us.
fn read_consistent(path: &Path, max_bytes: u64) -> ReadResult {
    let meta = match std::fs::metadata(path) {
        Ok(m) => m,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return ReadResult::Absent,
        Err(e) if is_sharing_violation(&e) => return ReadResult::Locked,
        Err(_) => return ReadResult::Absent,
    };
    let size_before = meta.len();
    if size_before > max_bytes {
        return ReadResult::TooLarge(size_before);
    }

    let file = match open_share_read(path) {
        Ok(f) => f,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return ReadResult::Absent,
        Err(_) => return ReadResult::Locked,
    };

    use std::io::Read;
    let mut buf = Vec::with_capacity(size_before as usize);
    let mut handle = file;
    if handle.read_to_end(&mut buf).is_err() {
        return ReadResult::Locked;
    }

    // Torn-read guard (FR-015): if the file grew/shrank while we read it, retry next cycle.
    match std::fs::metadata(path) {
        Ok(m) if m.len() != size_before => return ReadResult::Locked,
        Ok(_) => {}
        Err(_) => return ReadResult::Locked,
    }
    if buf.len() as u64 > max_bytes {
        return ReadResult::TooLarge(buf.len() as u64);
    }

    let mut hasher = Sha256::new();
    hasher.update(&buf);
    let sha256 = hex(&hasher.finalize());
    let mtime = meta.modified().ok().map(OffsetDateTime::from);

    ReadResult::Ok {
        bytes: buf,
        sha256,
        mtime,
    }
}

#[cfg(windows)]
fn open_share_read(path: &Path) -> std::io::Result<std::fs::File> {
    use std::os::windows::fs::OpenOptionsExt;
    const FILE_SHARE_READ: u32 = 0x0000_0001;
    std::fs::OpenOptions::new()
        .read(true)
        .share_mode(FILE_SHARE_READ)
        .open(path)
}

#[cfg(not(windows))]
fn open_share_read(path: &Path) -> std::io::Result<std::fs::File> {
    std::fs::File::open(path)
}

fn is_sharing_violation(e: &std::io::Error) -> bool {
    // ERROR_SHARING_VIOLATION = 32, ERROR_LOCK_VIOLATION = 33
    matches!(e.raw_os_error(), Some(32) | Some(33))
        || e.kind() == std::io::ErrorKind::PermissionDenied
}

pub fn hex(bytes: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        let _ = write!(s, "{b:02x}");
    }
    s
}

pub fn sha256_hex(bytes: &[u8]) -> String {
    let mut hasher = Sha256::new();
    hasher.update(bytes);
    hex(&hasher.finalize())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn scratch(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("pqu_ws_{}_{name}", std::process::id()));
        fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn sha256_matches_known_vector() {
        // sha256("abc")
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }

    #[test]
    fn absent_file_is_absent() {
        let p = scratch("absent").join("nope.conf");
        assert!(matches!(read_consistent(&p, 1000), ReadResult::Absent));
    }

    #[test]
    fn oversize_file_is_flagged() {
        let d = scratch("big");
        let p = d.join("big.bin");
        fs::write(&p, vec![0u8; 2048]).unwrap();
        assert!(matches!(
            read_consistent(&p, 1024),
            ReadResult::TooLarge(2048)
        ));
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn ok_read_returns_hash_and_bytes() {
        let d = scratch("ok");
        let p = d.join("x.conf");
        fs::write(&p, b"hello").unwrap();
        match read_consistent(&p, 1024) {
            ReadResult::Ok { bytes, sha256, .. } => {
                assert_eq!(bytes, b"hello");
                assert_eq!(sha256, sha256_hex(b"hello"));
            }
            other => panic!("expected Ok, got {other:?}"),
        }
        fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn glob_expands_and_prefixes_file_key() {
        let d = scratch("glob");
        fs::write(d.join("A.xml"), b"1").unwrap();
        fs::write(d.join("B.xml"), b"2").unwrap();
        fs::write(d.join("ignore.txt"), b"3").unwrap();
        let spec = WatchedFileSpec {
            file_key: "settings/",
            root: FileRoot::DataDir,
            rel: "*.xml",
            kind: FileKind::Glob,
        };
        let got = expand_glob(&spec, &d);
        let keys: Vec<_> = got.iter().map(|(k, _)| k.clone()).collect();
        assert_eq!(keys, ["settings/A.xml", "settings/B.xml"]);
        fs::remove_dir_all(&d).ok();
    }
}
