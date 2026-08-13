// Docs: docs/spellcraft-engine/allocator.md

pub const PRIMARY_SLOT_RANGE: std::ops::RangeInclusive<i64> = 4000..=4999;

pub const SECONDARY_SLOT_RANGE: std::ops::RangeInclusive<i64> = 30000..=30999;

pub const ROW_BUDGET_PER_SLOT: usize = 18;

#[derive(Debug, thiserror::Error)]
pub enum AllocatorError {
    #[error("no free slot id available in range")]
    NoFreeSlot,
    #[error("fusion needs {needed} rows, budget is {budget} — simplify the composition mode")]
    OverRowBudget { needed: usize, budget: usize },
}

pub struct IdAllocator {
    occupied: std::collections::HashSet<i64>,
}

impl IdAllocator {
    pub fn from_param_bank(
        bank: &souls_format::ParamBank,
    ) -> Result<Self, souls_format::param_bank::ParamBankError> {
        let mut occupied = std::collections::HashSet::new();
        occupied.extend(bank.row_ids("Magic.param")?);
        occupied.extend(bank.row_ids("EquipParamGoods.param")?);
        Ok(IdAllocator { occupied })
    }

    pub fn allocate_slot(&mut self, rows_needed: usize) -> Result<i64, AllocatorError> {
        if rows_needed > ROW_BUDGET_PER_SLOT {
            return Err(AllocatorError::OverRowBudget {
                needed: rows_needed,
                budget: ROW_BUDGET_PER_SLOT,
            });
        }
        for id in PRIMARY_SLOT_RANGE {
            if !self.occupied.contains(&id) {
                self.occupied.insert(id);
                return Ok(id);
            }
        }
        for id in SECONDARY_SLOT_RANGE {
            if !self.occupied.contains(&id) {
                self.occupied.insert(id);
                return Ok(id);
            }
        }
        Err(AllocatorError::NoFreeSlot)
    }
}

pub const REF_ID_FIELDS: [&str; 10] = [
    "refId1", "refId2", "refId3", "refId4", "refId5", "refId6", "refId7", "refId8", "refId9",
    "refId10",
];

pub fn is_empty_dummy(magic_row: &souls_format::ParamRow) -> bool {
    REF_ID_FIELDS.iter().all(|field| {
        matches!(
            magic_row.fields.get(*field),
            Some(souls_format::paramdef::ParamValue::I64(-1))
        )
    })
}

#[cfg(test)]
mod empty_dummy_tests {
    use super::*;
    use souls_format::paramdef::{ParamRow, ParamValue};
    use std::collections::BTreeMap;

    fn pristine_dummy() -> ParamRow {
        let mut fields = BTreeMap::new();
        for field in REF_ID_FIELDS {
            fields.insert(field.to_string(), ParamValue::I64(-1));
        }
        ParamRow { id: 4002, fields }
    }

    #[test]
    fn a_pristine_dummy_is_free() {
        assert!(is_empty_dummy(&pristine_dummy()));
    }

    #[test]
    fn any_reference_set_means_taken() {
        for field in REF_ID_FIELDS {
            let mut row = pristine_dummy();
            row.fields
                .insert(field.to_string(), ParamValue::I64(400200000));
            assert!(
                !is_empty_dummy(&row),
                "{field} set should mark the slot taken"
            );
        }
    }

    #[test]
    fn unreadable_rows_are_never_free() {
        let mut missing = pristine_dummy();
        missing.fields.remove("refId7");
        assert!(!is_empty_dummy(&missing), "a missing refId is not free");

        let mut wrong_type = pristine_dummy();
        wrong_type
            .fields
            .insert("refId3".to_string(), ParamValue::F32(-1.0));
        assert!(
            !is_empty_dummy(&wrong_type),
            "an f32 -1.0 is not the i64 -1 this checks for"
        );

        assert!(
            !is_empty_dummy(&ParamRow {
                id: 4002,
                fields: BTreeMap::new()
            }),
            "a row with no fields at all is not free"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use souls_format::locate::{locate_paramdex_defs, locate_regulation_bin};
    use souls_format::{ParamBank, ParamdefLibrary, Regulation};

    fn bank() -> Option<ParamBank> {
        let path = locate_regulation_bin()?;
        let regulation = Regulation::open(&path).expect("regulation should open");
        let defs = ParamdefLibrary::open(&locate_paramdex_defs()?).expect("paramdex should open");
        Some(ParamBank::new(regulation, defs))
    }

    #[test]
    #[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
    fn allocates_a_free_id_in_the_primary_range() {
        let Some(bank) = bank() else {
            eprintln!("skipping: no regulation.bin");
            return;
        };
        let mut allocator = IdAllocator::from_param_bank(&bank).unwrap();
        let id = allocator.allocate_slot(0).unwrap();
        assert!(
            PRIMARY_SLOT_RANGE.contains(&id) || SECONDARY_SLOT_RANGE.contains(&id),
            "allocated id {id} outside both known ranges"
        );

        let magic_ids: std::collections::HashSet<i64> =
            bank.row_ids("Magic.param").unwrap().into_iter().collect();
        let goods_ids: std::collections::HashSet<i64> = bank
            .row_ids("EquipParamGoods.param")
            .unwrap()
            .into_iter()
            .collect();
        assert!(!magic_ids.contains(&id), "allocated id already used in Magic.param");
        assert!(!goods_ids.contains(&id), "allocated id already used in EquipParamGoods.param");
    }

    #[test]
    #[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
    fn does_not_allocate_the_sentinel_or_reallocate_the_same_id_twice() {
        let Some(bank) = bank() else {
            eprintln!("skipping: no regulation.bin");
            return;
        };
        let mut allocator = IdAllocator::from_param_bank(&bank).unwrap();
        let first = allocator.allocate_slot(0).unwrap();
        let second = allocator.allocate_slot(0).unwrap();
        assert_ne!(first, second, "allocator must not hand out the same id twice");
        assert_ne!(first, 999_999_999);
        assert_ne!(second, 999_999_999);
    }

    #[test]
    fn exhausting_both_ranges_returns_no_free_slot() {
        let mut occupied = std::collections::HashSet::new();
        occupied.extend(PRIMARY_SLOT_RANGE);
        occupied.extend(SECONDARY_SLOT_RANGE);
        let mut allocator = IdAllocator { occupied };
        assert!(matches!(
            allocator.allocate_slot(0),
            Err(AllocatorError::NoFreeSlot)
        ));
    }

    #[test]
    fn a_request_over_budget_is_refused_before_searching() {
        let mut allocator = IdAllocator {
            occupied: std::collections::HashSet::new(),
        };
        let err = allocator.allocate_slot(19).unwrap_err();
        assert!(matches!(
            err,
            AllocatorError::OverRowBudget { needed: 19, budget: 18 }
        ));
    }
}
