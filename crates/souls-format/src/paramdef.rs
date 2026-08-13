// Docs: docs/souls-format/paramdef.md

use std::collections::BTreeMap;
use std::path::Path;

use crate::cursor::ByteReader;

#[derive(Debug, thiserror::Error)]
pub enum ParamDefError {
    #[error("failed to read paramdef '{path}': {source}")]
    Io {
        path: String,
        #[source]
        source: std::io::Error,
    },
    #[error("paramdef xml error: {0}")]
    Xml(String),
    #[error("invalid paramdef {param_type}: bits would be lost before +0x{offset:x} in row {row_id} (bit_value={bit_value:#x})")]
    OrphanedBits {
        param_type: String,
        offset: usize,
        row_id: i64,
        bit_value: u64,
    },
    #[error("bit size 0 is not supported (param {param_type}, row {row_id})")]
    UnsupportedBitSizeZero { param_type: String, row_id: i64 },
    #[error("bit size {bit_size} too large for limit {limit} (param {param_type}, row {row_id})")]
    BitSizeTooLarge {
        param_type: String,
        row_id: i64,
        bit_size: i32,
        limit: u32,
    },
    #[error("row {row_id} of {param_type} has no field '{field}' to encode")]
    EncodeFieldMissing {
        param_type: String,
        row_id: i64,
        field: String,
    },
    #[error(
        "row {row_id} of {param_type}: field '{field}' is {found}, but the paramdef says {expected}"
    )]
    EncodeFieldTypeMismatch {
        param_type: String,
        row_id: i64,
        field: String,
        expected: &'static str,
        found: &'static str,
    },
    #[error(
        "{param_type} repeats field names, so row {row_id} cannot be re-encoded faithfully \
         (decoding already dropped the earlier occurrences)"
    )]
    EncodeAmbiguousDef { param_type: String, row_id: i64 },
    #[error(
        "row {row_id} of {param_type}: field '{field}' needs {needed} bytes but only {capacity} are reserved"
    )]
    EncodeValueTooLong {
        param_type: String,
        row_id: i64,
        field: String,
        needed: usize,
        capacity: usize,
    },
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DefType {
    S8,
    U8,
    S16,
    U16,
    S32,
    U32,
    B32,
    F32,
    Angle32,
    F64,
    Dummy8,
    FixStr,
    FixStrW,
}

impl DefType {
    fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "s8" => Self::S8,
            "u8" => Self::U8,
            "s16" => Self::S16,
            "u16" => Self::U16,
            "s32" => Self::S32,
            "u32" => Self::U32,
            "b32" => Self::B32,
            "f32" => Self::F32,
            "angle32" => Self::Angle32,
            "f64" => Self::F64,
            "dummy8" => Self::Dummy8,
            "fixstr" => Self::FixStr,
            "fixstrW" => Self::FixStrW,
            _ => return None,
        })
    }

    fn is_bit_type(self) -> bool {
        matches!(
            self,
            Self::S8 | Self::U8 | Self::S16 | Self::U16 | Self::S32 | Self::U32 | Self::Dummy8
        )
    }

    fn is_array_type(self) -> bool {
        matches!(self, Self::U8 | Self::Dummy8 | Self::FixStr | Self::FixStrW)
    }

    fn is_signed_bit_type(self) -> bool {
        matches!(self, Self::S8 | Self::S16 | Self::S32)
    }

    fn bit_limit(self) -> Option<u32> {
        match self {
            Self::S8 | Self::U8 | Self::Dummy8 => Some(8),
            Self::S16 | Self::U16 => Some(16),
            Self::S32 | Self::U32 => Some(32),
            _ => None,
        }
    }
}

pub struct Field {
    pub internal_name: String,
    pub display_type: DefType,
    pub array_length: i32,
    pub bit_size: i32,
}

impl Field {
    fn parse_def(def_attr: &str) -> Result<Field, ParamDefError> {
        let def_attr = def_attr.trim();
        let mut parts = def_attr.split_whitespace();
        let type_str = parts
            .next()
            .ok_or_else(|| ParamDefError::Xml(format!("empty Def attribute: '{def_attr}'")))?;
        let name_part = parts.next().ok_or_else(|| {
            ParamDefError::Xml(format!("Def attribute missing a name: '{def_attr}'"))
        })?;

        let display_type = DefType::parse(type_str)
            .ok_or_else(|| ParamDefError::Xml(format!("unknown field type '{type_str}'")))?;

        let (internal_name, bit_size, array_length) =
            if display_type.is_bit_type() && name_part.rfind(':').is_some() {
                let idx = name_part.rfind(':').unwrap();
                let size: i32 = name_part[idx + 1..]
                    .parse()
                    .map_err(|_| ParamDefError::Xml(format!("bad bit size in '{name_part}'")))?;
                (name_part[..idx].to_string(), size, 1)
            } else if display_type.is_array_type() {
                parse_array_suffix(name_part)
            } else {
                (name_part.to_string(), -1, 1)
            };

        Ok(Field {
            internal_name,
            display_type,
            array_length,
            bit_size,
        })
    }
}

fn parse_array_suffix(name_part: &str) -> (String, i32, i32) {
    if let (Some(open), Some(close)) = (name_part.find('['), name_part.find(']')) {
        if close > open {
            if let Ok(len) = name_part[open + 1..close].parse::<i32>() {
                return (name_part[..open].to_string(), -1, len);
            }
        }
    }
    (name_part.to_string(), -1, 1)
}

pub struct Paramdef {
    pub param_type: String,
    pub data_version: i16,
    pub format_version: i16,
    pub fields: Vec<Field>,
    pub has_duplicate_field_names: bool,
}

impl Paramdef {
    pub fn describes_bytes(&self) -> usize {
        self.fields
            .iter()
            .filter(|f| f.bit_size == -1)
            .map(|f| match f.display_type {
                DefType::FixStrW => f.array_length as usize * 2,
                DefType::FixStr | DefType::Dummy8 | DefType::S8 | DefType::U8 => {
                    f.array_length as usize
                }
                DefType::S16 | DefType::U16 => 2,
                DefType::F64 => 8,
                _ => 4,
            })
            .sum::<usize>()
            + packed_word_bytes(&self.fields)
    }
}

fn packed_word_bytes(fields: &[Field]) -> usize {
    let (mut total, mut open): (usize, Option<(u32, i32)>) = (0, None);
    for f in fields {
        if !f.display_type.is_bit_type() || f.bit_size == -1 {
            open = None;
            continue;
        }
        let limit = f.display_type.bit_limit().unwrap_or(32);
        let new_group = match open {
            None => true,
            Some((w, used)) => w != limit || used + f.bit_size > limit as i32,
        };
        if new_group {
            total += (limit / 8) as usize;
            open = Some((limit, f.bit_size));
        } else if let Some((_, used)) = open.as_mut() {
            *used += f.bit_size;
        }
    }
    total
}

fn child_text<'a, 'input>(node: roxmltree::Node<'a, 'input>, tag: &str) -> Option<&'a str> {
    node.children()
        .find(|c| c.has_tag_name(tag))
        .and_then(|c| c.text())
}

impl Paramdef {
    pub fn from_xml_str(xml: &str) -> Result<Paramdef, ParamDefError> {
        let doc = roxmltree::Document::parse(xml).map_err(|e| ParamDefError::Xml(e.to_string()))?;
        let root = doc.root_element();

        let param_type = child_text(root, "ParamType")
            .ok_or_else(|| ParamDefError::Xml("missing <ParamType>".to_string()))?
            .trim()
            .to_string();
        let data_version: i16 = child_text(root, "DataVersion")
            .ok_or_else(|| ParamDefError::Xml("missing <DataVersion>".to_string()))?
            .trim()
            .parse()
            .map_err(|_| ParamDefError::Xml("invalid <DataVersion>".to_string()))?;
        let format_version: i16 = child_text(root, "FormatVersion")
            .ok_or_else(|| ParamDefError::Xml("missing <FormatVersion>".to_string()))?
            .trim()
            .parse()
            .map_err(|_| ParamDefError::Xml("invalid <FormatVersion>".to_string()))?;

        let fields_node = root
            .children()
            .find(|c| c.has_tag_name("Fields"))
            .ok_or_else(|| ParamDefError::Xml("missing <Fields>".to_string()))?;

        let mut fields = Vec::new();
        for field_node in fields_node.children().filter(|c| c.has_tag_name("Field")) {
            let def_attr = field_node
                .attribute("Def")
                .ok_or_else(|| ParamDefError::Xml("<Field> missing Def attribute".to_string()))?;
            fields.push(Field::parse_def(def_attr)?);
        }

        let mut seen = std::collections::HashSet::new();
        let has_duplicate_field_names =
            !fields.iter().all(|f| seen.insert(f.internal_name.as_str()));

        Ok(Paramdef {
            param_type,
            data_version,
            format_version,
            has_duplicate_field_names,
            fields,
        })
    }

    pub fn load(path: &Path) -> Result<Paramdef, ParamDefError> {
        let xml = std::fs::read_to_string(path).map_err(|source| ParamDefError::Io {
            path: path.display().to_string(),
            source,
        })?;
        Self::from_xml_str(&xml)
    }

    pub fn row_size(&self) -> i64 {
        let mut size: i64 = 0;
        let mut i = 0;
        while i < self.fields.len() {
            let field = &self.fields[i];
            let ty = field.display_type;
            let value_size = value_size(ty);
            if ty.is_array_type() {
                size += value_size * field.array_length as i64;
            } else {
                size += value_size;
            }

            if ty.is_bit_type() && field.bit_size != -1 {
                let mut bit_offset = field.bit_size;
                let bit_limit = ty.bit_limit().unwrap() as i32;
                while i + 1 < self.fields.len() {
                    let next = &self.fields[i + 1];
                    if !next.display_type.is_bit_type()
                        || next.bit_size == -1
                        || next.display_type.bit_limit().unwrap() as i32 != bit_limit
                        || bit_offset + next.bit_size > bit_limit
                    {
                        break;
                    }
                    bit_offset += next.bit_size;
                    i += 1;
                }
            }
            i += 1;
        }
        size
    }
}

fn value_size(ty: DefType) -> i64 {
    match ty {
        DefType::S8 | DefType::U8 | DefType::Dummy8 | DefType::FixStr => 1,
        DefType::S16 | DefType::U16 | DefType::FixStrW => 2,
        DefType::S32 | DefType::U32 | DefType::B32 | DefType::F32 | DefType::Angle32 => 4,
        DefType::F64 => 8,
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct ParamRow {
    pub id: i64,
    pub fields: BTreeMap<String, ParamValue>,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub enum ParamValue {
    I64(i64),
    F32(f32),
    F64(f64),
    Str(String),
    Bytes(Vec<u8>),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FieldError {
    Missing,
    WrongType { found: &'static str },
}

impl std::fmt::Display for FieldError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            FieldError::Missing => write!(f, "field not present on row"),
            FieldError::WrongType { found } => {
                write!(f, "field holds {found}, not the requested type")
            }
        }
    }
}

impl ParamValue {
    pub fn variant_name(&self) -> &'static str {
        match self {
            ParamValue::I64(_) => "I64",
            ParamValue::F32(_) => "F32",
            ParamValue::F64(_) => "F64",
            ParamValue::Str(_) => "Str",
            ParamValue::Bytes(_) => "Bytes",
        }
    }
}

impl ParamRow {
    pub fn get_i64(&self, name: &str) -> Result<i64, FieldError> {
        match self.fields.get(name) {
            Some(ParamValue::I64(v)) => Ok(*v),
            Some(other) => Err(FieldError::WrongType {
                found: other.variant_name(),
            }),
            None => Err(FieldError::Missing),
        }
    }

    pub fn get_f32(&self, name: &str) -> Result<f32, FieldError> {
        match self.fields.get(name) {
            Some(ParamValue::F32(v)) => Ok(*v),
            Some(other) => Err(FieldError::WrongType {
                found: other.variant_name(),
            }),
            None => Err(FieldError::Missing),
        }
    }

    pub fn get_str(&self, name: &str) -> Result<&str, FieldError> {
        match self.fields.get(name) {
            Some(ParamValue::Str(v)) => Ok(v),
            Some(other) => Err(FieldError::WrongType {
                found: other.variant_name(),
            }),
            None => Err(FieldError::Missing),
        }
    }

    pub fn get_bytes(&self, name: &str) -> Result<&[u8], FieldError> {
        match self.fields.get(name) {
            Some(ParamValue::Bytes(v)) => Ok(v),
            Some(other) => Err(FieldError::WrongType {
                found: other.variant_name(),
            }),
            None => Err(FieldError::Missing),
        }
    }

    pub fn as_f64(&self, name: &str) -> Result<f64, FieldError> {
        match self.fields.get(name) {
            Some(ParamValue::I64(v)) => Ok(*v as f64),
            Some(ParamValue::F32(v)) => Ok(*v as f64),
            Some(ParamValue::F64(v)) => Ok(*v),
            Some(other) => Err(FieldError::WrongType {
                found: other.variant_name(),
            }),
            None => Err(FieldError::Missing),
        }
    }

    pub fn set_i64(&mut self, name: &str, value: i64) -> Result<(), FieldError> {
        match self.fields.get_mut(name) {
            Some(ParamValue::I64(v)) => {
                *v = value;
                Ok(())
            }
            Some(other) => Err(FieldError::WrongType {
                found: other.variant_name(),
            }),
            None => Err(FieldError::Missing),
        }
    }

    pub fn set_f32(&mut self, name: &str, value: f32) -> Result<(), FieldError> {
        match self.fields.get_mut(name) {
            Some(ParamValue::F32(v)) => {
                *v = value;
                Ok(())
            }
            Some(other) => Err(FieldError::WrongType {
                found: other.variant_name(),
            }),
            None => Err(FieldError::Missing),
        }
    }
}

pub fn decode_row(paramdef: &Paramdef, row_id: i64, raw: &[u8]) -> Result<ParamRow, ParamDefError> {
    let mut br = ByteReader::new(raw);
    let mut fields = BTreeMap::new();

    let mut bit_offset: i32 = -1;
    let mut bit_limit: u32 = 0;
    let mut bit_value: u64 = 0;

    for field in &paramdef.fields {
        let ty = field.display_type;
        let value: Option<ParamValue> = if ty == DefType::B32 {
            Some(ParamValue::I64(br.read_i32() as i64))
        } else if ty == DefType::F32 || ty == DefType::Angle32 {
            Some(ParamValue::F32(br.read_f32()))
        } else if ty == DefType::F64 {
            Some(ParamValue::F64(br.read_f64()))
        } else if ty == DefType::FixStr {
            Some(ParamValue::Str(br.read_fixstr(field.array_length as usize)))
        } else if ty == DefType::FixStrW {
            Some(ParamValue::Str(
                br.read_fixstrw(field.array_length as usize * 2),
            ))
        } else if ty.is_bit_type() {
            if field.bit_size == -1 {
                Some(match ty {
                    DefType::S8 => ParamValue::I64(br.read_i8() as i64),
                    DefType::U8 => {
                        if field.array_length > 1 {
                            ParamValue::Bytes(br.read_bytes(field.array_length as usize).to_vec())
                        } else {
                            ParamValue::I64(br.read_u8() as i64)
                        }
                    }
                    DefType::S16 => ParamValue::I64(br.read_i16() as i64),
                    DefType::U16 => ParamValue::I64(br.read_u16() as i64),
                    DefType::S32 => ParamValue::I64(br.read_i32() as i64),
                    DefType::U32 => ParamValue::I64(br.read_u32() as i64),
                    DefType::Dummy8 => {
                        ParamValue::Bytes(br.read_bytes(field.array_length as usize).to_vec())
                    }
                    _ => unreachable!(),
                })
            } else {
                None // bitfield path, handled below
            }
        } else {
            unreachable!("unsupported field type in paramdef")
        };

        let value = match value {
            Some(v) => {
                check_orphaned_bits(&paramdef.param_type, row_id, &br, bit_offset, bit_value)?;
                bit_offset = -1;
                v
            }
            None => {
                let limit = ty.bit_limit().expect("bitfield type must have a bit limit");
                if bit_offset == -1
                    || limit != bit_limit
                    || bit_offset + field.bit_size > limit as i32
                {
                    check_orphaned_bits(&paramdef.param_type, row_id, &br, bit_offset, bit_value)?;
                    bit_offset = 0;
                    bit_limit = limit;
                    bit_value = match limit {
                        8 => br.read_u8() as u64,
                        16 => br.read_u16() as u64,
                        32 => br.read_u32() as u64,
                        _ => unreachable!(),
                    };
                }

                if field.bit_size == 0 {
                    return Err(ParamDefError::UnsupportedBitSizeZero {
                        param_type: paramdef.param_type.clone(),
                        row_id,
                    });
                }
                if field.bit_size > bit_limit as i32 {
                    return Err(ParamDefError::BitSizeTooLarge {
                        param_type: paramdef.param_type.clone(),
                        row_id,
                        bit_size: field.bit_size,
                        limit: bit_limit,
                    });
                }

                let left_shift = (64 - field.bit_size - bit_offset) as u32;
                let right_shift = (64 - field.bit_size) as u32;

                let shifted: i64 = if ty.is_signed_bit_type() {
                    ((bit_value as i64) << left_shift) >> right_shift
                } else {
                    ((bit_value << left_shift) >> right_shift) as i64
                };

                bit_offset += field.bit_size;

                match ty {
                    DefType::S8 => ParamValue::I64(shifted as i8 as i64),
                    DefType::U8 => ParamValue::I64(shifted as u8 as i64),
                    DefType::S16 => ParamValue::I64(shifted as i16 as i64),
                    DefType::U16 => ParamValue::I64(shifted as u16 as i64),
                    DefType::S32 => ParamValue::I64(shifted as i32 as i64),
                    DefType::U32 => ParamValue::I64(shifted as u32 as i64),
                    DefType::Dummy8 => ParamValue::I64(shifted as u8 as i64),
                    _ => unreachable!(),
                }
            }
        };

        fields.insert(field.internal_name.clone(), value);
    }

    check_orphaned_bits(&paramdef.param_type, row_id, &br, bit_offset, bit_value)?;

    Ok(ParamRow { id: row_id, fields })
}

pub fn encode_row(paramdef: &Paramdef, row: &ParamRow) -> Result<Vec<u8>, ParamDefError> {
    let mut out: Vec<u8> = Vec::new();
    let row_id = row.id;
    let pt = &paramdef.param_type;

    // Refused, not attempted. With repeated field names the decoded row already lost the
    // earlier occurrences, so every repeated slot would be written with the last value —
    // a row that looks plausible and is wrong. See `Paramdef::has_duplicate_field_names`.
    if paramdef.has_duplicate_field_names {
        return Err(ParamDefError::EncodeAmbiguousDef {
            param_type: pt.clone(),
            row_id,
        });
    }

    // The open packed word, if any: (storage width in bits, bits used so far, value).
    let mut pending: Option<(u32, i32, u64)> = None;

    for field in &paramdef.fields {
        let ty = field.display_type;
        let name = &field.internal_name;

        let get = || -> Result<&ParamValue, ParamDefError> {
            row.fields
                .get(name)
                .ok_or_else(|| ParamDefError::EncodeFieldMissing {
                    param_type: pt.clone(),
                    row_id,
                    field: name.clone(),
                })
        };

        let is_direct = !ty.is_bit_type() || field.bit_size == -1;
        if is_direct {
            // A directly-stored field closes any open packed word — decode_row resets its
            // accumulator on exactly this condition.
            flush(&mut out, &mut pending);
        }

        match ty {
            DefType::B32 => {
                out.extend_from_slice(&(int_of(get()?, pt, row_id, name)? as i32).to_le_bytes())
            }
            DefType::F32 | DefType::Angle32 => {
                out.extend_from_slice(&float_of(get()?, pt, row_id, name)?.to_le_bytes())
            }
            DefType::F64 => {
                out.extend_from_slice(&double_of(get()?, pt, row_id, name)?.to_le_bytes())
            }
            DefType::FixStr => {
                let s = str_of(get()?, pt, row_id, name)?;
                write_fixed(
                    &mut out,
                    s.as_bytes(),
                    field.array_length as usize,
                    pt,
                    row_id,
                    name,
                )?;
            }
            DefType::FixStrW => {
                let s = str_of(get()?, pt, row_id, name)?;
                let units: Vec<u8> = s.encode_utf16().flat_map(|u| u.to_le_bytes()).collect();
                write_fixed(
                    &mut out,
                    &units,
                    field.array_length as usize * 2,
                    pt,
                    row_id,
                    name,
                )?;
            }
            _ if is_direct => {
                let v = get()?;
                match ty {
                    DefType::S8 => out.push(int_of(v, pt, row_id, name)? as i8 as u8),
                    DefType::U8 if field.array_length > 1 => {
                        let b = bytes_of(v, pt, row_id, name)?;
                        write_fixed(&mut out, b, field.array_length as usize, pt, row_id, name)?;
                    }
                    DefType::U8 => out.push(int_of(v, pt, row_id, name)? as u8),
                    DefType::S16 => {
                        out.extend_from_slice(&(int_of(v, pt, row_id, name)? as i16).to_le_bytes())
                    }
                    DefType::U16 => {
                        out.extend_from_slice(&(int_of(v, pt, row_id, name)? as u16).to_le_bytes())
                    }
                    DefType::S32 => {
                        out.extend_from_slice(&(int_of(v, pt, row_id, name)? as i32).to_le_bytes())
                    }
                    DefType::U32 => {
                        out.extend_from_slice(&(int_of(v, pt, row_id, name)? as u32).to_le_bytes())
                    }
                    DefType::Dummy8 => {
                        let b = bytes_of(v, pt, row_id, name)?;
                        write_fixed(&mut out, b, field.array_length as usize, pt, row_id, name)?;
                    }
                    _ => unreachable!("non-bit type handled above"),
                }
            }
            _ => {
                // Packed bitfield.
                let limit = ty.bit_limit().expect("bitfield type must have a bit limit");
                let starts_new_group = match pending {
                    None => true,
                    Some((width, used, _)) => {
                        width != limit || used + field.bit_size > limit as i32
                    }
                };
                if starts_new_group {
                    flush(&mut out, &mut pending);
                    pending = Some((limit, 0, 0));
                }

                if field.bit_size == 0 {
                    return Err(ParamDefError::UnsupportedBitSizeZero {
                        param_type: pt.clone(),
                        row_id,
                    });
                }
                let (width, used, value) = pending.as_mut().expect("just set");
                if field.bit_size > *width as i32 {
                    return Err(ParamDefError::BitSizeTooLarge {
                        param_type: pt.clone(),
                        row_id,
                        bit_size: field.bit_size,
                        limit: *width,
                    });
                }

                let raw = int_of(get()?, pt, row_id, name)? as u64;
                let mask = if field.bit_size >= 64 {
                    u64::MAX
                } else {
                    (1u64 << field.bit_size) - 1
                };
                *value |= (raw & mask) << *used;
                *used += field.bit_size;
            }
        }
    }

    flush(&mut out, &mut pending);
    Ok(out)
}

fn flush(out: &mut Vec<u8>, pending: &mut Option<(u32, i32, u64)>) {
    if let Some((width, _, value)) = pending.take() {
        match width {
            8 => out.push(value as u8),
            16 => out.extend_from_slice(&(value as u16).to_le_bytes()),
            32 => out.extend_from_slice(&(value as u32).to_le_bytes()),
            _ => unreachable!("bit limits are 8/16/32"),
        }
    }
}

fn write_fixed(
    out: &mut Vec<u8>,
    bytes: &[u8],
    capacity: usize,
    param_type: &str,
    row_id: i64,
    field: &str,
) -> Result<(), ParamDefError> {
    if bytes.len() > capacity {
        return Err(ParamDefError::EncodeValueTooLong {
            param_type: param_type.to_string(),
            row_id,
            field: field.to_string(),
            needed: bytes.len(),
            capacity,
        });
    }
    out.extend_from_slice(bytes);
    out.resize(out.len() + (capacity - bytes.len()), 0);
    Ok(())
}

fn mismatch(
    param_type: &str,
    row_id: i64,
    field: &str,
    expected: &'static str,
    found: &'static str,
) -> ParamDefError {
    ParamDefError::EncodeFieldTypeMismatch {
        param_type: param_type.to_string(),
        row_id,
        field: field.to_string(),
        expected,
        found,
    }
}

fn int_of(v: &ParamValue, pt: &str, id: i64, f: &str) -> Result<i64, ParamDefError> {
    match v {
        ParamValue::I64(i) => Ok(*i),
        other => Err(mismatch(pt, id, f, "an integer", other.variant_name())),
    }
}

fn float_of(v: &ParamValue, pt: &str, id: i64, f: &str) -> Result<f32, ParamDefError> {
    match v {
        ParamValue::F32(x) => Ok(*x),
        other => Err(mismatch(pt, id, f, "f32", other.variant_name())),
    }
}

fn double_of(v: &ParamValue, pt: &str, id: i64, f: &str) -> Result<f64, ParamDefError> {
    match v {
        ParamValue::F64(x) => Ok(*x),
        other => Err(mismatch(pt, id, f, "f64", other.variant_name())),
    }
}

fn str_of<'a>(v: &'a ParamValue, pt: &str, id: i64, f: &str) -> Result<&'a str, ParamDefError> {
    match v {
        ParamValue::Str(s) => Ok(s),
        other => Err(mismatch(pt, id, f, "a string", other.variant_name())),
    }
}

fn bytes_of<'a>(v: &'a ParamValue, pt: &str, id: i64, f: &str) -> Result<&'a [u8], ParamDefError> {
    match v {
        ParamValue::Bytes(b) => Ok(b),
        other => Err(mismatch(pt, id, f, "bytes", other.variant_name())),
    }
}

fn check_orphaned_bits(
    param_type: &str,
    row_id: i64,
    br: &ByteReader,
    bit_offset: i32,
    bit_value: u64,
) -> Result<(), ParamDefError> {
    if bit_offset != -1 && (bit_value >> bit_offset as u32) != 0 {
        return Err(ParamDefError::OrphanedBits {
            param_type: param_type.to_string(),
            offset: br.position(),
            row_id,
            bit_value,
        });
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sp_effect_paramdef_path() -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join("../../data/paramdex/ER/Defs/SpEffect.xml")
    }

    #[test]
    fn loads_sp_effect_paramdef() {
        let def = Paramdef::load(&sp_effect_paramdef_path()).expect("should parse");
        assert_eq!(def.param_type, "SP_EFFECT_PARAM_ST");
        assert_eq!(def.data_version, 4);
        // Confirmed via tools/csharp-baseline decode-row (see docs/known-offsets.md).
        assert_eq!(def.fields.len(), 361);
        assert_eq!(def.row_size(), 912);
    }

    #[test]
    fn row_835_reports_orphaned_bits_matching_csharp_baseline() {
        let def = Paramdef::load(&sp_effect_paramdef_path()).expect("should parse");

        let Some(regulation_bin) = crate::locate::game_regulation_bin() else {
            eprintln!("skipping: no regulation.bin found (set SSC_REGULATION_BIN_PATH)");
            return;
        };

        let regulation = crate::regulation::Regulation::open(&regulation_bin)
            .expect("decrypt + BND4 parse should succeed");
        let table = regulation
            .param_table("SpEffectParam.param")
            .expect("row directory should parse");
        let row_835 = table
            .rows
            .iter()
            .find(|r| r.id == 835)
            .expect("row 835 should exist");

        let err = decode_row(&def, 835, &row_835.bytes)
            .expect_err("row 835 is known to have orphaned bits");
        match err {
            ParamDefError::OrphanedBits { offset, .. } => {
                // Confirmed via tools/csharp-baseline decode-row against today's vendored
                // SpEffect.xml: "bits would be lost before +0x354 in row 835" (see
                // docs/known-offsets.md) -- identical offset, live and reproducible today.
                assert_eq!(offset, 0x354);
            }
            other => panic!("expected OrphanedBits, got {other:?}"),
        }
    }
}
