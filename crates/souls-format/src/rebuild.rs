// Docs: docs/souls-format/rebuild.md

use crate::regulation::RawParamTable;

#[derive(Debug, thiserror::Error)]
pub enum RebuildError {
    #[error(
        "table shape not supported by the rebuild path (format2d={format2d:#04x}, \
         unnamed_rows={unnamed_rows}) — only format2d=0x85 with named rows is proven"
    )]
    UnsupportedShape { format2d: u8, unnamed_rows: bool },
    #[error(
        "rows in this table do not all share one name_offset — the rebuild path only \
         supports the shared-empty-name convention observed on Magic.param/EquipParamGoods.param"
    )]
    NonUniformRowNames,
    #[error("row {id} already exists in this table")]
    RowAlreadyExists { id: i32 },
    #[error("row {id}: encoded to {got} bytes but the table's slot size is {want}")]
    RowSizeMismatch { id: i32, got: usize, want: usize },
    #[error(
        "row name_offset {name_offset} does not point at an empty string within the entry's \
         trailing bytes — refusing to insert rather than silently drop the stored name (see \
         docs/known-offsets.md, \"Row insertion / rebuild path\")"
    )]
    NameOffsetNotEmpty { name_offset: i64 },
}

const ROW_DIR_ENTRY_SIZE: usize = 24; // id(4) + pad(4) + data_offset(8) + name_offset(8)
const HEADER_SIZE: usize = 0x40;
const EMPTY_NAME_TAIL: [u8; 4] = [0, 0, 0, 0];

pub(crate) fn check_shape(table: &RawParamTable) -> Result<(), RebuildError> {
    if table.format2d != 0x85 || table.unnamed_rows {
        return Err(RebuildError::UnsupportedShape {
            format2d: table.format2d,
            unnamed_rows: table.unnamed_rows,
        });
    }
    if let Some(first) = table.rows.first() {
        if table
            .rows
            .iter()
            .any(|r| r.name_offset != first.name_offset)
        {
            return Err(RebuildError::NonUniformRowNames);
        }
    }
    Ok(())
}

pub fn verify_name_offset_is_empty(
    entry_bytes: &[u8],
    name_offset: i64,
) -> Result<(), RebuildError> {
    let Ok(offset) = usize::try_from(name_offset) else {
        return Err(RebuildError::NameOffsetNotEmpty { name_offset });
    };
    let points_at_empty_string = offset < entry_bytes.len()
        && entry_bytes[offset] == 0
        && entry_bytes.len() - offset <= EMPTY_NAME_TAIL.len();
    if points_at_empty_string {
        Ok(())
    } else {
        Err(RebuildError::NameOffsetNotEmpty { name_offset })
    }
}

pub fn rebuild_param_entry(table: &RawParamTable) -> Result<Vec<u8>, RebuildError> {
    check_shape(table)?;

    let mut rows: Vec<&crate::regulation::RawRow> = table.rows.iter().collect();
    rows.sort_by_key(|r| r.id);

    let row_count = rows.len() as u16;
    let detected_size = table.detected_size as usize;
    let rows_start = HEADER_SIZE;
    let dir_bytes = rows.len() * ROW_DIR_ENTRY_SIZE;
    let payload_start = rows_start + dir_bytes;
    let payload_bytes: usize = rows.iter().map(|r| r.bytes.len()).sum();
    let string_table_start = payload_start + payload_bytes;

    let type_name_offset = string_table_start as i64;
    let empty_name_offset = (string_table_start + table.param_type.len() + 1) as i64;
    let data_start_header = payload_start as i64;

    let mut out = Vec::with_capacity(string_table_start + table.param_type.len() + 5);

    // Header, 0x40 bytes.
    out.extend_from_slice(&(type_name_offset as u32).to_le_bytes()); // 0x00 stringsOffset
    out.extend_from_slice(&[0u8, 0u8]); // 0x04 assert-zero (P_LONG_DATA_OFFSET branch)
    out.extend_from_slice(&table.unk06.to_le_bytes()); // 0x06 Unk06, preserved verbatim
    out.extend_from_slice(&table.data_version.to_le_bytes()); // 0x08 data_version
    out.extend_from_slice(&row_count.to_le_bytes()); // 0x0A row_count
    out.extend_from_slice(&[0u8; 4]); // 0x0C assert-zero (P_OFFSET_PARAM_TYPE branch)
    out.extend_from_slice(&type_name_offset.to_le_bytes()); // 0x10 param_type_offset
    out.extend_from_slice(&[0u8; 0x14]); // 0x18 assert-pattern zero block
    out.push(0x00); // 0x2C BigEndian marker
    out.push(table.format2d); // 0x2D
    out.push(table.format2e); // 0x2E
    out.push(table.paramdef_format_version); // 0x2F
    out.extend_from_slice(&data_start_header.to_le_bytes()); // 0x30 data_start_header
    out.extend_from_slice(&[0u8; 8]); // 0x38 assert-zero
    debug_assert_eq!(out.len(), HEADER_SIZE);

    // Row directory, sorted by id, 24 bytes each. data_offset is filled in below once the
    // payload layout is known.
    for row in &rows {
        out.extend_from_slice(&row.id.to_le_bytes());
        out.extend_from_slice(&[0u8; 4]); // pad
        out.extend_from_slice(&0i64.to_le_bytes()); // placeholder
        out.extend_from_slice(&empty_name_offset.to_le_bytes());
    }
    let mut offset = payload_start;
    for (i, row) in rows.iter().enumerate() {
        let entry_pos = HEADER_SIZE + i * ROW_DIR_ENTRY_SIZE + 8;
        out[entry_pos..entry_pos + 8].copy_from_slice(&(offset as i64).to_le_bytes());
        offset += row.bytes.len();
    }

    // Row payloads, sorted by id — same order as the directory. This is load-bearing: the
    // real file already has payloads in id order, which is the only reason a zero-edit
    // rebuild can be byte-identical. Do not append new rows' payloads out of id order.
    for row in &rows {
        if row.bytes.len() != detected_size {
            return Err(RebuildError::RowSizeMismatch {
                id: row.id,
                got: row.bytes.len(),
                want: detected_size,
            });
        }
        out.extend_from_slice(&row.bytes);
    }

    // String table: type name + null, then the shared empty name.
    out.extend_from_slice(table.param_type.as_bytes());
    out.push(0x00);
    out.extend_from_slice(&EMPTY_NAME_TAIL);

    Ok(out)
}

pub fn insert_row(table: &mut RawParamTable, id: i32, bytes: Vec<u8>) -> Result<(), RebuildError> {
    check_shape(table)?;

    if table.rows.iter().any(|r| r.id == id) {
        return Err(RebuildError::RowAlreadyExists { id });
    }
    let want = table.detected_size as usize;
    if bytes.len() != want {
        return Err(RebuildError::RowSizeMismatch {
            id,
            got: bytes.len(),
            want,
        });
    }

    let shared_name_offset = table.rows.first().map(|r| r.name_offset).unwrap_or(-1);

    table.rows.push(crate::regulation::RawRow {
        id,
        bytes,
        data_offset: 0, // recomputed by rebuild_param_entry
        name_offset: shared_name_offset,
    });
    table.rows.sort_by_key(|r| r.id);

    Ok(())
}
