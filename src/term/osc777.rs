//! OSC 777 extension framing, shared by Silkie's selectors (`caps`, `ink`,
//! `tablet-pointer`, `frame`, `latency`, `display`, `haptic`, ...).
//!
//! A frame is `ESC ] 777 ; <selector> ; key=value:key=value ESC \`. Frames sent
//! by a program are rejected whole by the terminal when the field section is
//! longer than 127 bytes, so [`Frame::encode`] refuses to produce such a frame
//! rather than emit something the terminal would silently drop.

use std::fmt::{self, Display, Write as _};

/// The framing rule from the OSC 777 extension framing specification: fields
/// longer than this, counted from after the semicolon ending the selector,
/// cause the whole frame to be rejected.
pub const MAX_FIELD_BYTES: usize = 127;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Frame {
    pub selector: String,
    pub fields: Vec<(String, String)>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FrameError {
    TooLong { selector: String, len: usize },
    DuplicateKey(String),
    InvalidKey(String),
}

impl Display for FrameError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            FrameError::TooLong { selector, len } => write!(
                f,
                "OSC 777 {selector} frame has {len} bytes of fields, more than the {MAX_FIELD_BYTES} the framing allows"
            ),
            FrameError::DuplicateKey(k) => write!(f, "OSC 777 frame repeats key {k}"),
            FrameError::InvalidKey(k) => write!(f, "OSC 777 frame key {k:?} is not valid"),
        }
    }
}

impl std::error::Error for FrameError {}

impl Frame {
    /// Starts a frame with the mandatory `v=1` and `op` fields.
    pub fn new(selector: &str, op: &str) -> Self {
        Frame {
            selector: selector.to_string(),
            fields: vec![("v".into(), "1".into()), ("op".into(), op.into())],
        }
    }

    pub fn field(mut self, key: &str, value: impl Display) -> Self {
        self.fields.push((key.to_string(), value.to_string()));
        self
    }

    pub fn get(&self, key: &str) -> Option<&str> {
        self.fields
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.as_str())
    }

    pub fn get_i64(&self, key: &str) -> Option<i64> {
        self.get(key).and_then(|v| v.parse().ok())
    }

    pub fn get_u64(&self, key: &str) -> Option<u64> {
        self.get(key).and_then(|v| v.parse().ok())
    }

    pub fn get_f64(&self, key: &str) -> Option<f64> {
        self.get(key).and_then(|v| v.parse().ok())
    }

    pub fn op(&self) -> Option<&str> {
        self.get("op")
    }

    pub fn version(&self) -> Option<u32> {
        self.get("v").and_then(|v| v.parse().ok())
    }

    /// The `key=value:key=value` section of the frame.
    pub fn field_text(&self) -> String {
        let mut s = String::new();
        for (i, (k, v)) in self.fields.iter().enumerate() {
            if i > 0 {
                s.push(':');
            }
            let _ = write!(s, "{k}={v}");
        }
        s
    }

    /// Encodes the frame terminated by ST (`ESC \`).
    pub fn encode(&self) -> Result<Vec<u8>, FrameError> {
        for (i, (k, _)) in self.fields.iter().enumerate() {
            if k.is_empty() || !k.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-') {
                return Err(FrameError::InvalidKey(k.clone()));
            }
            if self.fields[..i].iter().any(|(k2, _)| k2 == k) {
                return Err(FrameError::DuplicateKey(k.clone()));
            }
        }
        let fields = self.field_text();
        if fields.len() > MAX_FIELD_BYTES {
            return Err(FrameError::TooLong {
                selector: self.selector.clone(),
                len: fields.len(),
            });
        }
        let mut out = Vec::with_capacity(fields.len() + self.selector.len() + 12);
        out.extend_from_slice(b"\x1b]777;");
        out.extend_from_slice(self.selector.as_bytes());
        out.push(b';');
        out.extend_from_slice(fields.as_bytes());
        out.extend_from_slice(b"\x1b\\");
        Ok(out)
    }

    /// Parses the body of an OSC sequence (everything between `ESC ]` and the
    /// terminator). Returns `None` for OSC sequences that are not OSC 777
    /// extension frames.
    pub fn parse_osc_body(body: &[u8]) -> Option<Frame> {
        let text = std::str::from_utf8(body).ok()?;
        let rest = text.strip_prefix("777;")?;
        let (selector, fields) = match rest.split_once(';') {
            Some((s, f)) => (s, f),
            None => (rest, ""),
        };
        if selector.is_empty() {
            return None;
        }
        let mut parsed = Vec::new();
        for part in fields.split(':') {
            let part = part.trim();
            if part.is_empty() {
                continue;
            }
            match part.split_once('=') {
                Some((k, v)) => parsed.push((k.trim().to_string(), v.trim().to_string())),
                None => parsed.push((part.to_string(), String::new())),
            }
        }
        Some(Frame {
            selector: selector.to_string(),
            fields: parsed,
        })
    }
}

/// Convenience constructors for the requests this game sends.
pub mod req {
    use super::Frame;

    pub fn query(selector: &str, id: u32) -> Frame {
        Frame::new(selector, "query").field("id", id)
    }

    pub fn set_mode(selector: &str, mode: u32) -> Frame {
        Frame::new(selector, "mode").field("mode", mode)
    }

    pub fn read_mode(selector: &str) -> Frame {
        Frame::new(selector, "mode")
    }

    pub fn caps_reset() -> Frame {
        Frame::new("caps", "reset")
    }

    pub fn haptic_play(pattern: &str) -> Frame {
        Frame::new("haptic", "play").field("pattern", pattern)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn encodes_query() {
        let f = req::query("caps", 42);
        assert_eq!(
            f.encode().unwrap(),
            b"\x1b]777;caps;v=1:op=query:id=42\x1b\\".to_vec()
        );
    }

    #[test]
    fn rejects_overlong_fields() {
        let mut f = Frame::new("ink", "brush");
        for i in 0..20 {
            f = f.field(&format!("k{i}"), 1234567);
        }
        assert!(matches!(f.encode(), Err(FrameError::TooLong { .. })));
    }

    #[test]
    fn rejects_duplicate_keys() {
        let f = Frame::new("ink", "mode").field("mode", 1).field("mode", 0);
        assert!(matches!(f.encode(), Err(FrameError::DuplicateKey(_))));
    }

    #[test]
    fn largest_brush_fits() {
        // The ink specification notes that a brush with every id at its
        // largest fits when the widths are written with a few digits.
        let f = Frame::new("ink", "brush")
            .field("i", 4294967295u32)
            .field("g", 4294967295u32)
            .field("k", 4294967295u32)
            .field("n", 1)
            .field("fill", "#7a5230ff")
            .field("w0", 18)
            .field("w1", 34)
            .field("tools", 58);
        let bytes = f.encode().unwrap();
        assert!(bytes.len() < 160);
    }

    #[test]
    fn parses_reply() {
        let body = b"777;caps;v=1:op=cap:id=42:caps=3:ext=caps.1,frame.1,ink.1:media=d,f,t,s:format=24,32,100,200";
        let f = Frame::parse_osc_body(body).unwrap();
        assert_eq!(f.selector, "caps");
        assert_eq!(f.op(), Some("cap"));
        assert_eq!(f.get_u64("id"), Some(42));
        assert_eq!(f.get("ext"), Some("caps.1,frame.1,ink.1"));
        assert_eq!(f.version(), Some(1));
    }

    #[test]
    fn ignores_other_osc() {
        assert!(Frame::parse_osc_body(b"11;rgb:0000/0000/0000").is_none());
        assert!(Frame::parse_osc_body(b"777;").is_none());
    }
}
