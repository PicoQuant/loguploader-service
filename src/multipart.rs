//! Minimal `multipart/form-data` writer (RFC 7578) — hand-rolled to avoid a dependency
//! (research D4). One optional binary part + any number of text parts; built in memory.

/// Accumulates parts, then `finish()` yields the body bytes + the `Content-Type` value.
pub struct MultipartBody {
    boundary: String,
    parts: Vec<u8>,
}

impl MultipartBody {
    pub fn new() -> Self {
        MultipartBody {
            boundary: generate_boundary(),
            parts: Vec::new(),
        }
    }

    /// Add a text field.
    pub fn text(mut self, name: &str, value: &str) -> Self {
        self.write_headers(name, None, None);
        self.parts.extend_from_slice(value.as_bytes());
        self.parts.extend_from_slice(b"\r\n");
        self
    }

    /// Add the file part (`application/octet-stream`).
    pub fn file(mut self, name: &str, filename: &str, bytes: &[u8]) -> Self {
        self.write_headers(name, Some(filename), Some("application/octet-stream"));
        self.parts.extend_from_slice(bytes);
        self.parts.extend_from_slice(b"\r\n");
        self
    }

    /// Consume and produce `(body, content_type)`.
    pub fn finish(mut self) -> (Vec<u8>, String) {
        self.parts
            .extend_from_slice(format!("--{}--\r\n", self.boundary).as_bytes());
        let ct = format!("multipart/form-data; boundary={}", self.boundary);
        (self.parts, ct)
    }

    fn write_headers(&mut self, name: &str, filename: Option<&str>, content_type: Option<&str>) {
        self.parts
            .extend_from_slice(format!("--{}\r\n", self.boundary).as_bytes());
        match filename {
            Some(fname) => self.parts.extend_from_slice(
                format!(
                    "Content-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\n",
                    name,
                    sanitize(fname)
                )
                .as_bytes(),
            ),
            None => self.parts.extend_from_slice(
                format!("Content-Disposition: form-data; name=\"{name}\"\r\n").as_bytes(),
            ),
        }
        if let Some(ct) = content_type {
            self.parts
                .extend_from_slice(format!("Content-Type: {ct}\r\n").as_bytes());
        }
        self.parts.extend_from_slice(b"\r\n");
    }
}

impl Default for MultipartBody {
    fn default() -> Self {
        Self::new()
    }
}

/// Quotes/backslashes/newlines would break the header — strip them from filenames.
fn sanitize(s: &str) -> String {
    s.chars()
        .filter(|c| *c != '"' && *c != '\\' && *c != '\r' && *c != '\n')
        .collect()
}

/// A boundary unlikely to collide with content: fixed prefix + a pseudo-random hex suffix
/// seeded from the system clock and the address of a stack local.
fn generate_boundary() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_nanos())
        .unwrap_or(0);
    let stack_marker = &nanos as *const _ as usize as u128;
    let mut mixed = nanos ^ (stack_marker.rotate_left(17)) ^ 0x9E37_79B9_7F4A_7C15;
    let mut out = String::from("----pquploader-");
    for _ in 0..24 {
        mixed = mixed
            .wrapping_mul(6364136223846793005)
            .wrapping_add(1442695040888963407);
        let nibble = (mixed >> 124) as u8 & 0xF;
        out.push(char::from_digit(nibble as u32, 16).unwrap());
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn body_structure_is_rfc7578() {
        let (body, ct) = MultipartBody::new()
            .text("file_key", "pqdevice_conf")
            .file("content", "PQDevice.conf", b"hello")
            .finish();
        let text = String::from_utf8_lossy(&body);
        let boundary = ct.split("boundary=").nth(1).unwrap();

        assert!(text.contains(&format!("--{boundary}\r\n")));
        assert!(text.contains("Content-Disposition: form-data; name=\"file_key\"\r\n"));
        assert!(text.contains(
            "Content-Disposition: form-data; name=\"content\"; filename=\"PQDevice.conf\"\r\n"
        ));
        assert!(text.contains("Content-Type: application/octet-stream\r\n"));
        assert!(text.contains("\r\nhello\r\n"));
        assert!(text.ends_with(&format!("--{boundary}--\r\n")));
    }

    #[test]
    fn boundaries_differ_between_bodies() {
        let (_, a) = MultipartBody::new().finish();
        let (_, b) = MultipartBody::new().finish();
        assert_ne!(a, b);
    }

    #[test]
    fn filename_is_sanitized() {
        let (body, _) = MultipartBody::new()
            .file("content", "we\"ird\\name.xml", b"x")
            .finish();
        let text = String::from_utf8_lossy(&body);
        assert!(text.contains("filename=\"weirdname.xml\""));
    }
}
