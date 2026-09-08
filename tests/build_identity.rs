//! T028 (US3) — compile-time identity resolution: `PQ_PRODUCT` → bucket / dirs / watched
//! set; `PQ_CHANNEL` → `Channel`. (The "release build fails on a bad product/channel/token"
//! rule lives in `build.rs` + the CI grep gate — see `tests/no_secret_in_source.rs` and
//! `.github/workflows/windows-build.yml`.)

use pquploader::config::{self, Channel};
use pquploader::product::{FileKind, FileRoot, Product};

#[test]
fn compiled_product_is_one_of_the_two() {
    assert!(matches!(config::PRODUCT, "luminosa" | "solira"));
    let p = Product::current();
    assert_eq!(p.bucket(), config::PRODUCT);
}

#[test]
fn dirs_follow_the_product_name() {
    let p = Product::current();
    let seg = p.dir_name();
    assert!(p.install_dir().to_string_lossy().contains(seg));
    assert!(p.data_dir().to_string_lossy().contains(seg));
    assert!(p.data_dir().to_string_lossy().contains("PicoQuant"));
    assert!(p.serial_file().ends_with("LastOpenSerial.txt"));
    assert!(p.agent_dir().ends_with("v2agent"));
}

#[test]
fn watched_set_is_the_fr008_table_without_logs() {
    let specs = Product::current().watched_files();
    let keys: Vec<_> = specs.iter().map(|s| s.file_key).collect();
    assert_eq!(keys, ["pqdevice_db", "pqdevice_conf", "settings/", "usersettings/"]);

    assert_eq!(specs[0].root, FileRoot::InstallDir);
    assert_eq!(specs[0].kind, FileKind::Fixed);
    assert_eq!(specs[2].root, FileRoot::DataDir);
    assert_eq!(specs[2].kind, FileKind::Glob);

    for s in specs {
        let low = s.rel.to_ascii_lowercase();
        assert!(!low.contains("pqlog"));
        assert!(!low.contains("laserpower"));
    }
}

#[test]
fn compiled_channel_parses() {
    assert!(matches!(config::CHANNEL, "stable" | "beta"));
    assert_eq!(Channel::current().as_str(), config::CHANNEL);
}

#[test]
fn version_is_non_empty_and_from_the_version_file() {
    assert!(!config::VERSION.is_empty());
    let on_disk = std::fs::read_to_string(concat!(env!("CARGO_MANIFEST_DIR"), "/VERSION"))
        .unwrap()
        .trim()
        .to_string();
    assert_eq!(config::VERSION, on_disk);
}
