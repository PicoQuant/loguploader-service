//! T023 (US2) — the backup body carries exactly the parts named in
//! `contracts/backend-api.md` §2, with `file_mtime` / `agent_version` / `client_timestamp`
//! optional.

use pquploader::backup::build_submission;
use pquploader::identity::{Identity, OsInfo, Serial};

fn identity(serial: Serial) -> Identity {
    Identity {
        machine_id: "0f4a-machine".to_string(),
        serial,
        os: OsInfo {
            version: "10.0.19045".into(),
            build: "19045".into(),
            arch: "x86_64".into(),
        },
        instrument_sw: Default::default(),
    }
}

fn parts(body: &[u8]) -> Vec<String> {
    let text = String::from_utf8_lossy(body);
    text.match_indices("name=\"")
        .map(|(i, _)| {
            let rest = &text[i + 6..];
            rest.split('"').next().unwrap_or("").to_string()
        })
        .collect()
}

#[test]
fn required_and_optional_parts_present() {
    let (body, ct) = build_submission(
        &identity(Serial::Known("SN-12345".into())),
        "pqdevice_conf",
        std::path::Path::new(r"C:\Program Files\PicoQuant\Luminosa\PQDevice.conf"),
        &"6".repeat(64),
        b"the whole file",
        Some("2026-09-08T07:55:00Z"),
        "2026-09-08T08:00:00Z",
    )
    .finish();

    assert!(ct.starts_with("multipart/form-data; boundary="));
    let names = parts(&body);
    for required in [
        "content",
        "instrument_serial",
        "machine_id",
        "file_key",
        "source_path",
        "content_sha256",
    ] {
        assert!(
            names.contains(&required.to_string()),
            "missing required part {required}"
        );
    }
    for optional in ["file_mtime", "agent_version", "client_timestamp"] {
        assert!(
            names.contains(&optional.to_string()),
            "missing optional part {optional}"
        );
    }

    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("filename=\"PQDevice.conf\""));
    assert!(text.contains(r"C:\Program Files\PicoQuant\Luminosa\PQDevice.conf"));
    assert!(text.contains(&"6".repeat(64)));
    assert!(text.contains("\r\nthe whole file\r\n"));
}

#[test]
fn unknown_serial_travels_verbatim() {
    let (body, _) = build_submission(
        &identity(Serial::Unknown),
        "settings/Settings.xml",
        std::path::Path::new(r"C:\ProgramData\PicoQuant\Luminosa\Settings.xml"),
        &"a".repeat(64),
        b"x",
        None,
        "2026-09-08T08:00:00Z",
    )
    .finish();
    let text = String::from_utf8_lossy(&body);
    assert!(text.contains("name=\"instrument_serial\"\r\n\r\nunknown\r\n"));
    // file_mtime omitted when None
    assert!(!text.contains("name=\"file_mtime\""));
}
