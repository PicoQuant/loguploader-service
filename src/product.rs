//! Compile-time product identity and the per-product watched-file table (FR-002a, FR-008).
//!
//! There is exactly one `.exe` per product; nothing selects the product at runtime
//! (`data-model.md` → BuildIdentity). A wrong Solira path is fixed in the Solira build only.

use std::path::PathBuf;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Product {
    Luminosa,
    Solira,
}

impl Product {
    /// The product compiled into this binary (`build.rs` → `PQ_PRODUCT`).
    pub fn current() -> Product {
        match env!("PQ_PRODUCT") {
            "solira" => Product::Solira,
            "luminosa" => Product::Luminosa,
            other => panic!("build emitted an unknown PQ_PRODUCT: {other}"),
        }
    }

    /// The submission bucket / URL segment (FR-002d, FR-019).
    pub fn bucket(self) -> &'static str {
        match self {
            Product::Luminosa => "luminosa",
            Product::Solira => "solira",
        }
    }

    /// Title-cased folder segment used under `Program Files` / `ProgramData`.
    pub fn dir_name(self) -> &'static str {
        match self {
            Product::Luminosa => "Luminosa",
            Product::Solira => "Solira",
        }
    }

    /// `C:\Program Files\PicoQuant\<Product>\` — honours `%ProgramFiles%` if set.
    pub fn install_dir(self) -> PathBuf {
        let base = std::env::var_os("ProgramFiles")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\Program Files"));
        base.join("PicoQuant").join(self.dir_name())
    }

    /// `C:\ProgramData\PicoQuant\<Product>\` — honours `%ProgramData%` if set.
    pub fn data_dir(self) -> PathBuf {
        let base = std::env::var_os("ProgramData")
            .map(PathBuf::from)
            .unwrap_or_else(|| PathBuf::from(r"C:\ProgramData"));
        base.join("PicoQuant").join(self.dir_name())
    }

    /// `<data_dir>\v2agent\` — state.json + cycles.log live here, OUTSIDE the install dir
    /// so a v2 self-update preserves them (FR-034, Constitution VI).
    pub fn agent_dir(self) -> PathBuf {
        self.data_dir().join("v2agent")
    }

    /// `<data_dir>\Logs\LastOpenSerial.txt` (FR-009a).
    pub fn serial_file(self) -> PathBuf {
        self.data_dir().join("Logs").join("LastOpenSerial.txt")
    }

    /// The watched configuration-file set for this product (FR-008). Operational logs are
    /// deliberately absent from this table (`Logs\*.pqlog`, `LaserPower.log` — FR-001).
    pub fn watched_files(self) -> &'static [WatchedFileSpec] {
        // Both products currently share the same layout; the Solira paths are a working
        // assumption pending confirmation against a real install (spec Open Items).
        match self {
            Product::Luminosa | Product::Solira => WATCHED,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileRoot {
    InstallDir,
    DataDir,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FileKind {
    /// A single concrete file at `rel`.
    Fixed,
    /// A `*.xml`-style glob under `rel`'s parent; expands to 0..n files at cycle time.
    Glob,
}

/// One entry in the per-product watched set (`data-model.md` → WatchedFileSpec).
#[derive(Debug, Clone, Copy)]
pub struct WatchedFileSpec {
    /// Logical id sent to the backend. For globs this is a *prefix* (`settings/`);
    /// `watchset` appends the real filename.
    pub file_key: &'static str,
    pub root: FileRoot,
    /// Path (Fixed) or `dir\*.ext` glob (Glob), relative to `root`. Backslash-separated.
    pub rel: &'static str,
    pub kind: FileKind,
}

/// FR-008: PQDevice.db + PQDevice.conf under InstallDir; `*.xml` under DataDir;
/// `UserSettings\*.xml` under DataDir. Logs excluded.
static WATCHED: &[WatchedFileSpec] = &[
    WatchedFileSpec {
        file_key: "pqdevice_db",
        root: FileRoot::InstallDir,
        rel: "PQDevice.db",
        kind: FileKind::Fixed,
    },
    WatchedFileSpec {
        file_key: "pqdevice_conf",
        root: FileRoot::InstallDir,
        rel: "PQDevice.conf",
        kind: FileKind::Fixed,
    },
    WatchedFileSpec {
        file_key: "settings/",
        root: FileRoot::DataDir,
        rel: r"*.xml",
        kind: FileKind::Glob,
    },
    WatchedFileSpec {
        file_key: "usersettings/",
        root: FileRoot::DataDir,
        rel: r"UserSettings\*.xml",
        kind: FileKind::Glob,
    },
];

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn buckets() {
        assert_eq!(Product::Luminosa.bucket(), "luminosa");
        assert_eq!(Product::Solira.bucket(), "solira");
    }

    #[test]
    fn dirs_end_with_product_name() {
        assert!(Product::Luminosa.install_dir().ends_with("Luminosa"));
        assert!(Product::Solira.data_dir().ends_with("Solira"));
        assert!(Product::Luminosa.agent_dir().ends_with("v2agent"));
    }

    #[test]
    fn watched_set_has_no_logs() {
        for spec in Product::Luminosa.watched_files() {
            assert!(!spec.rel.to_ascii_lowercase().contains("pqlog"));
            assert!(!spec.rel.to_ascii_lowercase().contains("laserpower"));
            assert!(!spec.rel.to_ascii_lowercase().contains(r"logs\"));
        }
    }

    #[test]
    fn watched_set_shape() {
        let keys: Vec<_> = WATCHED.iter().map(|s| s.file_key).collect();
        assert_eq!(keys, ["pqdevice_db", "pqdevice_conf", "settings/", "usersettings/"]);
    }
}
