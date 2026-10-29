//! Encoding of kitty graphics protocol commands (`ESC _ G ... ESC \`).
//!
//! Silkie's vector images (`f=200`), HDR images (`f=64`) and gamut tags
//! (`G=`) ride on the same commands, so they are encoded here as well.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;

/// The graphics protocol requires base64 payload chunks of at most 4096
/// bytes; every chunk but the last must be a multiple of four bytes long.
pub const MAX_CHUNK: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Format {
    Rgb,
    Rgba,
    Png,
    /// Silkie HDR: four little-endian f16 per pixel, linear extended sRGB.
    HdrF16,
    /// Silkie retained vector scene.
    Vector,
}

impl Format {
    pub fn code(self) -> u32 {
        match self {
            Format::Rgb => 24,
            Format::Rgba => 32,
            Format::Png => 100,
            Format::HdrF16 => 64,
            Format::Vector => 200,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Medium {
    /// Data inside the escape code, base64 encoded and chunked.
    Direct,
    /// A temporary file the terminal reads and deletes. The path must contain
    /// `tty-graphics-protocol` and live in a temporary directory.
    TempFile(PathBuf),
}

/// Response suppression (`q` key).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Quiet {
    All,
    ErrorsOnly,
    None,
}

impl Quiet {
    fn code(self) -> u8 {
        match self {
            Quiet::None => 0,
            Quiet::ErrorsOnly => 1,
            Quiet::All => 2,
        }
    }
}

/// Counts what was emitted, for diagnostics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct EncodeStats {
    pub commands: u64,
    pub escape_codes: u64,
    pub bytes: u64,
}

impl EncodeStats {
    pub fn add(&mut self, other: EncodeStats) {
        self.commands += other.commands;
        self.escape_codes += other.escape_codes;
        self.bytes += other.bytes;
    }
}

/// Writes one graphics command, chunking the payload when it is large.
/// `payload` is the raw payload before base64 encoding.
pub fn write_command(out: &mut Vec<u8>, control: &str, payload: &[u8], quiet: Quiet) -> EncodeStats {
    let start = out.len();
    let mut codes = 0u64;
    if payload.is_empty() {
        out.extend_from_slice(b"\x1b_G");
        out.extend_from_slice(control.as_bytes());
        out.extend_from_slice(b"\x1b\\");
        codes += 1;
    } else {
        let encoded = B64.encode(payload);
        let bytes = encoded.as_bytes();
        if bytes.len() <= MAX_CHUNK {
            out.extend_from_slice(b"\x1b_G");
            out.extend_from_slice(control.as_bytes());
            out.push(b';');
            out.extend_from_slice(bytes);
            out.extend_from_slice(b"\x1b\\");
            codes += 1;
        } else {
            let mut chunks = bytes.chunks(MAX_CHUNK).peekable();
            let mut first = true;
            while let Some(chunk) = chunks.next() {
                let more = chunks.peek().is_some();
                out.extend_from_slice(b"\x1b_G");
                if first {
                    out.extend_from_slice(control.as_bytes());
                    let _ = write!(out, ",m={}", u8::from(more));
                    first = false;
                } else {
                    let _ = write!(out, "m={}", u8::from(more));
                    if quiet != Quiet::None {
                        let _ = write!(out, ",q={}", quiet.code());
                    }
                }
                out.push(b';');
                out.extend_from_slice(chunk);
                out.extend_from_slice(b"\x1b\\");
                codes += 1;
            }
        }
    }
    EncodeStats {
        commands: 1,
        escape_codes: codes,
        bytes: (out.len() - start) as u64,
    }
}

/// Image transmission (`a=t`), optionally compressed with zlib.
#[derive(Clone, Debug)]
pub struct Transmit<'a> {
    pub id: u32,
    pub format: Format,
    pub width: u32,
    pub height: u32,
    pub data: &'a [u8],
    pub compress: bool,
    pub medium: Medium,
    pub quiet: Quiet,
    /// Silkie wide gamut tag: 0 sRGB, 1 Display P3, 2 Rec. 2020.
    pub gamut: u8,
}

impl Transmit<'_> {
    pub fn write(&self, out: &mut Vec<u8>) -> std::io::Result<EncodeStats> {
        let mut control = String::with_capacity(64);
        let _ = write!(control, "a=t,i={},f={}", self.id, self.format.code());
        if self.format != Format::Png {
            let _ = write!(control, ",s={},v={}", self.width, self.height);
        }
        if self.gamut != 0 {
            let _ = write!(control, ",G={}", self.gamut);
        }
        let _ = write!(control, ",q={}", self.quiet.code());
        let compressed;
        let body: &[u8] = if self.compress {
            compressed = zlib(self.data);
            let _ = write!(control, ",o=z");
            if self.format == Format::Png {
                let _ = write!(control, ",S={}", self.data.len());
            }
            &compressed
        } else {
            self.data
        };
        match &self.medium {
            Medium::Direct => Ok(write_command(out, &control, body, self.quiet)),
            Medium::TempFile(path) => {
                std::fs::write(path, body)?;
                let _ = write!(control, ",t=t");
                Ok(write_command(
                    out,
                    &control,
                    path.to_string_lossy().as_bytes(),
                    self.quiet,
                ))
            }
        }
    }
}

pub fn zlib(data: &[u8]) -> Vec<u8> {
    let mut enc = flate2::write::ZlibEncoder::new(
        Vec::with_capacity(data.len() / 4 + 64),
        flate2::Compression::fast(),
    );
    // Writing to a Vec cannot fail.
    enc.write_all(data).expect("zlib into memory");
    enc.finish().expect("zlib into memory")
}

/// A placement (`a=p`) at the current cursor position.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Default)]
pub struct Place {
    pub id: u32,
    pub placement: u32,
    /// Source rectangle in image pixels: x, y, w, h.
    pub src: Option<[u32; 4]>,
    /// Pixel offset within the starting cell; must be smaller than a cell.
    pub offset: (u32, u32),
    pub z: i32,
    /// Scale to this many columns (rows follow the aspect ratio).
    pub cols: Option<u32>,
    pub rows: Option<u32>,
}

impl Place {
    pub fn control(&self, quiet: Quiet) -> String {
        let mut c = String::with_capacity(64);
        let _ = write!(c, "a=p,i={},p={}", self.id, self.placement);
        if let Some([x, y, w, h]) = self.src {
            let _ = write!(c, ",x={x},y={y},w={w},h={h}");
        }
        if self.offset.0 != 0 {
            let _ = write!(c, ",X={}", self.offset.0);
        }
        if self.offset.1 != 0 {
            let _ = write!(c, ",Y={}", self.offset.1);
        }
        if let Some(cols) = self.cols {
            let _ = write!(c, ",c={cols}");
        }
        if let Some(rows) = self.rows {
            let _ = write!(c, ",r={rows}");
        }
        let _ = write!(c, ",z={},C=1,q={}", self.z, quiet.code());
        c
    }

    pub fn write(&self, out: &mut Vec<u8>, quiet: Quiet) -> EncodeStats {
        write_command(out, &self.control(quiet), &[], quiet)
    }
}

/// Deletes one placement, keeping the image data for reuse.
pub fn delete_placement(out: &mut Vec<u8>, id: u32, placement: u32) -> EncodeStats {
    write_command(
        out,
        &format!("a=d,d=i,i={id},p={placement},q=2"),
        &[],
        Quiet::All,
    )
}

/// Deletes an image with all of its placements and frees its data.
pub fn delete_image(out: &mut Vec<u8>, id: u32) -> EncodeStats {
    write_command(out, &format!("a=d,d=I,i={id},q=2"), &[], Quiet::All)
}

/// Applies a patch of vector records to a Silkie vector image (`a=e`).
pub fn vector_edit(out: &mut Vec<u8>, id: u32, records: &str, quiet: Quiet) -> EncodeStats {
    let mut control = format!("a=e,i={id},q={}", quiet.code());
    if records.len() > 2048 {
        control.push_str(",o=z");
        let z = zlib(records.as_bytes());
        write_command(out, &control, &z, quiet)
    } else {
        write_command(out, &control, records.as_bytes(), quiet)
    }
}

/// Transmits a whole vector scene, replacing any previous one with that id.
pub fn vector_transmit(
    out: &mut Vec<u8>,
    id: u32,
    width_units: f64,
    height_units: f64,
    records: &str,
    quiet: Quiet,
) -> EncodeStats {
    let mut control = format!(
        "a=t,f=200,i={id},s={},v={},q={}",
        fmt_num(width_units),
        fmt_num(height_units),
        quiet.code()
    );
    let z = zlib(records.as_bytes());
    if z.len() < records.len() {
        control.push_str(",o=z");
        write_command(out, &control, &z, quiet)
    } else {
        write_command(out, &control, records.as_bytes(), quiet)
    }
}

/// Formats a number compactly: integers without a fraction, others with at
/// most two decimals.
pub fn fmt_num(v: f64) -> String {
    if (v - v.round()).abs() < 1e-9 {
        format!("{}", v.round() as i64)
    } else {
        let s = format!("{v:.2}");
        s.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

/// The probe used to detect graphics support: a 1x1 RGB query image.
pub fn query_rgb_probe(out: &mut Vec<u8>, id: u32, extra: &str) {
    let control = format!("a=q,i={id},s=1,v=1,t=d,f=24{extra}");
    write_command(out, &control, &[0, 0, 0], Quiet::None);
}

/// Probe for Silkie vector image support, using the scene from the
/// specification.
pub fn query_vector_probe(out: &mut Vec<u8>, id: u32) {
    let control = format!("a=q,i={id},f=200,s=1,v=1");
    write_command(out, &control, b"p id=1 d=M0,0L1,0L1,1Z", Quiet::None);
}

/// Probe for HDR (`f=64`) support with one pixel.
pub fn query_hdr_probe(out: &mut Vec<u8>, id: u32) {
    let control = format!("a=q,i={id},f=64,s=1,v=1");
    // One pixel of half floats: 1.0, 1.0, 1.0, 1.0.
    let one = 0x3c00u16.to_le_bytes();
    let mut px = Vec::with_capacity(8);
    for _ in 0..4 {
        px.extend_from_slice(&one);
    }
    write_command(out, &control, &px, Quiet::None);
}

/// Probe for the temporary file medium; returns the probe file's path so the
/// caller can remove it if the terminal never reads it.
pub fn query_tempfile_probe(out: &mut Vec<u8>, id: u32) -> std::io::Result<PathBuf> {
    let path = temp_path(&format!("probe-{}-{id}", std::process::id()));
    std::fs::write(&path, [0u8, 0, 0])?;
    let control = format!("a=q,i={id},s=1,v=1,t=t,f=24");
    write_command(out, &control, path.to_string_lossy().as_bytes(), Quiet::None);
    Ok(path)
}

/// A temporary file path the terminal is allowed to read and delete.
pub fn temp_path(tag: &str) -> PathBuf {
    let dir = std::env::temp_dir();
    dir.join(format!("tty-graphics-protocol-silkie-scramble-{tag}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn decode_chunks(bytes: &[u8]) -> Vec<(String, Vec<u8>)> {
        let mut out = Vec::new();
        let mut rest = bytes;
        while let Some(start) = rest.windows(3).position(|w| w == b"\x1b_G") {
            let after = &rest[start + 3..];
            let end = after.windows(2).position(|w| w == b"\x1b\\").unwrap();
            let body = &after[..end];
            let (control, payload) = match body.iter().position(|&b| b == b';') {
                Some(p) => (&body[..p], &body[p + 1..]),
                None => (body, &b""[..]),
            };
            out.push((
                String::from_utf8(control.to_vec()).unwrap(),
                B64.decode(payload).unwrap_or_default(),
            ));
            rest = &after[end + 2..];
        }
        out
    }

    #[test]
    fn small_payload_is_single_code() {
        let mut out = Vec::new();
        let s = write_command(&mut out, "a=t,i=1", b"hello", Quiet::All);
        assert_eq!(s.escape_codes, 1);
        let chunks = decode_chunks(&out);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].0, "a=t,i=1");
        assert_eq!(chunks[0].1, b"hello");
    }

    #[test]
    fn large_payload_is_chunked() {
        let data: Vec<u8> = (0..20000u32).map(|i| (i * 7 % 251) as u8).collect();
        let mut out = Vec::new();
        let s = write_command(&mut out, "a=t,i=9,f=32,s=100,v=50", &data, Quiet::ErrorsOnly);
        assert!(s.escape_codes > 1);
        let chunks = decode_chunks(&out);
        assert!(chunks[0].0.starts_with("a=t,i=9,f=32,s=100,v=50,m=1"));
        for c in &chunks[1..chunks.len() - 1] {
            assert_eq!(c.0, "m=1,q=1");
        }
        assert_eq!(chunks.last().unwrap().0, "m=0,q=1");
        // Every chunk but the last is a multiple of four base64 bytes, so it
        // decodes on its own and the concatenation is the original payload.
        let joined: Vec<u8> = chunks.iter().flat_map(|c| c.1.clone()).collect();
        assert_eq!(joined, data);
    }

    #[test]
    fn placement_control() {
        let p = Place {
            id: 7,
            placement: 3,
            src: Some([64, 0, 64, 64]),
            offset: (5, 11),
            z: -1500,
            cols: None,
            rows: None,
        };
        assert_eq!(
            p.control(Quiet::ErrorsOnly),
            "a=p,i=7,p=3,x=64,y=0,w=64,h=64,X=5,Y=11,z=-1500,C=1,q=1"
        );
    }

    #[test]
    fn compressed_transmit_roundtrips() {
        let data = vec![200u8; 4 * 32 * 32];
        let mut out = Vec::new();
        Transmit {
            id: 5,
            format: Format::Rgba,
            width: 32,
            height: 32,
            data: &data,
            compress: true,
            medium: Medium::Direct,
            quiet: Quiet::ErrorsOnly,
            gamut: 1,
        }
        .write(&mut out)
        .unwrap();
        let chunks = decode_chunks(&out);
        assert_eq!(chunks.len(), 1);
        assert!(chunks[0].0.contains("o=z"));
        assert!(chunks[0].0.contains("G=1"));
        let mut d = flate2::read::ZlibDecoder::new(&chunks[0].1[..]);
        let mut raw = Vec::new();
        std::io::Read::read_to_end(&mut d, &mut raw).unwrap();
        assert_eq!(raw, data);
    }

    #[test]
    fn numbers_are_compact() {
        assert_eq!(fmt_num(3.0), "3");
        assert_eq!(fmt_num(2.5), "2.5");
        assert_eq!(fmt_num(-0.126), "-0.13");
        assert_eq!(fmt_num(1.10), "1.1");
    }
}
