// Docs: docs/souls-format/fmg.md

use crate::cursor::{ByteReader, ByteWriter};

const VERSION_WIDE: u8 =2;
const GROUPS_OFFSET: usize = 0x28;
const GROUP_SIZE: usize = 16;

#[derive(Debug, thiserror::Error)]
pub enum FmgError {
    #[error("FMG has an MD5 prefix, which this reader does not handle")]
    Md5Prefixed,

    #[error("big-endian FMGs are not supported")]
    BigEndian,

    #[error("unsupported FMG version {0}; expected {VERSION_WIDE} (DarkSouls3/Elden Ring)")]
    UnsupportedVersion(u8),

    #[error("non-Unicode FMGs are not supported")]
    NotUnicode,
}

struct Header {
    group_count: usize,
    string_offsets_offset: usize,
}

fn read_header(br: &mut ByteReader) -> Result<Header, FmgError> {
    if br.read_u8() != 0 {
        return Err(FmgError::Md5Prefixed);
    }
    if br.read_bool() {
        return Err(FmgError::BigEndian);
    }
    let version = br.read_u8();
    if version != VERSION_WIDE {
        return Err(FmgError::UnsupportedVersion(version));
    }
    br.skip(1); // zero

    br.skip(4); // file size — the slice already bounds us

    if !br.read_bool() {
        return Err(FmgError::NotUnicode);
    }
    br.skip(3); // three zero bytes

    let group_count = br.read_i32() as usize;
    br.skip(4); // string count — the groups already say how many ids there are
    br.skip(4); // 0xFF, the wide-format marker

    let string_offsets_offset = br.read_i64() as usize;
    br.skip(8); // asserted zero in the C#

    debug_assert_eq!(br.position(), GROUPS_OFFSET);

    Ok(Header {
        group_count,
        string_offsets_offset,
    })
}
pub fn parse(data: &[u8]) -> Result<Vec<(i32, String)>, FmgError> {
    Ok(parse_entries(data)?
        .into_iter()
        .filter_map(|(id, text)| text.map(|t| (id, t)))
        .collect())
}

pub fn parse_entries(data: &[u8]) -> Result<Vec<(i32, Option<String>)>, FmgError> {
    let mut br = ByteReader::new(data);
    let header = read_header(&mut br)?;

    let mut entries = Vec::new();
    for i in 0..header.group_count {
        br.seek(GROUPS_OFFSET + i * GROUP_SIZE);
        let offset_index = br.read_i32() as usize;
        let first_id = br.read_i32();
        let last_id = br.read_i32();

        for j in 0..=(last_id - first_id) {
            let slot = header.string_offsets_offset + (offset_index + j as usize) * 8;
            let string_offset = br.get_i64_at(slot);
            let text = (string_offset > 0).then(|| br.get_utf16_at(string_offset as usize));
            entries.push((first_id + j, text));
        }
    }

    Ok(entries)
}

pub fn write(entries: &[(i32, Option<String>)]) -> Vec<u8> {
    let mut sorted: Vec<(i32, Option<&str>)> = entries
        .iter()
        .map(|(id, text)| (*id, text.as_deref()))
        .collect();
    sorted.sort_by_key(|(id, _)| *id);

    let mut w = ByteWriter::new();

    w.write_u8(0);
    w.write_u8(0); // big-endian flag
    w.write_u8(VERSION_WIDE);
    w.write_u8(0);

    let file_size_at = w.reserve_i32();
    w.write_u8(1); // unicode
    w.write_u8(0);
    w.write_u8(0);
    w.write_u8(0);

    let group_count_at = w.reserve_i32();
    w.write_i32(sorted.len() as i32);
    w.write_i32(0xFF); // wide-format marker

    let string_offsets_at = w.reserve_i64();
    w.write_i64(0);
    debug_assert_eq!(w.pos(), GROUPS_OFFSET);

    // One group per run of consecutive ids. `offset_index` is the run's first index in the
    // sorted list, which is what makes offsets for a group contiguous.
    let mut group_count = 0i32;
    let mut i = 0usize;
    while i < sorted.len() {
        w.write_i32(i as i32);
        w.write_i32(sorted[i].0);
        while i + 1 < sorted.len() && sorted[i + 1].0 == sorted[i].0 + 1 {
            i += 1;
        }
        w.write_i32(sorted[i].0);
        w.write_i32(0); // wide padding
        group_count += 1;
        i += 1;
    }
    w.fill_i32(group_count_at, group_count);

    let string_offsets = w.pos() as i64;
    w.fill_i64(string_offsets_at, string_offsets);
    let slots: Vec<usize> = (0..sorted.len()).map(|_| w.reserve_i64()).collect();

    for (slot, (_, text)) in slots.iter().zip(&sorted) {
        match text {
            Some(t) => {
                let here = w.pos() as i64;
                w.fill_i64(*slot, here);
                w.write_utf16(t);
            }
            // Offset 0 is the format's "no string here", not an empty one.
            None => w.fill_i64(*slot, 0),
        }
    }

    let size = w.pos() as i32;
    w.fill_i32(file_size_at, size);
    w.into_vec()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::test_support::msgbnd_payload as payload;

    #[test]
    #[ignore = "reads an external game install — see docs/known-offsets.md"]
    fn reads_goods_names() {
        let payload = payload();
        let entries = crate::regulation::parse_bnd4(&payload).expect("should parse as BND4");

        let goods = entries
            .iter()
            .find(|e| e.leaf_name().eq_ignore_ascii_case("GoodsName.fmg"))
            .expect("GoodsName.fmg should be in the binder");

        let names = parse(&goods.bytes).expect("GoodsName.fmg should parse");
        eprintln!("{} entries", names.len());
        let test_names = names.iter().filter(|(i, _)| *i >=4000);
        for (id, text) in test_names.take(5) {
            eprintln!("  {id:>6}  {text}");
        }

        let pebble = names.iter().find(|(id, _)| *id == 4000);
        assert_eq!(pebble.map(|(_, t)| t.as_str()), Some("Glintstone Pebble"));
    }

    #[test]
    #[ignore = "reads an external game install — see docs/known-offsets.md"]
    fn round_trips_goods_name_fmg() {
        let payload = payload();
        let entries = crate::regulation::parse_bnd4(&payload).expect("should parse as BND4");
        let goods = entries
            .iter()
            .find(|e| e.leaf_name().eq_ignore_ascii_case("GoodsName.fmg"))
            .expect("GoodsName.fmg should be in the binder");

        let parsed = parse_entries(&goods.bytes).expect("should parse");
        let rewritten = write(&parsed);

        let with_text = parsed.iter().filter(|(_, t)| t.is_some()).count();
        eprintln!(
            "{} ids ({} with text), {} bytes -> {} bytes",
            parsed.len(),
            with_text,
            goods.bytes.len(),
            rewritten.len()
        );

        if let Some(at) = goods.bytes.iter().zip(&rewritten).position(|(a, b)| a != b) {
            let end_o = (at + 16).min(goods.bytes.len());
            let end_r = (at + 16).min(rewritten.len());
            panic!(
                "first difference at {at:#x}\n  original  {:02x?}\n  rewritten {:02x?}",
                &goods.bytes[at..end_o],
                &rewritten[at..end_r]
            );
        }
        assert_eq!(goods.bytes.len(), rewritten.len(), "lengths differ");
    }
}