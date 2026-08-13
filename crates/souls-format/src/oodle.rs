// Docs: docs/souls-format/oodle.md

use std::ffi::c_void;
use libloading::Library;
use std::path::Path;
use std::ptr::null_mut;

type DecompressFn = unsafe extern "C" fn(
    *const u8, // compBuf
    i64, // compBufSize
    *mut u8, // rawBuf
    i64,    // rawLen
    i32, //fuzzSafety
    i32, // checkCRC
    i32, // verbosityLevel
    *mut c_void, // decBufBase
    i64, // decBufSize
    *mut c_void, // fpCallback
    *mut c_void, // callbackUserData
    *mut c_void, // decoderMemory
    i64, // decoderMemorySize
    i32, // threadPhase
)-> i64;

type GetDecodeBufferSizeFn = unsafe extern "C" fn(i64,i32)-> i64;

type CompressFn = unsafe extern "C" fn(
    i32,                    // compressor
    *const u8,              // rawBuf
    i64,                    // rawLen
    *mut u8,                // compBuf
    i32,                    // level
    *const CompressOptions, // pOptions
    *mut c_void,            // dictionaryBase
    *mut c_void,            // lrm
    *mut c_void,            // scratchMem
    i64,                    // scratchSize
) -> i64;

type GetCompressedBufferSizeNeededFn = unsafe extern "C" fn(i64) -> i64;

type CompressOptionsGetDefaultFn = unsafe extern "C" fn(i32, i32) -> *const CompressOptions;

#[repr(C)]
#[derive(Debug, Clone, Copy)]
struct CompressOptions {
    verbosity: u32,
    min_match_len: i32,
    seek_chunk_reset: i32,
    seek_chunk_len: i32,
    profile: i32,
    dictionary_size: i32,
    space_speed_tradeoff_bytes: i32,
    max_huffmans_per_chunk: i32,
    send_quantum_crcs: i32,
    max_local_dictionary_size: i32,
    make_long_range_matcher: i32,
    match_table_size_log2: i32,
}

const FUZZ_SAFE_YES: i32 = 1;
const CHECK_CRC_NO: i32 = 0;
const VERBOSITY_NONE: i32 = 0;
const THREAD_PHASE_UNTHREADED: i32 = 3;

pub const COMPRESSOR_KRAKEN: i32 = 8;
pub const LEVEL_OPTIMAL2: i32 = 6;

const SEEK_CHUNK_LEN: i32 = 0x40000;

pub struct Oodle {
    decompress: DecompressFn,
    get_decode_buffer_size: GetDecodeBufferSizeFn,
    compress: CompressFn,
    get_compressed_buffer_size_needed: GetCompressedBufferSizeNeededFn,
    compress_options_get_default: CompressOptionsGetDefaultFn,
    _lib: Library,
}

#[derive(Debug, thiserror::Error)]
pub enum OodleError {
    #[error(
        "could not load Oodle from '{path}': {source}. It ships with the game — copy it from \
         'ELDEN RING/Game/oo2core_6_win64.dll', or point SSC_OODLE_DLL_PATH at one."
    )]
    Load {
        path: String,
        #[source]
        source: libloading::Error,
    },

    #[error("'{0}' is missing from the Oodle DLL — is this really oo2core_6_win64.dll?")]
    MissingSymbol(&'static str),

    #[error(
        "OodleLZ_Decompress produced {got} bytes, expected {expected}. A 0 means it rejected \
         the input outright — usually a wrong slice offset, not a corrupt file."
    )]
    ShortDecode { got: i64, expected: usize },

    #[error("OodleLZ_CompressOptions_GetDefault returned null for compressor {compressor}, level {level}")]
    NoCompressOptions { compressor: i32, level: i32 },

    #[error("OodleLZ_Compress returned {got}; anything <= 0 means it refused the input")]
    CompressFailed { got: i64 },
}

impl Oodle {
    pub fn load(path: &Path) -> Result<Self, OodleError> {
        let lib = unsafe { Library::new(path) }.map_err(|source| OodleError::Load {path: path.display().to_string(), source})?;
            let (decompress, get_decode_buffer_size) = unsafe {
            let d = lib
                .get::<DecompressFn>(b"OodleLZ_Decompress\0")
                .map_err(|_| OodleError::MissingSymbol("OodleLZ_Decompress"))?;
            let g = lib
                .get::<GetDecodeBufferSizeFn>(b"OodleLZ_GetDecodeBufferSize\0")
                .map_err(|_| OodleError::MissingSymbol("OodleLZ_GetDecodeBufferSize"))?;
            (*d, *g)
        };

        // SAFETY: same contract as above — signatures transcribed from Oodle26.cs against
        // these exact exports, and `*sym` copies the pointer out so it stops borrowing `lib`.
        let (compress, get_compressed_buffer_size_needed, compress_options_get_default) = unsafe {
            let c = lib
                .get::<CompressFn>(b"OodleLZ_Compress\0")
                .map_err(|_| OodleError::MissingSymbol("OodleLZ_Compress"))?;
            let n = lib
                .get::<GetCompressedBufferSizeNeededFn>(b"OodleLZ_GetCompressedBufferSizeNeeded\0")
                .map_err(|_| OodleError::MissingSymbol("OodleLZ_GetCompressedBufferSizeNeeded"))?;
            let o = lib
                .get::<CompressOptionsGetDefaultFn>(b"OodleLZ_CompressOptions_GetDefault\0")
                .map_err(|_| OodleError::MissingSymbol("OodleLZ_CompressOptions_GetDefault"))?;
            (*c, *n, *o)
        };

        Ok(Self {
            decompress,
            get_decode_buffer_size,
            compress,
            get_compressed_buffer_size_needed,
            compress_options_get_default,
            _lib: lib,
        })

    }

    pub fn compress(
        &self,
        src: &[u8],
        compressor: i32,
        level: i32,
    ) -> Result<Vec<u8>, OodleError> {
        // SAFETY: the DLL owns this pointer and keeps it valid for the process lifetime;
        // we only read through it, immediately, and keep a copy.
        let defaults = unsafe { (self.compress_options_get_default)(compressor, level) };
        if defaults.is_null() {
            return Err(OodleError::NoCompressOptions { compressor, level });
        }
        let mut options = unsafe { *defaults };
        options.seek_chunk_reset = 1;
        options.seek_chunk_len = SEEK_CHUNK_LEN;

        // Compressed output can exceed the input on incompressible data, so the DLL is asked
        // for the worst case rather than assuming `src.len()` is enough.
        let capacity = unsafe { (self.get_compressed_buffer_size_needed)(src.len() as i64) };
        let mut buf = vec![0u8; capacity.max(src.len() as i64) as usize];

        // SAFETY: `src` and `buf` are valid for the lengths passed, `options` outlives the
        // call, and every optional pointer is null with a matching zero size.
        let written = unsafe {
            (self.compress)(
                compressor,
                src.as_ptr(),
                src.len() as i64,
                buf.as_mut_ptr(),
                level,
                &options,
                null_mut(),
                null_mut(),
                null_mut(),
                0,
            )
        };

        if written <= 0 {
            return Err(OodleError::CompressFailed { got: written });
        }
        buf.truncate(written as usize);
        Ok(buf)
    }
        pub fn decompress(&self, src: &[u8], raw_len: usize) -> Result<Vec<u8>, OodleError> {
        // Oodle writes *past* the end of the decoded data as it works, so the buffer must
        // be larger than the result — allocating exactly `raw_len` corrupts the heap. Ask
        // the DLL how much slack it wants rather than hardcoding a formula that could
        // change between Oodle versions. `Oodle26.Decompress` does the same, then truncates.
        let padded = unsafe { (self.get_decode_buffer_size)(raw_len as i64, FUZZ_SAFE_YES) };
        let mut buf = vec![0u8; padded.max(raw_len as i64) as usize];

        // SAFETY: `src` and `buf` are valid for the lengths passed, and every optional
        // pointer is null with a matching zero size — exactly how `Oodle26.Decompress`
        // calls it.
        let written = unsafe {
            (self.decompress)(
                src.as_ptr(),
                src.len() as i64,
                buf.as_mut_ptr(),
                raw_len as i64,
                FUZZ_SAFE_YES,
                CHECK_CRC_NO,
                VERBOSITY_NONE,
                null_mut(),
                0,
                null_mut(),
                null_mut(),
                null_mut(),
                0,
                THREAD_PHASE_UNTHREADED,
            )
        };

        if written != raw_len as i64 {
            return Err(OodleError::ShortDecode {
                got: written,
                expected: raw_len,
            });
        }

        buf.truncate(raw_len);
        Ok(buf)
    }
}


#[cfg(test)]
mod tests {
    use super::*;
    use crate::locate::{game_item_msgbnd, locate_oodle_dll};

    #[test]
    #[ignore = "reads an external game install — see docs/known-offsets.md"]
    fn decompresses_the_item_msgbnd_payload() {
        let dll = locate_oodle_dll().expect("oo2core_6_win64.dll should be found");
        let binder = game_item_msgbnd().expect("item msgbnd should be found");
        let bytes = std::fs::read(&binder).expect("binder should be readable");

        assert_eq!(&bytes[0x00..0x04], b"DCX\0", "not a DCX container");
        assert_eq!(&bytes[0x28..0x2C], b"KRAK", "not Oodle-compressed");

        // Big-endian: DCX headers are BE regardless of the payload's own endianness.
        let uncompressed = u32::from_be_bytes(bytes[0x1C..0x20].try_into().unwrap()) as usize;
        let compressed = u32::from_be_bytes(bytes[0x20..0x24].try_into().unwrap()) as usize;
        eprintln!("uncompressed {uncompressed}  compressed {compressed}");

        let oodle = Oodle::load(&dll).expect("Oodle should load");
        let payload = oodle
            .decompress(&bytes[0x4C..0x4C + compressed], uncompressed)
            .expect("decompression should succeed");

        assert_eq!(payload.len(), uncompressed, "wrong decompressed length");
        assert_eq!(&payload[0..4], b"BND4", "payload should be a BND4 container");
    }
}
