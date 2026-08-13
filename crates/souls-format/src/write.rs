// Docs: docs/souls-format/write.md

use aes::cipher::{block_padding::NoPadding, BlockEncryptMut, KeyIvInit};

use crate::paramdef::{encode_row, ParamRow, Paramdef};
use crate::regulation::{RegulationError, ER_REGULATION_KEY};

pub const DEFAULT_ZSTD_LEVEL: i32 = 21;

pub const ZSTD_LEVEL_ENV: &str = "SSC_ZSTD_LEVEL";

fn zstd_level_from_env() -> Option<i32> {
    let raw = std::env::var(ZSTD_LEVEL_ENV).ok()?;
    let level: i32 = raw.trim().parse().ok()?;
    (1..=22).contains(&level).then_some(level)
}

const DCX_HEADER_LEN: usize = 0x4C;
const DCX_COMPRESSED_SIZE: std::ops::Range<usize> = 0x20..0x24;
const DCX_UNCOMPRESSED_SIZE: std::ops::Range<usize> = 0x1C..0x20;
const DCX_ZSTD_LEVEL: usize = 0x30;
const AES_BLOCK: usize = 16;

const ZSTD_WINDOW_LOG: u32 = 16;

fn compress_like_soulsformats(data: &[u8], level: i32) -> Result<Vec<u8>, WriteError> {
    use zstd::stream::raw::CParameter;

    let mut compressor =
        zstd::bulk::Compressor::new(level).map_err(|e| WriteError::Zstd(e.to_string()))?;
    compressor
        .set_parameter(CParameter::WindowLog(ZSTD_WINDOW_LOG))
        .map_err(|e| WriteError::Zstd(format!("setting windowLog: {e}")))?;
    compressor
        .set_parameter(CParameter::ContentSizeFlag(false))
        .map_err(|e| WriteError::Zstd(format!("setting contentSizeFlag: {e}")))?;
    compressor
        .compress(data)
        .map_err(|e| WriteError::Zstd(e.to_string()))
}

#[derive(Debug, thiserror::Error)]
pub enum WriteError {
    #[error(transparent)]
    Regulation(#[from] RegulationError),
    #[error(transparent)]
    Paramdef(#[from] crate::paramdef::ParamDefError),
    #[error("no BND4 entry matching '{0}'")]
    NoSuchEntry(String),
    #[error("no row {row_id} in {entry}")]
    NoSuchRow { entry: String, row_id: i64 },
    #[error(
        "row {row_id} of {entry} encoded to {got} bytes but its slot is {want} — refusing to \
         write, since anything else would overwrite the following row"
    )]
    RowSizeMismatch {
        entry: String,
        row_id: i64,
        got: usize,
        want: usize,
    },
    #[error("zstd compression failed: {0}")]
    Zstd(String),
    #[error(transparent)]
    Rebuild(#[from] crate::rebuild::RebuildError),
}

fn align_up_16(n: usize) -> usize {
    n.div_ceil(16) * 16
}

fn find_entry_index(
    entries: &[crate::regulation::BinderEntry],
    suffix: &str,
) -> Result<usize, WriteError> {
    let suffix_lower = suffix.to_ascii_lowercase();
    entries
        .iter()
        .position(|e| e.name.to_ascii_lowercase().ends_with(&suffix_lower))
        .ok_or_else(|| WriteError::NoSuchEntry(suffix.to_string()))
}

pub struct RegulationWriter {
    dcx_header: [u8; DCX_HEADER_LEN],
    payload: Vec<u8>,
    patches: usize,
    zstd_level: i32,
}

impl RegulationWriter {
    pub fn open(encrypted: &[u8]) -> Result<Self, WriteError> {
        let decrypted = crate::regulation::decrypt_er_regulation(encrypted);
        if decrypted.len() < DCX_HEADER_LEN {
            return Err(RegulationError::NotDcx.into());
        }
        let mut dcx_header = [0u8; DCX_HEADER_LEN];
        dcx_header.copy_from_slice(&decrypted[..DCX_HEADER_LEN]);
        // A level outside zstd's range means the byte is not what we think it is; fall back
        // rather than hand a nonsense level to the compressor.
        let declared = dcx_header[DCX_ZSTD_LEVEL] as i32;
        let from_header = if (1..=22).contains(&declared) {
            declared
        } else {
            DEFAULT_ZSTD_LEVEL
        };
        // The env override exists because level 21 — what the shipped regulation declares —
        // costs ~22s per write on a 54 MB payload where level 9 costs ~2s, and that lands
        // squarely in the edit/relaunch loop. `finish` writes the level byte back to match
        // whatever is used here, so a lowered level leaves the header and frame in
        // agreement rather than self-contradictory.
        let zstd_level = zstd_level_from_env().unwrap_or(from_header);
        let payload = crate::regulation::decrypt_and_unwrap_regulation(encrypted)?;
        Ok(RegulationWriter {
            zstd_level,
            dcx_header,
            payload,
            patches: 0,
        })
    }

    pub fn payload(&self) -> &[u8] {
        &self.payload
    }

    pub fn patch_count(&self) -> usize {
        self.patches
    }

    pub fn zstd_level(&self) -> i32 {
        self.zstd_level
    }

    pub fn with_zstd_level(mut self, level: i32) -> Self {
        self.zstd_level = level;
        self
    }

    pub fn patch_row(
        &mut self,
        entry_suffix: &str,
        paramdef: &Paramdef,
        row: &ParamRow,
    ) -> Result<(), WriteError> {
        let bytes = encode_row(paramdef, row)?;
        self.patch_row_bytes(entry_suffix, row.id, &bytes)
    }

    pub fn patch_row_bytes(
        &mut self,
        entry_suffix: &str,
        row_id: i64,
        bytes: &[u8],
    ) -> Result<(), WriteError> {
        let entries = crate::regulation::parse_bnd4(&self.payload)?;
        let entry_index = find_entry_index(&entries, entry_suffix)?;
        let entry = &entries[entry_index];

        let table = crate::regulation::parse_param_table(&entry.name, &entry.bytes)?;
        let raw = table
            .rows
            .iter()
            .find(|r| r.id as i64 == row_id)
            .ok_or_else(|| WriteError::NoSuchRow {
                entry: entry.name.clone(),
                row_id,
            })?;

        if bytes.len() != raw.bytes.len() {
            return Err(WriteError::RowSizeMismatch {
                entry: entry.name.clone(),
                row_id,
                got: bytes.len(),
                want: raw.bytes.len(),
            });
        }

        // The one write. Absolute position of the row inside the decompressed container.
        let at = entry.data_offset + raw.data_offset;
        self.payload[at..at + bytes.len()].copy_from_slice(bytes);
        self.patches += 1;
        Ok(())
    }

    pub fn insert_row(
        &mut self,
        entry_suffix: &str,
        paramdef: &Paramdef,
        row: &ParamRow,
    ) -> Result<(), WriteError> {
        let bytes = encode_row(paramdef, row)?;
        self.insert_row_bytes(entry_suffix, row.id, &bytes)
    }

    pub fn insert_row_bytes(
        &mut self,
        entry_suffix: &str,
        row_id: i64,
        bytes: &[u8],
    ) -> Result<(), WriteError> {
        let entries = crate::regulation::parse_bnd4(&self.payload)?;
        let modified_index = find_entry_index(&entries, entry_suffix)?;
        let modified = &entries[modified_index];

        let mut table = crate::regulation::parse_param_table(&modified.name, &modified.bytes)?;

        // Guard against `rebuild_param_entry` silently dropping a genuine row name: it never
        // reads a row's stored `name_offset` back, always writing a freshly computed
        // empty-name offset/tail instead (see `rebuild.rs`'s module doc). `check_shape`'s
        // `NonUniformRowNames` check only proves every row *shares* one `name_offset`; it does
        // not prove that shared value actually points at an empty string. This must run here,
        // before insertion mutates `table`, using the entry's original bytes — a check inside
        // `rebuild_param_entry` itself would be too late, since the stored offsets are already
        // stale by then.
        crate::rebuild::check_shape(&table)?;
        if let Some(first) = table.rows.first() {
            crate::rebuild::verify_name_offset_is_empty(&modified.bytes, first.name_offset)?;
        }

        crate::rebuild::insert_row(&mut table, row_id as i32, bytes.to_vec())?;
        let new_entry_bytes = crate::rebuild::rebuild_param_entry(&table)?;

        let old_len = modified.bytes.len();
        let new_len = new_entry_bytes.len();
        let old_aligned = align_up_16(old_len);
        let new_aligned = align_up_16(new_len);
        let delta = new_aligned as i64 - old_aligned as i64;

        // Header region: everything before the modified entry's data. Unchanged in size —
        // patch only the modified entry's own compressed_size/uncompressed_size, and every
        // later entry's data_offset, in place.
        let mut header_region = self.payload[..modified.data_offset].to_vec();
        let header_start = 0x40 + modified_index * 0x24;
        header_region[header_start + 8..header_start + 16]
            .copy_from_slice(&(new_len as i64).to_le_bytes()); // compressed_size
        header_region[header_start + 16..header_start + 24]
            .copy_from_slice(&(new_len as i64).to_le_bytes()); // uncompressed_size
        for i in (modified_index + 1)..entries.len() {
            let pos = 0x40 + i * 0x24 + 24;
            let old_offset = u32::from_le_bytes(header_region[pos..pos + 4].try_into().unwrap());
            let new_offset = (old_offset as i64 + delta) as u32;
            header_region[pos..pos + 4].copy_from_slice(&new_offset.to_le_bytes());
        }

        let mut new_payload = header_region;
        new_payload.extend_from_slice(&new_entry_bytes);
        let pad = align_up_16(new_payload.len()) - new_payload.len();
        new_payload.resize(new_payload.len() + pad, 0);

        // Everything from the next entry's original start to the end of the file is
        // untouched content, just relocated later — copy it as one contiguous block.
        let tail_start = if modified_index + 1 < entries.len() {
            entries[modified_index + 1].data_offset
        } else {
            self.payload.len()
        };
        new_payload.extend_from_slice(&self.payload[tail_start..]);

        self.payload = new_payload;
        self.patches += 1;
        Ok(())
    }

    pub fn finish(&self) -> Result<Vec<u8>, WriteError> {
        let compressed = compress_like_soulsformats(&self.payload, self.zstd_level)?;

        let mut framed = Vec::with_capacity(DCX_HEADER_LEN + compressed.len() + AES_BLOCK);
        framed.extend_from_slice(&self.dcx_header);
        framed[DCX_COMPRESSED_SIZE].copy_from_slice(&(compressed.len() as u32).to_be_bytes());
        // A row-overwriting splice never changes the payload length; an insertion does, since
        // it grows one entry and shifts the rest. Reading it from `self.payload` rather than
        // the source header keeps this correct for both.
        framed[DCX_UNCOMPRESSED_SIZE].copy_from_slice(&(self.payload.len() as u32).to_be_bytes());
        // Keep the declared level honest. Reusing the header verbatim previously left it
        // claiming 21 while the frame was level 9 — a file that describes itself wrongly,
        // which is worth fixing whether or not the game reads this byte.
        framed[DCX_ZSTD_LEVEL] = self.zstd_level as u8;
        framed.extend_from_slice(&compressed);

        // CBC with NoPadding needs a whole number of blocks. The original file pads with
        // zeroes for exactly this reason — 4 of them, which is how we know the scheme —
        // and the DCX header's own size field is what tells the reader where the frame
        // ends, so trailing zeroes are ignored on the way back in.
        let pad = (AES_BLOCK - framed.len() % AES_BLOCK) % AES_BLOCK;
        framed.resize(framed.len() + pad, 0);

        // The IV is 16 zero bytes in the shipped file, and it is prepended to the
        // ciphertext rather than derived. Reusing it keeps output diffable against the
        // original; generating a fresh one would be equally valid CBC and gratuitously
        // change every byte.
        let iv = [0u8; AES_BLOCK];
        let mut out = iv.to_vec();
        let encryptor = cbc::Encryptor::<aes::Aes256>::new(&ER_REGULATION_KEY.into(), &iv.into());
        let len = framed.len();
        let ciphertext = encryptor
            .encrypt_padded_mut::<NoPadding>(&mut framed, len)
            .expect("length is block-aligned above");
        out.extend_from_slice(ciphertext);
        Ok(out)
    }
}
