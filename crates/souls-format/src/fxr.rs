// Docs: docs/souls-format/fxr.md

use crate::cursor::ByteReader;

#[derive(Debug, thiserror::Error)]
pub enum FxrError {
    #[error("not an FXR file (bad magic)")]
    NotFxr,
    #[error("unsupported FXR version {0} (only version 5 / Sekiro-Elden Ring is implemented)")]
    UnsupportedVersion(u16),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Field {
    Int(i32),
    Float(f32),
}

#[derive(Debug, Clone)]
pub struct StateCondition {
    pub operator_type: u8, // 0=NotEqual 1=Equal 2=GreaterThanOrEqual 3=GreaterThan
    pub unk_modifier: u8,
    pub else_move_to_state_index: i32,
    pub left_operand_type: i16, // -4=Literal -3=External -2=TimeOfDay -1=StateTime
    pub left_value: Option<Field>,
    pub right_operand_type: i16,
    pub right_value: Option<Field>,
}

#[derive(Debug, Clone, Default)]
pub struct State {
    pub conditions: Vec<StateCondition>,
}

#[derive(Debug, Clone, Default)]
pub struct StateMap {
    pub states: Vec<State>,
}

#[derive(Debug, Clone, Default)]
pub struct UnkFieldList {
    pub fields: Vec<Field>,
}

#[derive(Debug, Clone, Default)]
pub struct Property {
    pub property_type: u8,      // 0=Scalar 1=Vector2 2=Vector3 3=Color
    pub interpolation_type: u8, // 0=Zero 1=One 2=Constant 3=Stepped 4=Linear 5=Curve1 6=Curve2 7=UnkAc6
    pub is_loop: bool,
    pub fields: Vec<Field>,
    pub modifiers: Vec<PropertyModifier>,
}

#[derive(Debug, Clone, Default)]
pub struct PropertyModifier {
    pub type_enum_a: u16,
    pub type_enum_b: u32,
    pub fields: Vec<Field>,
    pub properties: Vec<Property>,
}

#[derive(Debug, Clone, Default)]
pub struct Action {
    pub id: i16,
    pub unk02: bool,
    pub unk03: bool,
    pub unk04: i32,
    pub unk_field_lists: Vec<UnkFieldList>,
    pub fields1: Vec<Field>,
    pub fields2: Vec<Field>,
    pub properties1: Vec<Property>,
    pub properties2: Vec<Property>,
}

#[derive(Debug, Clone, Default)]
pub struct Effect {
    pub id: i16,
    pub actions: Vec<Action>,
}

#[derive(Debug, Clone, Default)]
pub struct Container {
    pub id: i16,
    pub actions: Vec<Action>,
    pub effects: Vec<Effect>,
    pub containers: Vec<Container>,
}

#[derive(Debug, Clone)]
pub struct Fxr {
    pub version: u16, // 5 = Sekiro/Elden Ring (only supported version — see module doc)
    pub id: i32,
    pub root_state_map: StateMap,
    pub root_container: Container,
    pub reference_list: Vec<i32>,
    pub external_value_list: Vec<i32>,
    pub unk_blood_enabler: Vec<i32>,
}

// --- Field decode heuristic (ports FXR3.cs's `Field.Read`) ---

#[derive(Clone, Copy)]
enum FieldCtx {
    Heuristic,
    Property {
        interpolation_type: u8,
        property_type: u8,
    },
    Operand {
        operand_type: i16,
    },
}

fn read_field(br: &mut ByteReader, ctx: FieldCtx, index: usize) -> Field {
    let mut is_int = false;
    let mut direct: Option<Field> = None;

    match ctx {
        FieldCtx::Property {
            interpolation_type,
            property_type,
        } => {
            if interpolation_type == 7 {
                // UnkAc6
                if index > 0 && index <= property_type as usize + 1 {
                    is_int = true;
                }
            } else if interpolation_type != 2 {
                // not Constant: first value is a stop count (int), rest are floats
                if index == 0 {
                    is_int = true;
                }
            }
        }
        FieldCtx::Operand { operand_type } => {
            is_int = operand_type == -3; // External
        }
        FieldCtx::Heuristic => {
            let single = br.get_f32_at(br.position());
            // The bound is written as the full f32 expansion of 1e-4 because that is how
            // the `@cccode/fxr` oracle spells it, and this heuristic was validated against
            // that oracle field-for-field. Truncating to `1e-4` is bit-identical in f32
            // but throws away the provenance, so clippy is silenced rather than obeyed.
            #[allow(clippy::excessive_precision)]
            if (9.99999974737875e-5..1_000_000.0).contains(&single)
                || (-1_000_000.0..=-9.99999974737875e-5).contains(&single)
            {
                direct = Some(Field::Float(single));
            } else {
                is_int = true;
            }
        }
    }

    let field = direct.unwrap_or_else(|| {
        if is_int {
            Field::Int(br.get_i32_at(br.position()))
        } else {
            Field::Float(br.get_f32_at(br.position()))
        }
    });
    br.skip(4);
    field
}

fn read_fields(br: &mut ByteReader, count: usize, ctx: FieldCtx) -> Vec<Field> {
    (0..count).map(|i| read_field(br, ctx, i)).collect()
}

fn read_fields_at(br: &mut ByteReader, offset: usize, count: usize, ctx: FieldCtx) -> Vec<Field> {
    br.step_in(offset, |br| read_fields(br, count, ctx))
}

// --- Read ---

fn read_state_condition(br: &mut ByteReader) -> StateCondition {
    let op = br.read_i16();
    let operator_type = (op & 0b11) as u8;
    let unk_modifier = ((op >> 2) & 0b11) as u8;
    br.skip(2); // assert byte 0, assert byte 1
    br.skip(4); // assert int32 0
    let else_move_to_state_index = br.read_i32();
    br.skip(4); // assert int32 0

    let left_operand_type = br.read_i16();
    br.skip(2);
    br.skip(4);
    let has_field1 = br.read_i32() == 1;
    br.skip(4);
    let field_offset1 = br.read_i32() as usize;
    br.skip(20); // 5x assert int32 0

    let right_operand_type = br.read_i16();
    br.skip(2);
    br.skip(4);
    let has_field2 = br.read_i32() == 1;
    br.skip(4);
    let field_offset2 = br.read_i32() as usize;
    br.skip(20);

    let left_value = has_field1.then(|| {
        br.step_in(field_offset1, |br| {
            read_field(
                br,
                FieldCtx::Operand {
                    operand_type: left_operand_type,
                },
                0,
            )
        })
    });
    let right_value = has_field2.then(|| {
        br.step_in(field_offset2, |br| {
            read_field(
                br,
                FieldCtx::Operand {
                    operand_type: right_operand_type,
                },
                0,
            )
        })
    });

    StateCondition {
        operator_type,
        unk_modifier,
        else_move_to_state_index,
        left_operand_type,
        left_value,
        right_operand_type,
        right_value,
    }
}

fn read_state(br: &mut ByteReader) -> State {
    br.skip(4); // assert 0
    let capacity = br.read_i32() as usize;
    let offset = br.read_i32() as usize;
    br.skip(4); // assert 0
    let conditions = br.step_in(offset, |br| {
        (0..capacity).map(|_| read_state_condition(br)).collect()
    });
    State { conditions }
}

fn read_state_map(br: &mut ByteReader) -> StateMap {
    br.skip(4); // assert 0
    let capacity = br.read_i32() as usize;
    let offset = br.read_i32() as usize;
    br.skip(4); // assert 0
    let states = br.step_in(offset, |br| (0..capacity).map(|_| read_state(br)).collect());
    StateMap { states }
}

fn read_unk_field_list(br: &mut ByteReader) -> UnkFieldList {
    let offset = br.read_i32() as usize;
    br.skip(4);
    let count = br.read_i32() as usize;
    br.skip(4);
    UnkFieldList {
        fields: read_fields_at(br, offset, count, FieldCtx::Heuristic),
    }
}

fn read_property(br: &mut ByteReader, conditional: bool) -> Property {
    let type_enum_a = br.read_i16();
    br.skip(2); // assert byte 0, assert byte 1
    let property_type = (type_enum_a & 0b11) as u8;
    let interpolation_type = ((type_enum_a >> 4) & 0b1111) as u8;
    let is_loop = ((type_enum_a >> 12) & 0b1) != 0;
    br.skip(4); // TypeEnumB -- discarded, recomputed on write (see Property::write)

    let count = br.read_i32() as usize;
    br.skip(4);
    let offset = br.read_i32() as usize;
    br.skip(4);

    let modifiers = if conditional {
        Vec::new()
    } else {
        let modifiers_offset = br.read_i32() as usize;
        br.skip(4);
        let modifiers_count = br.read_i32() as usize;
        br.skip(4);
        br.step_in(modifiers_offset, |br| {
            (0..modifiers_count)
                .map(|_| read_property_modifier(br))
                .collect()
        })
    };

    let fields = read_fields_at(
        br,
        offset,
        count,
        FieldCtx::Property {
            interpolation_type,
            property_type,
        },
    );

    Property {
        property_type,
        interpolation_type,
        is_loop,
        fields,
        modifiers,
    }
}

fn read_property_modifier(br: &mut ByteReader) -> PropertyModifier {
    let type_enum_a = br.read_u16();
    br.skip(2);
    let type_enum_b = br.read_u32();
    let field_count = br.read_i32() as usize;
    let property_count = br.read_i32() as usize;
    let field_offset = br.read_i32() as usize;
    br.skip(4);
    let property_offset = br.read_i32() as usize;
    br.skip(4);

    let properties = br.step_in(property_offset, |br| {
        (0..property_count)
            .map(|_| read_property(br, true))
            .collect()
    });
    let fields = read_fields_at(br, field_offset, field_count, FieldCtx::Heuristic);

    PropertyModifier {
        type_enum_a,
        type_enum_b,
        fields,
        properties,
    }
}

fn read_action(br: &mut ByteReader) -> Action {
    let id = br.read_i16();
    let unk02 = br.read_bool();
    let unk03 = br.read_bool();
    let unk04 = br.read_i32();
    let field_count1 = br.read_i32() as usize;
    let unk_field_list_count = br.read_i32() as usize;
    let property_count1 = br.read_i32() as usize;
    let field_count2 = br.read_i32() as usize;
    br.skip(4);
    let property_count2 = br.read_i32() as usize;
    let field_offset = br.read_i32() as usize;
    br.skip(4);
    let unk_field_list_offset = br.read_i32() as usize;
    br.skip(4);
    let property_offset = br.read_i32() as usize;
    br.skip(12); // 3x assert int32 0

    let (properties1, properties2) = br.step_in(property_offset, |br| {
        let p1: Vec<Property> = (0..property_count1)
            .map(|_| read_property(br, false))
            .collect();
        let p2: Vec<Property> = (0..property_count2)
            .map(|_| read_property(br, false))
            .collect();
        (p1, p2)
    });

    let unk_field_lists = br.step_in(unk_field_list_offset, |br| {
        (0..unk_field_list_count)
            .map(|_| read_unk_field_list(br))
            .collect()
    });

    let (fields1, fields2) = br.step_in(field_offset, |br| {
        let f1 = read_fields(br, field_count1, FieldCtx::Heuristic);
        let f2 = read_fields(br, field_count2, FieldCtx::Heuristic);
        (f1, f2)
    });

    Action {
        id,
        unk02,
        unk03,
        unk04,
        unk_field_lists,
        fields1,
        fields2,
        properties1,
        properties2,
    }
}

fn read_effect(br: &mut ByteReader) -> Effect {
    let id = br.read_i16();
    br.skip(2);
    br.skip(4);
    br.skip(4);
    let action_count = br.read_i32() as usize;
    br.skip(8);
    let action_offset = br.read_i32() as usize;
    br.skip(4);
    let actions = br.step_in(action_offset, |br| {
        (0..action_count).map(|_| read_action(br)).collect()
    });
    Effect { id, actions }
}

fn read_container(br: &mut ByteReader) -> Container {
    let id = br.read_i16();
    br.skip(2);
    br.skip(4);
    let effect_count = br.read_i32() as usize;
    let action_count = br.read_i32() as usize;
    let container_count = br.read_i32() as usize;
    br.skip(4);
    let effect_offset = br.read_i32() as usize;
    br.skip(4);
    let action_offset = br.read_i32() as usize;
    br.skip(4);
    let container_offset = br.read_i32() as usize;
    br.skip(4);

    let containers = br.step_in(container_offset, |br| {
        (0..container_count).map(|_| read_container(br)).collect()
    });
    let effects = br.step_in(effect_offset, |br| {
        (0..effect_count).map(|_| read_effect(br)).collect()
    });
    let actions = br.step_in(action_offset, |br| {
        (0..action_count).map(|_| read_action(br)).collect()
    });

    Container {
        id,
        actions,
        effects,
        containers,
    }
}

fn read_i32s_at(br: &ByteReader, offset: usize, count: usize) -> Vec<i32> {
    (0..count).map(|i| br.get_i32_at(offset + i * 4)).collect()
}

pub const SFX_REFERENCE_ACTION_ID: i16 = 132;

fn walk_container_actions<'a>(c: &'a Container, out: &mut Vec<&'a Action>) {
    out.extend(c.actions.iter());
    for e in &c.effects {
        out.extend(e.actions.iter());
    }
    for child in &c.containers {
        walk_container_actions(child, out);
    }
}

impl Fxr {
    pub fn actions(&self) -> Vec<&Action> {
        let mut out = Vec::new();
        walk_container_actions(&self.root_container, &mut out);
        out
    }

    pub fn proxy_targets(&self) -> Vec<i32> {
        let mut seen = Vec::new();
        for action in self.actions() {
            if action.id != SFX_REFERENCE_ACTION_ID {
                continue;
            }
            if let Some(Field::Int(id)) = action.fields1.first() {
                if *id > 0 && !seen.contains(id) {
                    seen.push(*id);
                }
            }
        }
        seen
    }

    pub fn parse(bytes: &[u8]) -> Result<Fxr, FxrError> {
        let mut br = ByteReader::new(bytes);
        if br.read_bytes(4) != b"FXR\0" {
            return Err(FxrError::NotFxr);
        }
        br.skip(2); // assert int16 0
        let version = br.read_u16();
        if version != 5 {
            return Err(FxrError::UnsupportedVersion(version));
        }
        br.skip(4); // assert int32 1
        let id = br.read_i32();
        let state_map_offset = br.read_i32() as usize;
        br.skip(4); // section1count assert==1
        br.skip(80); // section2..11 offset+count pairs (20 i32s) -- all discarded, see module doc
        let container_offset = {
            // container_offset lives among the discarded section table (Section 4's offset,
            // read here directly rather than via the generic skip above).
            br.get_i32_at(40) as usize
        };
        br.skip(8); // assert int32 1, assert int32 0

        // Sekiro-only tail (version 5, the only version this module supports).
        let reference_offset = br.read_i32() as usize;
        let reference_count = br.read_i32() as usize;
        let external_value_offset = br.read_i32() as usize;
        let external_value_count = br.read_i32() as usize;
        let unk_blood_enabler_offset = br.read_i32() as usize;
        let unk_blood_enabler_count = br.read_i32() as usize;
        br.skip(8); // section15 offset + assert0 count

        let reference_list = read_i32s_at(&br, reference_offset, reference_count);
        let external_value_list = read_i32s_at(&br, external_value_offset, external_value_count);
        let unk_blood_enabler =
            read_i32s_at(&br, unk_blood_enabler_offset, unk_blood_enabler_count);

        let root_state_map = br.step_in(state_map_offset, read_state_map);
        let root_container = br.step_in(container_offset, read_container);

        Ok(Fxr {
            version,
            id,
            root_state_map,
            root_container,
            reference_list,
            external_value_list,
            unk_blood_enabler,
        })
    }
}

// --- Write ---
//
// Every offset in this format is absolute, and this module is the only reader for its own
// output, so encoding uses a simple recursive "write this level's headers, then write each
// item's deferred body" pattern rather than SoulsFormatsNEXT's exact global multi-pass
// traversal (see module doc's third simplification).

// The encoder's buffer is `cursor::ByteWriter`, aliased so the ~180 `w.write_*` call sites
// below read unchanged. It was this module's own private `Writer` until `bnd4` and `fmg`
// needed the same primitives.
use crate::cursor::ByteWriter as Writer;

fn write_field(w: &mut Writer, f: &Field) {
    match f {
        Field::Int(v) => w.write_i32(*v),
        Field::Float(v) => w.write_f32(*v),
    }
}

fn write_fields(w: &mut Writer, fields: &[Field]) {
    for f in fields {
        write_field(w, f);
    }
}

fn write_state_condition(w: &mut Writer, c: &StateCondition) {
    let op = (c.operator_type & 0b11) as i16 | (((c.unk_modifier & 0b11) as i16) << 2);
    w.write_i16(op);
    w.write_u8(0);
    w.write_u8(1);
    w.write_i32(0);
    w.write_i32(c.else_move_to_state_index);
    w.write_i32(0);

    w.write_i16(c.left_operand_type);
    w.write_u8(0);
    w.write_u8(1);
    w.write_i32(0);
    w.write_i32(if c.left_value.is_some() { 1 } else { 0 });
    w.write_i32(0);
    let field_offset1 = w.reserve_i32();
    w.write_i32(0);
    w.write_i32(0);
    w.write_i32(0);
    w.write_i32(0);
    w.write_i32(0);

    w.write_i16(c.right_operand_type);
    w.write_u8(0);
    w.write_u8(1);
    w.write_i32(0);
    w.write_i32(if c.right_value.is_some() { 1 } else { 0 });
    w.write_i32(0);
    let field_offset2 = w.reserve_i32();
    w.write_i32(0);
    w.write_i32(0);
    w.write_i32(0);
    w.write_i32(0);
    w.write_i32(0);

    if let Some(v) = &c.left_value {
        w.fill_i32(field_offset1, w.pos() as i32);
        write_field(w, v);
    } else {
        w.fill_i32(field_offset1, 0);
    }
    if let Some(v) = &c.right_value {
        w.fill_i32(field_offset2, w.pos() as i32);
        write_field(w, v);
    } else {
        w.fill_i32(field_offset2, 0);
    }
}

fn write_states(w: &mut Writer, states: &[State]) {
    let mut reserves = Vec::with_capacity(states.len());
    for s in states {
        w.write_i32(0);
        w.write_i32(s.conditions.len() as i32);
        reserves.push(w.reserve_i32());
        w.write_i32(0);
    }
    for (s, reserve) in states.iter().zip(reserves) {
        w.fill_i32(reserve, w.pos() as i32);
        for c in &s.conditions {
            write_state_condition(w, c);
        }
    }
}

fn write_state_map(w: &mut Writer, sm: &StateMap) {
    w.write_i32(0);
    w.write_i32(sm.states.len() as i32);
    let states_offset = w.reserve_i32();
    w.write_i32(0);
    w.pad(16);
    w.fill_i32(states_offset, w.pos() as i32);
    write_states(w, &sm.states);
}

fn write_unk_field_lists(w: &mut Writer, lists: &[UnkFieldList]) {
    let mut reserves = Vec::with_capacity(lists.len());
    for l in lists {
        let offset = w.reserve_i32();
        w.write_i32(0);
        w.write_i32(l.fields.len() as i32);
        w.write_i32(0);
        reserves.push(offset);
    }
    for (l, offset) in lists.iter().zip(reserves) {
        w.fill_i32(offset, w.pos() as i32);
        write_fields(w, &l.fields);
    }
}

fn write_property_header(
    w: &mut Writer,
    p: &Property,
    conditional: bool,
) -> (usize, Option<usize>) {
    let type_enum_a = (p.property_type as i32 & 0b11)
        | ((p.interpolation_type as i32 & 0b1111) << 4)
        | ((p.is_loop as i32) << 12);
    let type_enum_b =
        (p.property_type as i32 | ((p.interpolation_type as i32) << 2)) + ((p.is_loop as i32) << 4);
    w.write_i16(type_enum_a as i16);
    w.write_u8(0);
    w.write_u8(1);
    w.write_i32(type_enum_b);
    w.write_i32(p.fields.len() as i32);
    w.write_i32(0);
    let fields_offset = w.reserve_i32();
    w.write_i32(0);

    let modifiers_offset = if conditional {
        None
    } else {
        let mo = w.reserve_i32();
        w.write_i32(0);
        w.write_i32(p.modifiers.len() as i32);
        w.write_i32(0);
        Some(mo)
    };

    (fields_offset, modifiers_offset)
}

fn write_properties(w: &mut Writer, props: &[Property], conditional: bool) {
    let mut reserves = Vec::with_capacity(props.len());
    for p in props {
        reserves.push(write_property_header(w, p, conditional));
    }
    for (p, (fields_offset, modifiers_offset)) in props.iter().zip(reserves) {
        w.fill_i32(fields_offset, w.pos() as i32);
        write_fields(w, &p.fields);
        if let Some(mo) = modifiers_offset {
            w.fill_i32(mo, w.pos() as i32);
            write_property_modifiers(w, &p.modifiers);
        }
    }
}

fn write_property_modifiers(w: &mut Writer, mods: &[PropertyModifier]) {
    let mut reserves = Vec::with_capacity(mods.len());
    for m in mods {
        w.write_u16(m.type_enum_a);
        w.write_u8(0);
        w.write_u8(1);
        w.write_u32(m.type_enum_b);
        w.write_i32(m.fields.len() as i32);
        w.write_i32(m.properties.len() as i32);
        let fields_offset = w.reserve_i32();
        w.write_i32(0);
        let properties_offset = w.reserve_i32();
        w.write_i32(0);
        reserves.push((fields_offset, properties_offset));
    }
    for (m, (fields_offset, properties_offset)) in mods.iter().zip(reserves) {
        w.fill_i32(fields_offset, w.pos() as i32);
        write_fields(w, &m.fields);
        w.fill_i32(properties_offset, w.pos() as i32);
        write_properties(w, &m.properties, true);
    }
}

fn write_actions(w: &mut Writer, actions: &[Action]) {
    let mut reserves = Vec::with_capacity(actions.len());
    for a in actions {
        w.write_i16(a.id);
        w.write_bool(a.unk02);
        w.write_bool(a.unk03);
        w.write_i32(a.unk04);
        w.write_i32(a.fields1.len() as i32);
        w.write_i32(a.unk_field_lists.len() as i32);
        w.write_i32(a.properties1.len() as i32);
        w.write_i32(a.fields2.len() as i32);
        w.write_i32(0);
        w.write_i32(a.properties2.len() as i32);
        let fields_offset = w.reserve_i32();
        w.write_i32(0);
        let unk_field_lists_offset = w.reserve_i32();
        w.write_i32(0);
        let properties_offset = w.reserve_i32();
        w.write_i32(0);
        w.write_i32(0);
        w.write_i32(0);
        reserves.push((fields_offset, unk_field_lists_offset, properties_offset));
    }

    for (a, (fields_offset, unk_field_lists_offset, properties_offset)) in
        actions.iter().zip(reserves)
    {
        w.fill_i32(properties_offset, w.pos() as i32);
        write_properties(w, &a.properties1, false);
        write_properties(w, &a.properties2, false);

        w.fill_i32(unk_field_lists_offset, w.pos() as i32);
        write_unk_field_lists(w, &a.unk_field_lists);

        if a.fields1.is_empty() && a.fields2.is_empty() {
            w.fill_i32(fields_offset, 0);
        } else {
            w.fill_i32(fields_offset, w.pos() as i32);
            write_fields(w, &a.fields1);
            write_fields(w, &a.fields2);
        }
    }
}

fn write_effects(w: &mut Writer, effects: &[Effect]) {
    let mut reserves = Vec::with_capacity(effects.len());
    for e in effects {
        w.write_i16(e.id);
        w.write_u8(0);
        w.write_u8(1);
        w.write_i32(0);
        w.write_i32(0);
        w.write_i32(e.actions.len() as i32);
        w.write_i32(0);
        w.write_i32(0);
        let actions_offset = w.reserve_i32();
        w.write_i32(0);
        reserves.push(actions_offset);
    }
    for (e, actions_offset) in effects.iter().zip(reserves) {
        w.fill_i32(actions_offset, w.pos() as i32);
        write_actions(w, &e.actions);
    }
}

fn write_containers(w: &mut Writer, containers: &[Container]) {
    let mut reserves = Vec::with_capacity(containers.len());
    for c in containers {
        w.write_i16(c.id);
        w.write_u8(0);
        w.write_u8(1);
        w.write_i32(0);
        w.write_i32(c.effects.len() as i32);
        w.write_i32(c.actions.len() as i32);
        w.write_i32(c.containers.len() as i32);
        w.write_i32(0);
        let effects_offset = w.reserve_i32();
        w.write_i32(0);
        let actions_offset = w.reserve_i32();
        w.write_i32(0);
        let children_offset = w.reserve_i32();
        w.write_i32(0);
        reserves.push((effects_offset, actions_offset, children_offset));
    }

    for (c, (effects_offset, actions_offset, children_offset)) in containers.iter().zip(reserves) {
        if c.effects.is_empty() {
            w.fill_i32(effects_offset, 0);
        } else {
            w.fill_i32(effects_offset, w.pos() as i32);
            write_effects(w, &c.effects);
        }

        w.fill_i32(actions_offset, w.pos() as i32);
        write_actions(w, &c.actions);

        if c.containers.is_empty() {
            w.fill_i32(children_offset, 0);
        } else {
            w.fill_i32(children_offset, w.pos() as i32);
            write_containers(w, &c.containers);
        }
    }
}

impl Fxr {
    pub fn encode(&self) -> Vec<u8> {
        let mut w = Writer::new();
        w.write_bytes(b"FXR\0");
        w.write_i16(0);
        w.write_u16(self.version);
        w.write_i32(1);
        w.write_i32(self.id);
        let state_map_offset = w.reserve_i32();
        w.write_i32(1); // section1count
                        // Section 2..11 offset+count pairs -- all recomputed as 0 here (unused on read, see
                        // module doc); container_offset (section 4's offset) is the one exception we must fill.
        let section_table_start = w.pos();
        for _ in 0..20 {
            w.write_i32(0);
        }
        w.write_i32(1);
        w.write_i32(0);

        let reference_offset = w.reserve_i32();
        w.write_i32(self.reference_list.len() as i32);
        let external_value_offset = w.reserve_i32();
        w.write_i32(self.external_value_list.len() as i32);
        let unk_blood_enabler_offset = w.reserve_i32();
        w.write_i32(self.unk_blood_enabler.len() as i32);
        let section15_offset = w.reserve_i32();
        w.write_i32(0);

        w.fill_i32(state_map_offset, w.pos() as i32);
        write_state_map(&mut w, &self.root_state_map);
        w.pad(16);

        // Section 4's offset (container offset) is the only discarded-on-read section-table
        // entry we actually need to fill for well-formedness; it lives at
        // section_table_start + 4*8 (section entries: 2,3,4,5,6,7,8,9,10,11 offset+count pairs,
        // container is the 3rd pair -> entries[4] in the flat 20-i32 array, 0-indexed).
        let container_table_offset_pos = section_table_start + 4 * 4;
        w.fill_i32(container_table_offset_pos, w.pos() as i32);
        write_containers(&mut w, std::slice::from_ref(&self.root_container));
        w.pad(16);

        w.fill_i32(reference_offset, w.pos() as i32);
        for v in &self.reference_list {
            w.write_i32(*v);
        }
        w.pad(16);

        w.fill_i32(external_value_offset, w.pos() as i32);
        for v in &self.external_value_list {
            w.write_i32(*v);
        }
        w.pad(16);

        w.fill_i32(unk_blood_enabler_offset, w.pos() as i32);
        for v in &self.unk_blood_enabler {
            w.write_i32(*v);
        }
        w.pad(16);

        w.fill_i32(section15_offset, 0);

        w.into_vec()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_path(name: &str) -> std::path::PathBuf {
        std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
            .join(format!("../../data/reference/fxr-samples/{name}"))
    }

    #[test]
    fn proxy_targets_finds_the_gravity_ball_via_action_132() {
        let bytes = std::fs::read(sample_path("f000529982.fxr")).unwrap();
        let fxr = Fxr::parse(&bytes).expect("should parse");
        assert_eq!(
            fxr.proxy_targets(),
            vec![529972],
            "Gravity Well's emitter should reference exactly the ball sfx"
        );
    }

    #[test]
    fn ball_fxr_has_no_proxy_targets() {
        let bytes = std::fs::read(sample_path("f000529972.fxr")).unwrap();
        let fxr = Fxr::parse(&bytes).expect("should parse");
        assert!(fxr.proxy_targets().is_empty());
    }

    #[test]
    fn parses_gravity_well_emitter() {
        let bytes = std::fs::read(sample_path("f000529982.fxr")).unwrap();
        let fxr = Fxr::parse(&bytes).expect("should parse");
        assert_eq!(fxr.id, 529982);
        assert_eq!(fxr.version, 5);
        assert_eq!(fxr.root_state_map.states.len(), 2);
        assert_eq!(fxr.root_container.actions.len(), 4);
        assert_eq!(fxr.root_container.containers.len(), 1);
    }

    #[test]
    fn round_trip_reparse_matches_original_tree() {
        // Test A (spike step 3): parse -> re-encode with zero mutation -> re-parse -> compare
        // decoded trees. Byte-identical output is NOT required (see module doc) -- a faithful
        // encoder can legitimately reorder sibling groups vs. FromSoft's own encoder.
        let bytes = std::fs::read(sample_path("f000529982.fxr")).unwrap();
        let original = Fxr::parse(&bytes).expect("should parse");
        let reencoded = original.encode();
        let reparsed =
            Fxr::parse(&reencoded).expect("re-encoded bytes should still parse as a valid FXR");

        assert_eq!(reparsed.id, original.id);
        assert_eq!(reparsed.version, original.version);
        assert_eq!(reparsed.reference_list, original.reference_list);
        assert_eq!(reparsed.external_value_list, original.external_value_list);
        assert_eq!(reparsed.unk_blood_enabler, original.unk_blood_enabler);
        assert_eq!(
            reparsed.root_state_map.states.len(),
            original.root_state_map.states.len()
        );
        assert_eq!(
            count_actions(&reparsed.root_container),
            count_actions(&original.root_container)
        );
        assert_eq!(
            count_fields(&reparsed.root_container),
            count_fields(&original.root_container)
        );
    }

    fn count_actions(c: &Container) -> usize {
        c.actions.len()
            + c.effects.iter().map(|e| e.actions.len()).sum::<usize>()
            + c.containers.iter().map(count_actions).sum::<usize>()
    }

    fn count_fields(c: &Container) -> usize {
        let action_fields = |a: &Action| a.fields1.len() + a.fields2.len();
        c.actions.iter().map(action_fields).sum::<usize>()
            + c.effects
                .iter()
                .flat_map(|e| &e.actions)
                .map(action_fields)
                .sum::<usize>()
            + c.containers.iter().map(count_fields).sum::<usize>()
    }

    #[test]
    fn retarget_proxy_node_and_reencode() {
        // Test B (spike step 3): find the ProxyNode-equivalent action, confirm its recorded
        // target id is 529972 (Gravity Well's ball, per
        // Spell_Making_UI/.../custom_spells/_reference/fxr-pipeline.md), retarget it, re-encode,
        // and confirm the new id round-trips. The oracle comparison against `@cccode/fxr`'s own
        // edit of the same file is a separate manual step (see docs/known-offsets.md) -- this
        // test proves the Rust side of the round trip in isolation.
        let bytes = std::fs::read(sample_path("f000529982.fxr")).unwrap();
        let mut fxr = Fxr::parse(&bytes).expect("should parse");

        let target_id = 529972i32;
        let mut found = false;
        for action in all_actions_mut(&mut fxr.root_container) {
            for field in action.fields1.iter_mut().chain(action.fields2.iter_mut()) {
                if let Field::Int(v) = field {
                    if *v == target_id {
                        *v = 529999;
                        found = true;
                    }
                }
            }
        }
        assert!(
            found,
            "expected to find a field referencing sfx id {target_id}"
        );

        let reencoded = fxr.encode();
        let reparsed = Fxr::parse(&reencoded).expect("re-encoded bytes should still parse");

        let has_new_id = all_actions(&reparsed.root_container)
            .flat_map(|a| a.fields1.iter().chain(a.fields2.iter()))
            .any(|f| matches!(f, Field::Int(529999)));
        assert!(has_new_id, "retargeted id should survive the round trip");

        let has_old_id = all_actions(&reparsed.root_container)
            .flat_map(|a| a.fields1.iter().chain(a.fields2.iter()))
            .any(|f| matches!(f, Field::Int(v) if *v == target_id));
        assert!(
            !has_old_id,
            "old id should no longer be present after retargeting"
        );
    }

    fn all_actions(c: &Container) -> Box<dyn Iterator<Item = &Action> + '_> {
        Box::new(
            c.actions
                .iter()
                .chain(c.effects.iter().flat_map(|e| e.actions.iter()))
                .chain(c.containers.iter().flat_map(all_actions)),
        )
    }

    fn all_actions_mut(c: &mut Container) -> Box<dyn Iterator<Item = &mut Action> + '_> {
        Box::new(
            c.actions
                .iter_mut()
                .chain(c.effects.iter_mut().flat_map(|e| e.actions.iter_mut()))
                .chain(c.containers.iter_mut().flat_map(all_actions_mut)),
        )
    }
}
