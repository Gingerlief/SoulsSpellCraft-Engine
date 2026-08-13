// Docs: docs/souls-format/cursor.md

// --- Little-endian byte writer: the counterpart to `ByteReader`, shared by every encoder
// in this crate (`fxr`, `bnd4`, `fmg`). `reserve_*` + `fill_*` mirror
// `BinaryWriterEx.ReserveInt32`/`FillInt32`: each format writes a placeholder for an offset
// or size it cannot know yet, then patches it once the thing it points at exists.

#[derive(Default)]
pub struct ByteWriter {
    buf: Vec<u8>,
}

#[allow(dead_code)]
impl ByteWriter {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn pos(&self) -> usize {
        self.buf.len()
    }
    pub fn into_vec(self) -> Vec<u8> {
        self.buf
    }
    pub fn write_bytes(&mut self, b: &[u8]) {
        self.buf.extend_from_slice(b);
    }
    pub fn write_u8(&mut self, v: u8) {
        self.buf.push(v);
    }
    pub fn write_bool(&mut self, v: bool) {
        self.buf.push(v as u8);
    }
    pub fn write_i16(&mut self, v: i16) {
        self.write_bytes(&v.to_le_bytes());
    }
    pub fn write_u16(&mut self, v: u16) {
        self.write_bytes(&v.to_le_bytes());
    }
    pub fn write_i32(&mut self, v: i32) {
        self.write_bytes(&v.to_le_bytes());
    }
    pub fn write_u32(&mut self, v: u32) {
        self.write_bytes(&v.to_le_bytes());
    }
    pub fn write_i64(&mut self, v: i64) {
        self.write_bytes(&v.to_le_bytes());
    }
    pub fn write_f32(&mut self, v: f32) {
        self.write_bytes(&v.to_le_bytes());
    }
    /// NUL-terminated UTF-16LE, mirroring `WriteUTF16(s, true)`.
    pub fn write_utf16(&mut self, s: &str) {
        for unit in s.encode_utf16() {
            self.write_bytes(&unit.to_le_bytes());
        }
        self.write_bytes(&0u16.to_le_bytes());
    }
    /// Fixed-width ASCII, NUL-padded to `len`. Mirrors `WriteFixStr`.
    pub fn write_fixstr(&mut self, s: &str, len: usize) {
        let bytes = s.as_bytes();
        for i in 0..len {
            self.buf.push(bytes.get(i).copied().unwrap_or(0));
        }
    }
    pub fn reserve_i32(&mut self) -> usize {
        let at = self.pos();
        self.write_i32(0);
        at
    }
    pub fn reserve_i64(&mut self) -> usize {
        let at = self.pos();
        self.write_i64(0);
        at
    }
    pub fn fill_i32(&mut self, at: usize, v: i32) {
        self.buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    pub fn fill_u32(&mut self, at: usize, v: u32) {
        self.buf[at..at + 4].copy_from_slice(&v.to_le_bytes());
    }
    pub fn fill_i64(&mut self, at: usize, v: i64) {
        self.buf[at..at + 8].copy_from_slice(&v.to_le_bytes());
    }
    /// Zero-pads to the next multiple of `align`, like `BinaryWriterEx.Pad`.
    pub fn pad(&mut self, align: usize) {
        while !self.buf.len().is_multiple_of(align) {
            self.buf.push(0);
        }
    }
}


pub struct ByteReader<'a> {
    data: &'a [u8],
    pos: usize,
}

// `len`, `seek` and `read_u64` have no caller today. Kept rather than deleted: this is the
// primitive every new format reader in this crate starts from, and absolute seeks plus
// 64-bit reads are table stakes for the BND4/PARAM-family layouts still unimplemented
// (docs/planning/recommendations.md). Deleting them trades a warning today for
// re-deriving them later. Reviewed deliberately — not an oversight.
#[allow(dead_code)]
impl<'a> ByteReader<'a> {
    pub fn new(data: &'a [u8]) -> Self {
        Self { data, pos: 0 }
    }

    pub fn len(&self) -> usize {
        self.data.len()
    }

    pub fn position(&self) -> usize {
        self.pos
    }

    pub fn seek(&mut self, pos: usize) {
        self.pos = pos;
    }

    pub fn skip(&mut self, n: usize) {
        self.pos += n;
    }

    pub fn step_in<T>(&mut self, offset: usize, f: impl FnOnce(&mut Self) -> T) -> T {
        let saved = self.pos;
        self.pos = offset;
        let result = f(self);
        self.pos = saved;
        result
    }

    pub fn read_bytes(&mut self, n: usize) -> &'a [u8] {
        let s = &self.data[self.pos..self.pos + n];
        self.pos += n;
        s
    }

    pub fn read_u8(&mut self) -> u8 {
        let v = self.data[self.pos];
        self.pos += 1;
        v
    }

    pub fn read_bool(&mut self) -> bool {
        self.read_u8() != 0
    }

    pub fn read_i8(&mut self) -> i8 {
        self.read_u8() as i8
    }

    pub fn read_u16(&mut self) -> u16 {
        u16::from_le_bytes(self.read_bytes(2).try_into().unwrap())
    }

    pub fn read_i16(&mut self) -> i16 {
        i16::from_le_bytes(self.read_bytes(2).try_into().unwrap())
    }

    pub fn read_u32(&mut self) -> u32 {
        u32::from_le_bytes(self.read_bytes(4).try_into().unwrap())
    }

    pub fn read_i32(&mut self) -> i32 {
        i32::from_le_bytes(self.read_bytes(4).try_into().unwrap())
    }

    pub fn read_u64(&mut self) -> u64 {
        u64::from_le_bytes(self.read_bytes(8).try_into().unwrap())
    }

    pub fn read_i64(&mut self) -> i64 {
        i64::from_le_bytes(self.read_bytes(8).try_into().unwrap())
    }

    pub fn read_f32(&mut self) -> f32 {
        f32::from_le_bytes(self.read_bytes(4).try_into().unwrap())
    }

    pub fn read_f64(&mut self) -> f64 {
        f64::from_le_bytes(self.read_bytes(8).try_into().unwrap())
    }

    pub fn read_fixstr(&mut self, len: usize) -> String {
        let bytes = self.read_bytes(len);
        let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
        String::from_utf8_lossy(&bytes[..end]).into_owned()
    }

    pub fn read_fixstrw(&mut self, len_bytes: usize) -> String {
        let bytes = self.read_bytes(len_bytes);
        let units: Vec<u16> = bytes
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&u| u != 0)
            .collect();
        String::from_utf16_lossy(&units)
    }

    // --- Random-access getters: read at an absolute offset without moving `pos`. ---
    // Mirror `BinaryReaderEx.Get*(offset)` calls in SoulsFormatsNEXT, used for peeking at
    // fields (e.g. a row's DataOffset) or resolving name-table strings.

    pub fn get_i64_at(&self, offset: usize) -> i64 {
        i64::from_le_bytes(self.data[offset..offset + 8].try_into().unwrap())
    }

    pub fn get_u32_at(&self, offset: usize) -> u32 {
        u32::from_le_bytes(self.data[offset..offset + 4].try_into().unwrap())
    }

    pub fn get_i32_at(&self, offset: usize) -> i32 {
        i32::from_le_bytes(self.data[offset..offset + 4].try_into().unwrap())
    }

    pub fn get_f32_at(&self, offset: usize) -> f32 {
        f32::from_le_bytes(self.data[offset..offset + 4].try_into().unwrap())
    }

    pub fn get_ascii_at(&self, offset: usize) -> String {
        let rest = &self.data[offset..];
        let end = rest.iter().position(|&b| b == 0).unwrap_or(rest.len());
        String::from_utf8_lossy(&rest[..end]).into_owned()
    }

    pub fn get_utf16_at(&self, offset: usize) -> String {
        let rest = &self.data[offset..];
        let units: Vec<u16> = rest
            .chunks_exact(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .take_while(|&u| u != 0)
            .collect();
        String::from_utf16_lossy(&units)
    }
}
