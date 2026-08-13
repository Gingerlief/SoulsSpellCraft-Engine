// Docs: docs/souls-format/bnd4.md

use crate::cursor::ByteWriter;
use crate::regulation::{
    bnd4_file_header_size, BinderEntry, Bnd4Header, FILEFLAG_COMPRESSED, FMT_COMPRESSION,
    FMT_IDS, FMT_LONG_OFFSETS, FMT_NAMES1, FMT_NAMES2,
};

#[derive(Debug, thiserror::Error)]
pub enum Bnd4WriteError {
    #[error("big-endian BND4 writing is not supported")]
    BigEndian,

    #[error("non-Unicode (Shift-JIS) binder names are not supported")]
    NotUnicode,

    #[error("entry '{0}' is flagged compressed; per-entry compression is not supported")]
    CompressedEntry(String),

    #[error("could not find a hash group count for {0} entries")]
    NoHashGroupCount(usize),
}

fn file_flags_byte(flags: u8, bit_big_endian: bool) -> u8 {
    if bit_big_endian {
        flags
    } else {
        flags.reverse_bits()
    }
}

// --- hash table --------------------------------------------------------------------------

fn path_hash(name: &str) -> u32 {
    let lowered = name.to_lowercase().replace('\\', "/");
    let hashable = if lowered.starts_with('/') {
        lowered
    } else {
        format!("/{lowered}")
    };
    hashable
        .encode_utf16()
        .fold(0u32, |acc, unit| acc.wrapping_mul(37).wrapping_add(unit as u32))
}

fn is_prime(candidate: u32) -> bool {
    if candidate < 2 {
        return false;
    }
    if candidate == 2 {
        return true;
    }
    if candidate % 2 == 0 {
        return false;
    }
    let mut i: u64 = 3;
    while i * i <= candidate as u64 {
        if candidate as u64 % i == 0 {
            return false;
        }
        i += 2;
    }
    true
}

fn write_hash_table(w: &mut ByteWriter, names: &[&str]) -> Result<(), Bnd4WriteError> {
    let group_count = ((names.len() as u32 / 7)..=100_000)
        .find(|&p| is_prime(p))
        .ok_or(Bnd4WriteError::NoHashGroupCount(names.len()))?;

    let mut buckets: Vec<Vec<(u32, i32)>> = vec![Vec::new(); group_count as usize];
    for (index, name) in names.iter().enumerate() {
        let hash = path_hash(name);
        buckets[(hash % group_count) as usize].push((hash, index as i32));
    }
    for bucket in &mut buckets {
        bucket.sort_by_key(|&(hash, _)| hash);
    }

    let hashes_offset_at = w.reserve_i64();
    w.write_u32(group_count);
    w.write_u8(0x10); // hash table header size
    w.write_u8(8); // bucket size
    w.write_u8(8); // hash size
    w.write_u8(0);

    let mut index = 0i32;
    for bucket in &buckets {
        w.write_i32(bucket.len() as i32); // length first, then index
        w.write_i32(index);
        index += bucket.len() as i32;
    }

    let hashes_offset = w.pos() as i64;
    w.fill_i64(hashes_offset_at, hashes_offset);
    for bucket in &buckets {
        for &(hash, entry_index) in bucket {
            w.write_u32(hash);
            w.write_i32(entry_index);
        }
    }

    Ok(())
}

// --- the writer --------------------------------------------------------------------------

pub fn write_bnd4(
    header: &Bnd4Header,
    entries: &[BinderEntry],
) -> Result<Vec<u8>, Bnd4WriteError> {
    if header.big_endian {
        return Err(Bnd4WriteError::BigEndian);
    }
    if !header.unicode {
        return Err(Bnd4WriteError::NotUnicode);
    }
    if let Some(e) = entries.iter().find(|e| e.flags & FILEFLAG_COMPRESSED != 0) {
        return Err(Bnd4WriteError::CompressedEntry(e.name.clone()));
    }

    let format = header.format;
    let has_compression = format & FMT_COMPRESSION != 0;
    let has_long_offsets = format & FMT_LONG_OFFSETS != 0;
    let has_ids = format & FMT_IDS != 0;
    let has_names = format & (FMT_NAMES1 | FMT_NAMES2) != 0;

    let mut w = ByteWriter::new();

    // --- fixed 0x40 header ---
    w.write_bytes(b"BND4");
    w.write_u8(header.unk04 as u8);
    w.write_u8(header.unk05 as u8);
    w.write_u8(0);
    w.write_u8(0);
    w.write_u8(0);
    w.write_u8(header.big_endian as u8);
    w.write_u8(!header.bit_big_endian as u8);
    w.write_u8(0);

    w.write_i32(entries.len() as i32);
    w.write_i64(0x40);
    w.write_fixstr(&header.version, 8);
    w.write_i64(bnd4_file_header_size(format) as i64);
    let headers_end_at = w.reserve_i64();

    w.write_u8(header.unicode as u8);
    w.write_u8(header.format_raw);
    w.write_u8(header.extended);
    w.write_u8(0);

    w.write_i32(0);
    let hash_table_offset_at = w.reserve_i64();
    debug_assert_eq!(w.pos(), 0x40);

    // --- file headers, with every derived field reserved ---
    struct Reserved {
        compressed_size: usize,
        uncompressed_size: Option<usize>,
        data_offset: usize,
        name_offset: Option<usize>,
    }
    let mut reserved = Vec::with_capacity(entries.len());

    for entry in entries {
        w.write_u8(file_flags_byte(entry.flags, header.bit_big_endian));
        w.write_u8(0);
        w.write_u8(0);
        w.write_u8(0);
        w.write_i32(-1);

        let compressed_size = w.reserve_i64();
        let uncompressed_size = has_compression.then(|| w.reserve_i64());
        let data_offset = if has_long_offsets {
            w.reserve_i64()
        } else {
            w.reserve_i32()
        };
        if has_ids {
            w.write_i32(entry.id);
        }
        let name_offset = has_names.then(|| w.reserve_i32());
        if format == FMT_NAMES1 {
            w.write_i32(entry.id);
            w.write_i32(0);
        }

        reserved.push(Reserved {
            compressed_size,
            uncompressed_size,
            data_offset,
            name_offset,
        });
    }

    // --- name table ---
    for (entry, slot) in entries.iter().zip(&reserved) {
        if let Some(at) = slot.name_offset {
            let here = w.pos() as i32;
            w.fill_i32(at, here);
            w.write_utf16(&entry.name);
        }
    }

    // --- hash table, when the container carries one ---
    if header.extended == 4 {
        w.pad(8);
        let here = w.pos() as i64;
        w.fill_i64(hash_table_offset_at, here);
        let names: Vec<&str> = entries.iter().map(|e| e.name.as_str()).collect();
        write_hash_table(&mut w, &names)?;
    } else {
        w.fill_i64(hash_table_offset_at, 0);
    }

    let headers_end = w.pos() as i64;
    w.fill_i64(headers_end_at, headers_end);

    // --- data, each non-empty entry aligned to 0x10 ---
    for (entry, slot) in entries.iter().zip(&reserved) {
        if !entry.bytes.is_empty() {
            w.pad(0x10);
        }
        let data_offset = w.pos();
        w.write_bytes(&entry.bytes);

        let size = entry.bytes.len() as i64;
        w.fill_i64(slot.compressed_size, size);
        if let Some(at) = slot.uncompressed_size {
            w.fill_i64(at, size);
        }
        if has_long_offsets {
            w.fill_i64(slot.data_offset, data_offset as i64);
        } else {
            w.fill_u32(slot.data_offset, data_offset as u32);
        }
    }

    Ok(w.into_vec())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_hash_matches_the_csharp_fold() {
        // '/' == 0x2F, then 'a' == 0x61: 0*37 + 47 = 47, 47*37 + 97 = 1836.
        assert_eq!(path_hash("a"), 1836);
        // Backslashes normalise to forward slashes, and case is folded.
        assert_eq!(path_hash(r"\A"), path_hash("/a"));
    }

    #[test]
    fn finds_the_group_count_soulsformats_would() {
        // 78 entries / 7 == 11, which is already prime.
        assert!(is_prime(11));
        assert_eq!((78u32 / 7..=100_000).find(|&p| is_prime(p)), Some(11));
    }

    #[test]
    #[ignore = "reads an external game install — see docs/known-offsets.md"]
    fn round_trips_the_item_msgbnd() {
        let original = crate::test_support::msgbnd_payload();

        let (header, entries) =
            crate::regulation::parse_bnd4_full(&original).expect("should parse as BND4");
        let rewritten = write_bnd4(&header, &entries).expect("should write");

        eprintln!(
            "original {} bytes, rewritten {} bytes",
            original.len(),
            rewritten.len()
        );

        if let Some(at) = original.iter().zip(&rewritten).position(|(a, b)| a != b) {
            let end_o = (at + 16).min(original.len());
            let end_r = (at + 16).min(rewritten.len());
            panic!(
                "first difference at {at:#x}\n  original  {:02x?}\n  rewritten {:02x?}",
                &original[at..end_o],
                &rewritten[at..end_r]
            );
        }
        assert_eq!(
            original.len(),
            rewritten.len(),
            "identical up to the shorter length, but the lengths differ"
        );
    }

    #[test]
    #[ignore = "reads an external game install — see docs/known-offsets.md"]
    fn round_trips_the_whole_dcx_container() {
        let oodle = crate::test_support::oodle();
        let raw = std::fs::read(crate::test_support::msgbnd_path())
            .expect("binder should be readable");

        let (header, entries) = crate::regulation::parse_bnd4_full(
            &crate::dcx::unwrap_krak(&raw, &oodle).expect("should unwrap"),
        )
        .expect("should parse as BND4");

        let rebuilt = crate::dcx::wrap_krak(
            &write_bnd4(&header, &entries).expect("should write"),
            &oodle,
            crate::oodle::LEVEL_OPTIMAL2,
        )
        .expect("should wrap");

        let (header2, entries2) = crate::regulation::parse_bnd4_full(
            &crate::dcx::unwrap_krak(&rebuilt, &oodle).expect("should unwrap ours"),
        )
        .expect("ours should parse as BND4");

        eprintln!("{} bytes -> {} bytes", raw.len(), rebuilt.len());
        assert_eq!(entries.len(), entries2.len(), "entry count changed");
        assert_eq!(header.format_raw, header2.format_raw);
        assert_eq!(header.extended, header2.extended);
        assert_eq!(header.version, header2.version);

        for (a, b) in entries.iter().zip(&entries2) {
            assert_eq!(a.name, b.name, "entry name changed");
            assert_eq!(a.id, b.id, "entry id changed for {}", a.name);
            assert_eq!(a.flags, b.flags, "entry flags changed for {}", a.name);
            assert!(a.bytes == b.bytes, "entry bytes changed for {}", a.name);
        }
    }
}

#[cfg(test)]
mod sfx {
    use super::*;

    /// An SFX binder round-trips byte-for-byte, same as the item msgbnd.
    ///
    /// This is what makes WitchyBND replaceable: `.ffxbnd.dcx` is the same DCX_KRAK + BND4
    /// stack (format 46, extended 4) with no per-entry compression, so unpacking is
    /// `unwrap_krak` + `parse_bnd4` + writing each entry out, and packing is the reverse.
    /// Two orders of magnitude larger than the msgbnd, and the writer does not care.
    #[test]
    #[ignore = "reads a 357 MB SFX binder from the working copy"]
    fn round_trips_the_sfx_binder() {
        let path = crate::locate::patch_dir().join("sfx/sfxbnd_commoneffects.ffxbnd.dcx");
        if !path.is_file() {
            eprintln!("skipping: no binder at {}", path.display());
            return;
        }
        let oodle = crate::test_support::oodle();
        let raw = std::fs::read(&path).expect("readable");
        let payload = crate::dcx::unwrap_krak(&raw, &oodle).expect("should unwrap DCX_KRAK");

        let (header, entries) =
            crate::regulation::parse_bnd4_full(&payload).expect("should parse as BND4");
        assert_eq!(
            entries.iter().filter(|e| e.flags & 1 != 0).count(),
            0,
            "per-entry compression would need decompressing each entry too"
        );

        let rewritten = write_bnd4(&header, &entries).expect("should write");
        if let Some(at) = payload.iter().zip(&rewritten).position(|(a, b)| a != b) {
            panic!("first difference at {at:#x}");
        }
        assert_eq!(payload.len(), rewritten.len(), "lengths differ");
        eprintln!(
            "{} entries, byte-identical ({} bytes)",
            entries.len(),
            rewritten.len()
        );
    }
}