// Docs: docs/souls-format/dcx.md

use crate::oodle::{Oodle, OodleError};

const DATA_OFFSET: usize = 0x4C;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::oodle::LEVEL_OPTIMAL2;
    use crate::test_support::{msgbnd_path, oodle};

    #[test]
    #[ignore = "reads an external game install — see docs/known-offsets.md"]
    fn compress_then_decompress_is_lossless() {
        let oodle = oodle();

        // Real binder bytes rather than synthetic filler: incompressible noise would exercise
        // a different path through Oodle than the text this actually has to carry.
        let raw = std::fs::read(msgbnd_path()).expect("binder should be readable");
        let payload = unwrap_krak(&raw, &oodle).expect("should unwrap");

        let wrapped = wrap_krak(&payload, &oodle, LEVEL_OPTIMAL2).expect("should wrap");
        eprintln!(
            "payload {} bytes -> dcx {} bytes (original dcx was {})",
            payload.len(),
            wrapped.len(),
            raw.len()
        );

        let back = unwrap_krak(&wrapped, &oodle).expect("should unwrap what we just wrote");
        assert_eq!(back.len(), payload.len(), "round-trip changed the length");
        assert!(back == payload, "round-trip changed the bytes");
    }
}

#[derive(Debug, thiserror::Error)]
pub enum DcxError {
    #[error("not a DCX container (bad magic)")]
    NotDcx,

    #[error("expected {expected} compression, found '{found}'")]
    WrongCompression {
        expected: &'static str,
        found: String,
    },

    #[error("DCX header claims {claimed} bytes of payload but the file holds only {available}")]
    Truncated { claimed: usize, available: usize },

    #[error("zstd decode failed: {0}")]
    Zstd(String),

    #[error(transparent)]
    Oodle(#[from] OodleError),
}

/// The fields every DCX header carries, whichever compressor it names.
///
/// One parser for all variants: the envelope is identical from `DCX\0` through `DCA\0`, and
/// only the four-byte tag at `0x28` and the decompressor differ. A second copy of these
/// offsets is how a reader for one variant drifts from a reader for another.
struct Header {
    /// The tag at `0x28` — `KRAK`, `ZSTD`, …
    compression: [u8; 4],
    uncompressed: usize,
    /// Where the payload ends, already bounds-checked against `data`.
    end: usize,
}

fn read_header(data: &[u8]) -> Result<Header, DcxError> {
    if data.len() < DATA_OFFSET
        || &data[0x00..0x04] != b"DCX\0"
        || &data[0x18..0x1C] != b"DCS\0"
        || &data[0x24..0x28] != b"DCP\0"
    {
        return Err(DcxError::NotDcx);
    }

    let uncompressed = u32::from_be_bytes(data[0x1C..0x20].try_into().unwrap()) as usize;
    let compressed = u32::from_be_bytes(data[0x20..0x24].try_into().unwrap()) as usize;

    // A header that lies about its own size would otherwise panic on the payload slice.
    // Turning that into an error keeps a corrupt file from looking like a crate bug.
    let end = DATA_OFFSET + compressed;
    if end > data.len() {
        return Err(DcxError::Truncated {
            claimed: end,
            available: data.len(),
        });
    }

    Ok(Header {
        compression: data[0x28..0x2C].try_into().unwrap(),
        uncompressed,
        end,
    })
}

impl Header {
    fn expect(&self, tag: &'static [u8; 4], name: &'static str) -> Result<(), DcxError> {
        if &self.compression == tag {
            Ok(())
        } else {
            Err(DcxError::WrongCompression {
                expected: name,
                found: String::from_utf8_lossy(&self.compression).into_owned(),
            })
        }
    }
}

/// Unwraps a `DCX_ZSTD` container. Used by `regulation.bin`, whose decrypted payload is
/// zstd-wrapped on Shadow-of-the-Erdtree-era files.
pub fn unwrap_zstd(data: &[u8]) -> Result<Vec<u8>, DcxError> {
    let header = read_header(data)?;
    header.expect(b"ZSTD", "ZSTD")?;

    let cursor = std::io::Cursor::new(&data[DATA_OFFSET..header.end]);
    let mut decoder =
        ruzstd::StreamingDecoder::new(cursor).map_err(|e| DcxError::Zstd(e.to_string()))?;
    let mut out = Vec::with_capacity(header.uncompressed);
    std::io::Read::read_to_end(&mut decoder, &mut out).map_err(|e| DcxError::Zstd(e.to_string()))?;
    Ok(out)
}

pub fn unwrap_krak(data: &[u8], oodle: &Oodle) -> Result<Vec<u8>, DcxError> {
    let header = read_header(data)?;
    header.expect(b"KRAK", "KRAK")?;

    Ok(oodle.decompress(&data[DATA_OFFSET..header.end], header.uncompressed)?)
}

pub fn wrap_krak(payload: &[u8], oodle: &Oodle, level: i32) -> Result<Vec<u8>, DcxError> {
    let compressed = oodle.compress(payload, crate::oodle::COMPRESSOR_KRAKEN, level)?;

    let mut out = Vec::with_capacity(DATA_OFFSET + compressed.len() + 0x10);
    let put_i32 = |o: &mut Vec<u8>, v: i32| o.extend_from_slice(&v.to_be_bytes());

    out.extend_from_slice(b"DCX\0");
    put_i32(&mut out, 0x11000);
    put_i32(&mut out, 0x18);
    put_i32(&mut out, 0x24);
    put_i32(&mut out, 0x44);
    put_i32(&mut out, 0x4C);
    out.extend_from_slice(b"DCS\0");
    out.extend_from_slice(&(payload.len() as u32).to_be_bytes());
    out.extend_from_slice(&(compressed.len() as u32).to_be_bytes());
    out.extend_from_slice(b"DCP\0");
    out.extend_from_slice(b"KRAK");
    put_i32(&mut out, 0x20);
    out.push(level as u8);
    out.extend_from_slice(&[0, 0, 0]);
    put_i32(&mut out, 0);
    put_i32(&mut out, 0);
    put_i32(&mut out, 0);
    put_i32(&mut out, 0x10100);
    out.extend_from_slice(b"DCA\0");
    put_i32(&mut out, 8);
    debug_assert_eq!(out.len(), DATA_OFFSET);

    out.extend_from_slice(&compressed);
    while out.len() % 0x10 != 0 {
        out.push(0);
    }

    Ok(out)
}