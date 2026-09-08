//! Shared helpers for the integration tests. Each test file pulls in the subset it needs,
//! so unused items here are expected.
#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Mutex, MutexGuard, OnceLock};

use pquploader::product::Product;

/// Tests that mutate process-global env (`ProgramFiles` / `ProgramData`) must hold this
/// lock so they don't race each other.
pub fn env_lock() -> MutexGuard<'static, ()> {
    static LOCK: OnceLock<Mutex<()>> = OnceLock::new();
    LOCK.get_or_init(|| Mutex::new(()))
        .lock()
        .unwrap_or_else(|e| e.into_inner())
}

/// A fresh scratch tree with `%ProgramFiles%` / `%ProgramData%` pointed inside it, so a
/// real backup pass reads fake watched files. Returns the roots; restores nothing (hold
/// `env_lock` for the whole test).
pub struct FakeRoots {
    pub program_files: PathBuf,
    pub program_data: PathBuf,
}

impl FakeRoots {
    pub fn create(tag: &str) -> FakeRoots {
        let base = std::env::temp_dir().join(format!(
            "pqu_it_{}_{}_{}",
            std::process::id(),
            tag,
            nanos()
        ));
        let program_files = base.join("ProgramFiles");
        let program_data = base.join("ProgramData");
        let product_dir = Product::current().dir_name();

        std::fs::create_dir_all(program_files.join("PicoQuant").join(product_dir)).unwrap();
        std::fs::create_dir_all(
            program_data
                .join("PicoQuant")
                .join(product_dir)
                .join("UserSettings"),
        )
        .unwrap();
        std::fs::create_dir_all(program_data.join("PicoQuant").join(product_dir).join("Logs"))
            .unwrap();

        std::env::set_var("ProgramFiles", &program_files);
        std::env::set_var("ProgramData", &program_data);

        FakeRoots {
            program_files,
            program_data,
        }
    }

    pub fn install_dir(&self) -> PathBuf {
        self.program_files
            .join("PicoQuant")
            .join(Product::current().dir_name())
    }

    pub fn data_dir(&self) -> PathBuf {
        self.program_data
            .join("PicoQuant")
            .join(Product::current().dir_name())
    }

    pub fn write_watched(&self, rel: &str, bytes: &[u8]) {
        let p = self.install_dir().join(rel);
        if let Some(parent) = p.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        std::fs::write(p, bytes).unwrap();
    }
}

impl Drop for FakeRoots {
    fn drop(&mut self) {
        if let Some(base) = self.program_files.parent() {
            let _ = std::fs::remove_dir_all(base);
        }
    }
}

fn nanos() -> u128 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0)
}
