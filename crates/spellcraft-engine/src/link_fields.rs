// Docs: docs/spellcraft-engine/link_fields.md

use serde::{Deserialize, Serialize};

#[cfg(feature = "ts")]
use ts_rs::TS;

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(TS))]
#[serde(rename_all = "camelCase")]
pub struct LinkFields {
    pub atk: Vec<String>,
    pub bullet: Vec<String>,
    pub sp_effect: Vec<String>,
    pub sp_effect_vfx: Vec<String>,
    pub sfx: Vec<String>,
    pub uncategorized: Vec<String>,
}

fn strs(v: &[&str]) -> Vec<String> {
    v.iter().map(|s| s.to_string()).collect()
}

pub const CATALOGUED_PARAM_TYPES: [&str; 5] = [
    "MAGIC_PARAM_ST",
    "BULLET_PARAM_ST",
    "ATK_PARAM_ST",
    "SP_EFFECT_PARAM_ST",
    "SP_EFFECT_VFX_PARAM_ST",
];

pub fn all_link_fields() -> std::collections::BTreeMap<String, LinkFields> {
    CATALOGUED_PARAM_TYPES
        .iter()
        .filter_map(|pt| link_fields(pt).map(|f| (pt.to_string(), f)))
        .collect()
}

pub fn link_fields(param_type: &str) -> Option<LinkFields> {
    match param_type {
        "BULLET_PARAM_ST" => Some(LinkFields {
            atk: strs(&["atkId_Bullet"]),
            bullet: strs(&["HitBulletID", "intervalCreateBulletId"]),
            sp_effect: strs(&[
                "spEffectIDForShooter",
                "spEffectId0",
                "spEffectId1",
                "spEffectId2",
                "spEffectId3",
                "spEffectId4",
            ]),
            sp_effect_vfx: Vec::new(),
            sfx: strs(&["sfxId_Bullet", "sfxId_Hit", "sfxId_Flick"]),
            uncategorized: strs(&[
                "seId_Bullet1",
                "seId_Bullet2",
                "seId_Hit",
                "seId_Flick",
                "sfxId_ForceErase",
            ]),
        }),

        "ATK_PARAM_ST" => Some(LinkFields {
            atk: strs(&["atkBehaviorId", "atkBehaviorId_2"]),
            bullet: Vec::new(),
            sp_effect: strs(&[
                "spEffectId0",
                "spEffectId1",
                "spEffectId2",
                "spEffectId3",
                "spEffectId4",
            ]),
            sp_effect_vfx: Vec::new(),
            sfx: strs(&[
                "traceSfxId0",
                "traceSfxId1",
                "traceSfxId2",
                "traceSfxId3",
                "traceSfxId4",
                "traceSfxId5",
                "traceSfxId6",
                "traceSfxId7",
            ]),
            uncategorized: strs(&[
                "decalId1",
                "decalId2",
                "AppearAiSoundId",
                "HitAiSoundId",
                "HitRumbleId",
                "HitRumbleIdByNormal",
                "HitRumbleIdByMiddle",
                "HitRumbleIdByRoot",
                "traceDmyIdHead0",
                "traceDmyIdTail0",
                "traceDmyIdHead1",
                "traceDmyIdTail1",
                "traceDmyIdHead2",
                "traceDmyIdTail2",
                "traceDmyIdHead3",
                "traceDmyIdTail3",
                "traceDmyIdHead4",
                "traceDmyIdTail4",
                "traceDmyIdHead5",
                "traceDmyIdTail5",
                "traceDmyIdHead6",
                "traceDmyIdTail6",
                "traceDmyIdHead7",
                "traceDmyIdTail7",
                "overwriteAttackElementCorrectId",
                "decalBaseId1",
                "decalBaseId2",
                "regainableSlotId",
            ]),
        }),

        // A SpEffect's outward links are overwhelmingly its eight `vfxId` slots, which is
        // where its visuals come from. The three SpEffect→SpEffect fields are included
        // because the walker follows them and they are how status chains are built — leaving
        // them out would make a real relationship unconnectable.
        "SP_EFFECT_PARAM_ST" => Some(LinkFields {
            atk: Vec::new(),
            bullet: Vec::new(),
            sp_effect: strs(&[
                "replaceSpEffectId",
                "cycleOccurrenceSpEffectId",
                "atkOccurrenceSpEffectId",
            ]),
            sp_effect_vfx: strs(&[
                "vfxId", "vfxId1", "vfxId2", "vfxId3", "vfxId4", "vfxId5", "vfxId6", "vfxId7",
            ]),
            sfx: Vec::new(),
            uncategorized: strs(&["addFootEffectSfxId"]),
        }),

        // The bridge from a SpEffect to real FXR files — see docs/known-offsets.md, which
        // records that `SpEffect.vfxId*` indexes this table rather than pointing at an FXR.
        "SP_EFFECT_VFX_PARAM_ST" => Some(LinkFields {
            sfx: strs(&["midstSfxId", "initSfxId", "finishSfxId"]),
            ..Default::default()
        }),

        // A Magic row's bullet slots. `refId1..10` are the cast slots the walker already
        // follows; which one is used decides the cast behaviour, so all ten are offerable.
        "MAGIC_PARAM_ST" => Some(LinkFields {
            bullet: strs(&[
                "refId1", "refId2", "refId3", "refId4", "refId5", "refId6", "refId7", "refId8",
                "refId9", "refId10",
            ]),
            ..Default::default()
        }),

        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_bullet_offers_its_attack_and_visual_slots() {
        let b = link_fields("BULLET_PARAM_ST").expect("bullets have link fields");
        assert_eq!(b.atk, vec!["atkId_Bullet"]);
        assert!(b.sfx.contains(&"sfxId_Flick".to_string()));
        assert_eq!(b.sp_effect.len(), 6, "shooter slot plus spEffectId0..4");
    }

    #[test]
    fn it_is_broader_than_what_a_given_spell_uses() {
        let b = link_fields("BULLET_PARAM_ST").unwrap();
        for field in ["HitBulletID", "intervalCreateBulletId", "spEffectIDForShooter"] {
            assert!(
                b.bullet.contains(&field.to_string()) || b.sp_effect.contains(&field.to_string()),
                "{field} should be offerable even when unused"
            );
        }
    }

    #[test]
    fn an_attack_offers_eight_trace_sfx_slots() {
        let a = link_fields("ATK_PARAM_ST").unwrap();
        assert_eq!(a.sfx.len(), 8);
        assert!(a.bullet.is_empty(), "attacks do not spawn bullets directly");
    }

    #[test]
    fn a_speffect_reaches_visuals_through_vfx_rows_not_fxr_ids() {
        let s = link_fields("SP_EFFECT_PARAM_ST").unwrap();
        assert_eq!(s.sp_effect_vfx.len(), 8, "vfxId plus vfxId1..7");
        assert!(
            s.sfx.is_empty(),
            "a SpEffect never names an FXR directly — see docs/known-offsets.md"
        );
    }

    #[test]
    fn the_catalogue_covers_every_type_regardless_of_any_document() {
        let all = all_link_fields();
        for pt in CATALOGUED_PARAM_TYPES {
            assert!(all.contains_key(pt), "{pt} missing from the exported catalogue");
        }
        assert_eq!(all.len(), CATALOGUED_PARAM_TYPES.len());
    }

    #[test]
    fn an_unknown_param_type_has_no_catalogue() {
        assert!(link_fields("EQUIP_PARAM_GOODS_ST").is_none());
        assert!(link_fields("NOT_A_PARAM").is_none());
    }

    #[test]
    fn every_catalogued_field_exists_in_the_paramdef() {
        let defs_dir = souls_format::locate::locate_paramdex_defs()
            .expect("vendored paramdex should always be found");
        let defs = souls_format::ParamdefLibrary::open(&defs_dir).expect("paramdex should open");

        for pt in [
            "BULLET_PARAM_ST",
            "ATK_PARAM_ST",
            "SP_EFFECT_PARAM_ST",
            "SP_EFFECT_VFX_PARAM_ST",
            "MAGIC_PARAM_ST",
        ] {
            let def = defs
                .by_param_type(pt)
                .unwrap_or_else(|e| panic!("{pt}: no paramdef: {e}"));
            let real: std::collections::HashSet<&str> =
                def.fields.iter().map(|f| f.internal_name.as_str()).collect();

            let groups = link_fields(pt).unwrap();
            for (group, fields) in [
                ("atk", &groups.atk),
                ("bullet", &groups.bullet),
                ("spEffect", &groups.sp_effect),
                ("spEffectVfx", &groups.sp_effect_vfx),
                ("sfx", &groups.sfx),
                ("uncategorized", &groups.uncategorized),
            ] {
                for name in fields {
                    assert!(
                        real.contains(name.as_str()),
                        "{pt}.{group}: '{name}' is not a field on this param type — a typo \
                         here writes to the wrong field, or fails inside apply with no trace \
                         back to this table"
                    );
                }
            }
        }
    }

    #[test]
    fn no_field_is_listed_twice_within_a_param_type() {
        for pt in [
            "BULLET_PARAM_ST",
            "ATK_PARAM_ST",
            "SP_EFFECT_PARAM_ST",
            "SP_EFFECT_VFX_PARAM_ST",
            "MAGIC_PARAM_ST",
        ] {
            let f = link_fields(pt).unwrap();
            let mut all: Vec<&String> = f
                .atk
                .iter()
                .chain(&f.bullet)
                .chain(&f.sp_effect)
                .chain(&f.sp_effect_vfx)
                .chain(&f.sfx)
                .chain(&f.uncategorized)
                .collect();
            let before = all.len();
            all.sort();
            all.dedup();
            assert_eq!(before, all.len(), "{pt} lists a field more than once");
        }
    }
}
