// Docs: docs/souls-format/regulation.md

use std::path::Path;

use crate::cursor::ByteReader;

#[derive(Debug, thiserror::Error)]
pub enum RegulationError {
    #[error("failed to read '{path}': {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("not a BND4 container (bad magic)")]
    NotBnd4,
    #[error("not a DCX container (bad magic)")]
    NotDcx,
    #[error(transparent)]
    Dcx(#[from] crate::dcx::DcxError),
    #[error("unexpected BND4 header size: {0:#x} (expected 0x40)")]
    UnexpectedHeaderSize(i64),
    #[error("unexpected BND4 file-header size: expected {expected:#x}, got {actual:#x} for format {format:#04x}")]
    UnexpectedFileHeaderSize {
        expected: usize,
        actual: i64,
        format: u8,
    },
    #[error("BND4 entry '{0}' is individually compressed — unsupported (confirmed unnecessary for regulation.bin, see docs/known-offsets.md)")]
    CompressedEntryUnsupported(String),
    #[error("could not find a BND4 entry ending with '{0}'")]
    EntryNotFound(String),
    #[error("could not determine row byte size for param table '{0}' (single-row/empty tables unsupported in this spike)")]
    UndetectableRowSize(String),
}

pub(crate) const ER_REGULATION_KEY: [u8; 32] = [
    0x99, 0xBF, 0xFC, 0x36, 0x6A, 0x6B, 0xC8, 0xC6, 0xF5, 0x82, 0x7D, 0x09, 0x36, 0x02, 0xD6, 0x76,
    0xC4, 0x28, 0x92, 0xA0, 0x1C, 0x20, 0x7F, 0xB0, 0x24, 0xD3, 0xAF, 0x4E, 0x49, 0x3F, 0xEF, 0x99,
];

pub fn decrypt_er_regulation(encrypted: &[u8]) -> Vec<u8> {
    use aes::Aes256;
    use cbc::cipher::block_padding::NoPadding;
    use cbc::cipher::{BlockDecryptMut, KeyIvInit};

    type Aes256CbcDec = cbc::Decryptor<Aes256>;

    let iv = &encrypted[0..16];
    let mut buf = encrypted[16..].to_vec();
    let rem = buf.len() % 16;
    if rem != 0 {
        buf.resize(buf.len() + (16 - rem), 0);
    }

    let decryptor = Aes256CbcDec::new_from_slices(&ER_REGULATION_KEY, iv)
        .expect("key and IV are always exactly 32 and 16 bytes");
    let plaintext_len = decryptor
        .decrypt_padded_mut::<NoPadding>(&mut buf)
        .expect("NoPadding decrypt never fails on block-aligned input")
        .len();
    buf.truncate(plaintext_len);
    buf
}

pub fn decrypt_and_unwrap_regulation(encrypted: &[u8]) -> Result<Vec<u8>, RegulationError> {
    let decrypted = decrypt_er_regulation(encrypted);
    if decrypted.len() >= 4 && &decrypted[0..4] == b"BND4" {
        Ok(decrypted)
    } else if decrypted.len() >= 4 && &decrypted[0..4] == b"DCX\0" {
        Ok(crate::dcx::unwrap_zstd(&decrypted)?)
    } else {
        Err(RegulationError::NotBnd4)
    }
}

pub struct BinderEntry {
    pub id: i32,
    pub name: String,
    pub bytes: Vec<u8>,
    pub data_offset: usize,
    pub flags: u8,
}

impl BinderEntry {
    /// The file name alone. Binder entry names are full game paths
    /// (`N:\GR\data\INTERROOT_win64\msg\engUS\GoodsName.fmg`), and only the leaf identifies
    /// what the entry holds — every caller matching an entry wants this, not `name`.
    pub fn leaf_name(&self) -> &str {
        self.name.rsplit(['\\', '/']).next().unwrap_or(&self.name)
    }
}

// --- Binder format flag bits (Binder.Format / Binder.FileFlags in the C# source). ---
// `pub(crate)` so `bnd4`'s writer decides the same field layout the reader did, from one
// definition — two copies of these bits is exactly how a writer silently drifts.
pub(crate) const FMT_IDS: u8 = 0b0000_0010;
pub(crate) const FMT_NAMES1: u8 = 0b0000_0100;
pub(crate) const FMT_NAMES2: u8 = 0b0000_1000;
pub(crate) const FMT_LONG_OFFSETS: u8 = 0b0001_0000;
pub(crate) const FMT_COMPRESSION: u8 = 0b0010_0000;
pub(crate) const FILEFLAG_COMPRESSED: u8 = 0b0000_0001;

pub(crate) fn bnd4_file_header_size(format: u8) -> usize {
    0x10 + if format & FMT_LONG_OFFSETS != 0 { 8 } else { 4 }
        + if format & FMT_COMPRESSION != 0 { 8 } else { 0 }
        + if format & FMT_IDS != 0 { 4 } else { 0 }
        + if format & (FMT_NAMES1 | FMT_NAMES2) != 0 {
            4
        } else {
            0
        }
        + if format == FMT_NAMES1 { 8 } else { 0 }
}

fn read_format_byte(br: &mut ByteReader, bit_big_endian: bool) -> u8 {
    let raw = br.read_u8();
    let reverse = bit_big_endian || ((raw & 1) != 0 && (raw & 0b1000_0000) == 0);
    if reverse {
        raw
    } else {
        raw.reverse_bits()
    }
}

fn read_file_flags(br: &mut ByteReader, bit_big_endian: bool) -> u8 {
    let raw = br.read_u8();
    if bit_big_endian {
        raw
    } else {
        raw.reverse_bits()
    }
}

struct RawFileHeader {
    flags: u8,
    compressed: bool,
    id: i32,
    name: String,
    data_offset: i64,
    compressed_size: i64,
}

fn read_bnd4_file_header(
    br: &mut ByteReader,
    format: u8,
    bit_big_endian: bool,
    unicode: bool,
) -> RawFileHeader {
    let flags = read_file_flags(br, bit_big_endian);
    br.skip(3); // three zero bytes
    br.skip(4); // assert int32 == -1

    let compressed_size = br.read_i64();
    if format & FMT_COMPRESSION != 0 {
        br.skip(8); // uncompressed size -- unused (nothing in regulation.bin is actually compressed)
    }

    let data_offset = if format & FMT_LONG_OFFSETS != 0 {
        br.read_i64()
    } else {
        br.read_u32() as i64
    };

    let id = if format & FMT_IDS != 0 {
        br.read_i32()
    } else {
        -1
    };

    let name = if format & (FMT_NAMES1 | FMT_NAMES2) != 0 {
        let name_offset = br.read_u32() as usize;
        if unicode {
            br.get_utf16_at(name_offset)
        } else {
            br.get_ascii_at(name_offset)
        }
    } else {
        String::new()
    };

    if format == FMT_NAMES1 {
        br.skip(8); // PC-save-file special case, not applicable to regulation.bin
    }

    RawFileHeader {
        flags,
        compressed: flags & FILEFLAG_COMPRESSED != 0,
        id,
        name,
        data_offset,
        compressed_size,
    }
}

#[derive(Debug, Clone)]
pub struct Bnd4Header {
    pub unk04: bool,
    pub unk05: bool,
    pub big_endian: bool,
    pub bit_big_endian: bool,
    pub version: String,
    pub format: u8,
    pub format_raw: u8,
    pub extended: u8,
    pub unicode: bool,
}

pub fn parse_bnd4(data: &[u8]) -> Result<Vec<BinderEntry>, RegulationError> {
    parse_bnd4_full(data).map(|(_, entries)| entries)
}

pub fn parse_bnd4_full(data: &[u8]) -> Result<(Bnd4Header, Vec<BinderEntry>), RegulationError> {
    let mut br = ByteReader::new(data);

    if br.read_bytes(4) != b"BND4" {
        return Err(RegulationError::NotBnd4);
    }

    let unk04 = br.read_bool();
    let unk05 = br.read_bool();
    br.skip(2); // two zero bytes
    br.skip(1); // zero byte
    let big_endian = br.read_bool(); // confirmed false for regulation.bin; LE assumed throughout
    let bit_big_endian = !br.read_bool();
    br.skip(1); // zero byte

    let file_count = br.read_i32() as usize;
    let header_size = br.read_i64();
    if header_size != 0x40 {
        return Err(RegulationError::UnexpectedHeaderSize(header_size));
    }
    let version = br.read_fixstr(8);
    let file_header_size = br.read_i64();
    br.skip(8); // "headers end" (includes hash table), unused

    let unicode = br.read_bool();
    let format_raw = data[br.position()];
    let format = read_format_byte(&mut br, bit_big_endian);
    let extended = br.read_u8();
    br.skip(1); // zero byte
    br.skip(4); // assert int32 == 0

    if extended == 4 {
        br.skip(8); // hash table offset -- file headers follow at a fixed position regardless, no need to chase it
    } else {
        br.skip(8); // assert int64 == 0
    }

    let expected_header_size = bnd4_file_header_size(format);
    if file_header_size as usize != expected_header_size {
        return Err(RegulationError::UnexpectedFileHeaderSize {
            expected: expected_header_size,
            actual: file_header_size,
            format,
        });
    }

    let mut headers = Vec::with_capacity(file_count);
    for _ in 0..file_count {
        headers.push(read_bnd4_file_header(
            &mut br,
            format,
            bit_big_endian,
            unicode,
        ));
    }

    let mut entries = Vec::with_capacity(file_count);
    for h in headers {
        if h.compressed {
            return Err(RegulationError::CompressedEntryUnsupported(h.name));
        }
        let start = h.data_offset as usize;
        let end = start + h.compressed_size as usize;
        entries.push(BinderEntry {
            id: h.id,
            name: h.name,
            bytes: data[start..end].to_vec(),
            data_offset: start,
            flags: h.flags,
        });
    }

    Ok((
        Bnd4Header {
            unk04,
            unk05,
            big_endian,
            bit_big_endian,
            version,
            format,
            format_raw,
            extended,
            unicode,
        },
        entries,
    ))
}

pub struct RawRow {
    pub id: i32,
    pub bytes: Vec<u8>,
    pub data_offset: usize,
    pub name_offset: i64,
}

pub struct RawParamTable {
    pub name: String,
    pub param_type: String,
    pub data_version: i16,
    pub detected_size: i64,
    pub rows: Vec<RawRow>,
    pub format2d: u8,
    pub format2e: u8,
    pub paramdef_format_version: u8,
    pub unk06: i16,
    pub unnamed_rows: bool,
}

// --- PARAM container format flag bits (PARAM.FormatFlags1 / FormatFlags2). ---
const P_FLAG01: u8 = 0b0000_0001;
const P_INT_DATA_OFFSET: u8 = 0b0000_0010;
const P_LONG_DATA_OFFSET: u8 = 0b0000_0100;
const P_OFFSET_PARAM_TYPE: u8 = 0b1000_0000;

pub fn parse_param_table(entry_name: &str, data: &[u8]) -> Result<RawParamTable, RegulationError> {
    // PARAM.cs peeks the BigEndian/Format2D/Format2E/ParamdefFormatVersion bytes at a fixed
    // absolute offset (0x2C) before parsing anything else. That offset is fixed by
    // construction regardless of which flags are set (both the inline-fixstr and the
    // offset+padding encodings of ParamType sum to exactly 0x2C bytes), so reading it directly
    // here is equivalent to the C# peek-then-rewind and simpler.
    let format2d = data[0x2D];
    let format2e = data[0x2E];
    let paramdef_format_version = data[0x2F];

    let mut br = ByteReader::new(data);

    br.skip(4); // stringsOffset (u32) -- unreliable per SoulsFormatsNEXT, only used as a
                // single-row DetectedSize fallback, which this parser doesn't support (see module doc)

    let mut data_start_header: i64 = -1;
    if (format2d & P_FLAG01 != 0 && format2d & P_INT_DATA_OFFSET != 0)
        || format2d & P_LONG_DATA_OFFSET != 0
    {
        br.skip(2); // assert int16 == 0
    } else {
        data_start_header = br.read_u16() as i64;
    }

    let unk06 = br.read_i16();
    let data_version = br.read_i16();
    let row_count = br.read_u16() as usize;

    let param_type = if format2d & P_OFFSET_PARAM_TYPE != 0 {
        br.skip(4); // assert int32 == 0
        let param_type_offset = br.read_i64();
        br.skip(0x14); // assert pattern: 0x14 bytes of 0x00
        if (param_type_offset as usize) < data.len() {
            br.get_ascii_at(param_type_offset as usize)
        } else {
            String::new()
        }
    } else {
        br.read_fixstr(0x20)
    };

    br.skip(4); // BigEndian marker + Format2D + Format2E + ParamdefFormatVersion (already peeked above)

    if format2d & P_FLAG01 != 0 && format2d & P_INT_DATA_OFFSET != 0 {
        data_start_header = br.read_i32() as i64;
        br.skip(12); // 3x assert int32 == 0
    } else if format2d & P_LONG_DATA_OFFSET != 0 {
        data_start_header = br.read_i64();
        br.skip(8); // assert int64 == 0
    }

    let rows_start = br.position();

    // Detect UnnamedRows via row-spacing heuristic, ported as-is from PARAM.cs including its
    // row-header-size constant of 12 (which assumes the *non*-long-offset row format even when
    // LongDataOffset is set) — bug-for-bug faithful, since correctness here is defined as
    // "matches SoulsFormatsNEXT's behavior," not "is the most sensible standalone heuristic."
    let row_data_offset_1 = if format2d & P_LONG_DATA_OFFSET != 0 {
        br.get_i64_at(rows_start + 8)
    } else {
        br.get_u32_at(rows_start + 4) as i64
    };
    let rows_size = row_data_offset_1 - rows_start as i64;
    let mut row_header_size: i64 = 12;
    let mut unnamed_rows = false;
    if rows_size < (row_count as i64) * row_header_size {
        unnamed_rows = true;
        row_header_size = 8;
    }

    let mut headerless_rows = false;
    if data_start_header != -1
        && (rows_start as i64 == data_start_header
            || (rows_start as i64 + row_header_size) > data_start_header)
    {
        headerless_rows = true;
        unnamed_rows = true;
    }

    struct RowHeader {
        id: i32,
        data_offset: i64,
        name_offset: i64,
    }

    let mut rows: Vec<RowHeader> = Vec::with_capacity(row_count);
    let detected_size: i64;

    if headerless_rows {
        detected_size = (data.len() as i64 - rows_start as i64) / row_count as i64;
        let mut row_offset = rows_start as i64;
        for i in 0..row_count {
            rows.push(RowHeader {
                id: i as i32,
                data_offset: row_offset,
                name_offset: -1,
            });
            row_offset += detected_size;
        }
    } else {
        for _ in 0..row_count {
            let id = br.read_i32();
            let (data_offset, name_offset) = if format2d & P_LONG_DATA_OFFSET != 0 {
                br.skip(4); // pad; some games leave garbage here, not asserted upstream either
                let off = br.read_i64();
                let name_off = if !unnamed_rows { br.read_i64() } else { -1 };
                (off, name_off)
            } else {
                let off = br.read_u32() as i64;
                let name_off = if !unnamed_rows {
                    br.read_u32() as i64
                } else {
                    -1
                };
                (off, name_off)
            };
            rows.push(RowHeader {
                id,
                data_offset,
                name_offset,
            });
        }

        detected_size = if rows.len() > 1 {
            rows[1].data_offset - rows[0].data_offset
        } else {
            -1
        };
    }

    if detected_size < 0 {
        return Err(RegulationError::UndetectableRowSize(entry_name.to_string()));
    }

    let raw_rows = rows
        .iter()
        .map(|r| {
            let start = r.data_offset as usize;
            let end = (start + detected_size as usize).min(data.len());
            RawRow {
                id: r.id,
                bytes: data[start..end].to_vec(),
                data_offset: start,
                name_offset: r.name_offset,
            }
        })
        .collect();

    Ok(RawParamTable {
        name: entry_name.to_string(),
        param_type,
        data_version,
        detected_size,
        rows: raw_rows,
        format2d,
        format2e,
        paramdef_format_version,
        unk06,
        unnamed_rows,
    })
}

pub struct Regulation {
    entries: Vec<BinderEntry>,
}

impl Regulation {
    pub fn open(path: &Path) -> Result<Self, RegulationError> {
        let encrypted = std::fs::read(path).map_err(|source| RegulationError::Io {
            path: path.display().to_string(),
            source,
        })?;
        let bnd4_bytes = decrypt_and_unwrap_regulation(&encrypted)?;
        let entries = parse_bnd4(&bnd4_bytes)?;
        Ok(Regulation { entries })
    }

    pub fn entries(&self) -> &[BinderEntry] {
        &self.entries
    }

    pub fn find_entry(&self, name_suffix: &str) -> Option<&BinderEntry> {
        let suffix_lower = name_suffix.to_lowercase();
        self.entries
            .iter()
            .find(|e| e.name.to_lowercase().ends_with(&suffix_lower))
    }

    pub fn param_table(&self, name_suffix: &str) -> Result<RawParamTable, RegulationError> {
        let entry = self
            .find_entry(name_suffix)
            .ok_or_else(|| RegulationError::EntryNotFound(name_suffix.to_string()))?;
        parse_param_table(&entry.name, &entry.bytes)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use crate::locate::game_regulation_bin as find_regulation_bin;

    #[test]
    #[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
    fn sp_effect_param_row_directory_looks_sane() {
        let Some(reg_path) = find_regulation_bin() else {
            eprintln!("skipping: no regulation.bin");
            return;
        };
        let regulation = Regulation::open(&reg_path).expect("decrypt + BND4 parse should succeed");
        let table = regulation
            .param_table("SpEffectParam.param")
            .expect("SpEffectParam row directory should parse");

        // Confirmed via tools/csharp-baseline decode-row against the real file (see
        // docs/known-offsets.md): ParamType=SP_EFFECT_PARAM_ST, DataVersion=4, DetectedSize=912,
        // RowCount=11325.
        assert_eq!(table.param_type, "SP_EFFECT_PARAM_ST");
        assert_eq!(table.data_version, 4);
        assert_eq!(table.detected_size, 912);
        assert_eq!(table.rows.len(), 11325);
        assert!(
            table.rows.iter().any(|r| r.id == 835),
            "row 835 should be present"
        );
    }

    #[test]
    #[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
    fn magic_and_goods_header_fields_match_measured_values() {
        let Some(reg_path) = find_regulation_bin() else {
            eprintln!("skipping: no regulation.bin");
            return;
        };
        let regulation = Regulation::open(&reg_path).expect("regulation should open");

        for (suffix, expected_row_count) in [("Magic.param", 317), ("EquipParamGoods.param", 2326)]
        {
            let table = regulation.param_table(suffix).expect("table should parse");
            assert_eq!(table.format2d, 0x85, "{suffix} format2d");
            assert_eq!(table.format2e, 0x07, "{suffix} format2e");
            assert_eq!(
                table.paramdef_format_version, 0x00,
                "{suffix} paramdef_format_version"
            );
            assert_eq!(table.unk06, 1, "{suffix} unk06");
            assert!(
                !table.unnamed_rows,
                "{suffix} should have per-row name offsets"
            );
            assert_eq!(table.rows.len(), expected_row_count, "{suffix} row count");
        }
    }

    #[test]
    #[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
    fn bullet_and_atk_pc_pass_the_rebuild_shape_gate() {
        let Some(reg_path) = find_regulation_bin() else {
            eprintln!("skipping: no regulation.bin");
            return;
        };
        let regulation = Regulation::open(&reg_path).expect("regulation should open");

        for suffix in ["Bullet.param", "AtkParam_Pc.param"] {
            let table = regulation.param_table(suffix).expect("table should parse");
            assert_eq!(table.format2d, 0x85, "{suffix} format2d — check_shape's actual gate");
            assert!(
                !table.unnamed_rows,
                "{suffix} unnamed_rows — check_shape's actual gate"
            );
            eprintln!(
                "{suffix}: format2e={:#04x} unk06={} rows={} (recorded, not asserted)",
                table.format2e,
                table.unk06,
                table.rows.len()
            );
        }

        // Sanity-check the target-slot-derived ids a later craft would use (target_id * 100_000
        // for the bullet, +1 for the atk — see docs/known-offsets.md) against a few candidate
        // slots. Not a hard assert since the real target slot isn't known until `xtask craft
        // gravity-pebble` allocates one — just a print so a collision is known ahead of time.
        let bullet_table = regulation
            .param_table("Bullet.param")
            .expect("Bullet.param should parse");
        let atk_table = regulation
            .param_table("AtkParam_Pc.param")
            .expect("AtkParam_Pc.param should parse");
        for candidate_slot in [4002i64, 4003, 4004] {
            let bullet_id = candidate_slot * 100_000;
            let atk_id = bullet_id + 1;
            eprintln!(
                "slot {candidate_slot}: derived bullet id {bullet_id} exists = {}, atk id {atk_id} exists = {}",
                bullet_table.rows.iter().any(|r| r.id as i64 == bullet_id),
                atk_table.rows.iter().any(|r| r.id as i64 == atk_id),
            );
        }
    }

    #[test]
    #[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
    fn subtree_tables_pass_the_rebuild_shape_gate() {
        let Some(reg_path) = find_regulation_bin() else {
            eprintln!("skipping: no regulation.bin");
            return;
        };
        let regulation = Regulation::open(&reg_path).expect("regulation should open");

        for suffix in [
            "SpEffectParam.param",
            "SpEffectVfxParam.param",
            "AtkParam_Npc.param",
        ] {
            let table = regulation.param_table(suffix).expect("table should parse");
            assert_eq!(
                table.format2d, 0x85,
                "{suffix} format2d — check_shape's actual gate"
            );
            assert!(
                !table.unnamed_rows,
                "{suffix} unnamed_rows — check_shape's actual gate"
            );
            eprintln!(
                "{suffix}: format2e={:#04x} unk06={} rows={} (recorded, not asserted)",
                table.format2e,
                table.unk06,
                table.rows.len()
            );
        }
    }

    #[test]
    #[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
    fn magic_rows_share_one_name_offset() {
        let Some(reg_path) = find_regulation_bin() else {
            eprintln!("skipping: no regulation.bin");
            return;
        };
        let regulation = Regulation::open(&reg_path).expect("regulation should open");
        let table = regulation
            .param_table("Magic.param")
            .expect("table should parse");

        let first = table.rows[0].name_offset;
        assert!(first > 0, "name_offset should be a real positive offset");
        assert!(
            table.rows.iter().all(|r| r.name_offset == first),
            "every row should share the same name_offset (the empty-name convention)"
        );
    }
}
