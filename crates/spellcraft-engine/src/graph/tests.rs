// Docs: docs/spellcraft-engine/graph/tests.md

// The 19/42 derivation below aligns its continuation lines under the arithmetic on
// purpose; clippy reads that as an overindented markdown list. The alignment is what
// makes the sum checkable at a glance, so the lint loses.
#![allow(clippy::doc_overindented_list_items)]

use super::*;
use crate::source::{int_row, FixtureSource, ParamSource, ParamTable};
use souls_format::ParamdefLibrary;
fn edges_to(graph: &SpellGraph, kind: NodeKind) -> Vec<&Edge> {
    graph.edges.iter().filter(|e| e.to.kind == kind).collect()
}

fn assert_projection_matches(full: &SpellGraph, params_only: &SpellGraph) {
    let projected = full.param_projection();
    assert_eq!(
        projected.nodes.iter().map(|n| n.key).collect::<Vec<_>>(),
        params_only.nodes.iter().map(|n| n.key).collect::<Vec<_>>(),
        "param node sequence changed when sfx/vfx were enabled"
    );
    assert_eq!(
        projected
            .edges
            .iter()
            .map(|e| (e.from, e.to, e.field.clone()))
            .collect::<Vec<_>>(),
        params_only
            .edges
            .iter()
            .map(|e| (e.from, e.to, e.field.clone()))
            .collect::<Vec<_>>(),
        "param edge sequence changed when sfx/vfx were enabled"
    );
}

// --- Real vendored data ----------------------------------------------------------

#[test]
fn pebble_walks_both_roots_and_records_the_unexported_bullet() {
    let src = crate::test_support::fixture_from_csvs("GlintstonePebble");
    let graph = Walker::new(&src)
        .with_options(WalkOptions::params_only())
        .walk(4000)
        .expect("Pebble should walk");

    // Both Magic roots are followed — the prior spike only ever followed refId1.
    let magic_edges: Vec<_> = graph.out_edges(graph.root()).collect();
    assert_eq!(
        magic_edges.len(),
        2,
        "Magic 4000 declares refId1 and refId2"
    );

    let first = magic_edges
        .iter()
        .find(|e| e.field.index == Some(1))
        .unwrap();
    assert_eq!(first.to, NodeKey::new(NodeKind::Bullet, 10400000));
    assert_eq!(first.ref_category, Some(1));
    assert_eq!(first.consume_type, Some(0));
    assert_eq!(first.cast_type, CastType::Default);

    let second = magic_edges
        .iter()
        .find(|e| e.field.index == Some(2))
        .unwrap();
    assert_eq!(second.to, NodeKey::new(NodeKind::Bullet, 10400099));
    assert_eq!(second.consume_type, Some(2));
    assert_eq!(second.cast_type, CastType::None);

    // The resolved bullet expands to its attack.
    assert!(graph.contains(NodeKey::new(NodeKind::Atk, 40000)));

    // The unexported one is present as a node, marked missing, and expanded no further.
    let missing = graph
        .node(NodeKey::new(NodeKind::Bullet, 10400099))
        .expect("an unresolvable reference still produces a node");
    assert_eq!(missing.status, NodeStatus::RowMissing);
    assert_eq!(graph.out_edges(missing.key).count(), 0);
    assert!(graph
        .diagnostics
        .iter()
        .any(|d| d.kind == DiagnosticKind::RowMissing));
}

#[test]
fn pebble_sfx_edges_do_not_disturb_param_structure() {
    let src = crate::test_support::fixture_from_csvs("GlintstonePebble");
    let params_only = Walker::new(&src)
        .with_options(WalkOptions::params_only())
        .walk(4000)
        .unwrap();
    let full = Walker::new(&src).walk(4000).unwrap();

    // Magic's three cast/fire/effect sfx ids, plus the bullet's.
    assert!(
        full.contains(NodeKey::new(NodeKind::Fxr, 523000)),
        "castSfxId"
    );
    assert!(
        full.contains(NodeKey::new(NodeKind::Fxr, 523001)),
        "fireSfxId"
    );
    assert!(
        full.contains(NodeKey::new(NodeKind::Fxr, 510010)),
        "effectSfxId"
    );
    assert!(
        full.contains(NodeKey::new(NodeKind::Fxr, 523002)),
        "sfxId_Bullet"
    );
    assert!(
        full.contains(NodeKey::new(NodeKind::Fxr, 523003)),
        "sfxId_Hit"
    );

    // No sfx source configured => every FXR node is an unresolved reference.
    for node in full.nodes_of_kind(NodeKind::Fxr) {
        assert_eq!(node.status, NodeStatus::FxrFileMissing);
    }

    assert_projection_matches(&full, &params_only);
}

#[test]
fn fxr_hydration_adds_proxy_edges_without_touching_params() {
    let src = crate::test_support::fixture_from_csvs("GlintstonePebble");
    let samples = souls_format::SfxDirectory::new(vec![crate::test_support::reference_dir().join("fxr-samples")]);

    let dry = Walker::new(&src).walk(4000).unwrap();
    let hydrated = Walker::new(&src).with_sfx(&samples).walk(4000).unwrap();

    // 523002 is vendored, so it resolves; 523000 is not, so it stays missing.
    assert_eq!(
        hydrated
            .node(NodeKey::new(NodeKind::Fxr, 523002))
            .unwrap()
            .status,
        NodeStatus::Resolved
    );
    assert_eq!(
        hydrated
            .node(NodeKey::new(NodeKind::Fxr, 523000))
            .unwrap()
            .status,
        NodeStatus::FxrFileMissing
    );

    assert_projection_matches(&hydrated, &dry.param_projection());
}

#[test]
fn lorettas_mastery_classifies_and_propagates_charged_casts() {
    let src = crate::test_support::fixture_from_csvs("Loretta's Mastery");
    let magic_id = 4381;
    let Some(_) = src.row(ParamTable::Magic, magic_id).found().cloned() else {
        panic!("Loretta's Mastery Magic row {magic_id} should be in the vendored CSV");
    };
    let graph = Walker::new(&src)
        .with_options(WalkOptions::params_only())
        .walk(magic_id)
        .expect("Loretta's Mastery should walk");

    assert!(
        graph.classifications.contains(&Classification::Charged),
        "a consumeType=1 slot must classify the spell as Charged"
    );

    let charged: Vec<_> = graph
        .out_edges(graph.root())
        .filter(|e| e.cast_type == CastType::Charged)
        .collect();
    assert!(
        !charged.is_empty(),
        "expected at least one charged root ref"
    );
    for e in &charged {
        assert_eq!(e.consume_type, Some(1));
    }

    // The charged branch must reach real attack rows — the C#'s shared-visited bug made
    // this list spuriously empty for spells whose branches overlap.
    assert!(
        !graph.attack_targets_under(CastType::Charged).is_empty(),
        "charged branch reached no attack rows"
    );
    assert!(!graph.attack_targets_under(CastType::Default).is_empty());
}

// --- Hand-authored traversal fixtures ---------------------------------------------

#[test]
fn edges_are_never_deduped_even_when_nodes_are() {
    let mut src = FixtureSource::new();
    src.insert(
        ParamTable::Magic,
        int_row(
            1,
            &[
                ("refId1", 10),
                ("refCategory1", 0),
                ("consumeType1", 0),
                ("refId2", 11),
                ("refCategory2", 0),
                ("consumeType2", 0),
            ],
        ),
    );
    // Both attacks point at the same two SpEffects.
    src.insert(
        ParamTable::AtkPc,
        int_row(10, &[("spEffectId0", 100), ("spEffectId1", 200)]),
    );
    src.insert(
        ParamTable::AtkPc,
        int_row(11, &[("spEffectId0", 100), ("spEffectId1", 200)]),
    );
    src.insert(ParamTable::SpEffect, int_row(100, &[]));
    src.insert(ParamTable::SpEffect, int_row(200, &[]));

    let graph = Walker::new(&src)
        .with_options(WalkOptions::params_only())
        .walk(1)
        .unwrap();

    assert_eq!(
        graph.nodes_of_kind(NodeKind::SpEffect).count(),
        2,
        "SpEffect nodes must be deduped"
    );
    assert_eq!(
        edges_to(&graph, NodeKind::SpEffect).len(),
        4,
        "all four SpEffect references must be recorded as distinct edges"
    );
    assert_eq!(graph.nodes.len(), 5); // Magic + 2 Atk + 2 SpEffect
    assert_eq!(graph.edges.len(), 6); // 2 Magic->Atk + 4 Atk->SpEffect
}

#[test]
fn cycles_terminate() {
    let mut src = FixtureSource::new();
    src.insert(
        ParamTable::Magic,
        int_row(
            1,
            &[("refId1", 10), ("refCategory1", 1), ("consumeType1", 0)],
        ),
    );
    src.insert(
        ParamTable::Bullet,
        int_row(10, &[("intervalCreateBulletId", 11)]),
    );
    src.insert(
        ParamTable::Bullet,
        int_row(11, &[("intervalCreateBulletId", 10)]),
    );

    let graph = Walker::new(&src)
        .with_options(WalkOptions::params_only())
        .walk(1)
        .unwrap();

    assert_eq!(graph.nodes_of_kind(NodeKind::Bullet).count(), 2);
    assert_eq!(graph.edges.len(), 3); // Magic->10, 10->11, 11->10
    assert!(graph.classifications.contains(&Classification::ChildBullet));
}

#[test]
fn speffect_chain_cycle_terminates() {
    let mut src = FixtureSource::new();
    src.insert(
        ParamTable::Magic,
        int_row(
            1,
            &[("refId1", 100), ("refCategory1", 2), ("consumeType1", 0)],
        ),
    );
    src.insert(
        ParamTable::SpEffect,
        int_row(100, &[("replaceSpEffectId", 200)]),
    );
    src.insert(
        ParamTable::SpEffect,
        int_row(200, &[("cycleOccurrenceSpEffectId", 100)]),
    );

    let graph = Walker::new(&src)
        .with_options(WalkOptions::params_only())
        .walk(1)
        .unwrap();

    assert_eq!(graph.nodes_of_kind(NodeKind::SpEffect).count(), 2);
    assert!(graph.classifications.contains(&Classification::Status));
}

#[test]
fn subtree_shared_across_cast_types_is_reachable_under_both() {
    let build = |include_charged: bool| {
        let mut src = FixtureSource::new();
        let mut magic = vec![("refId1", 10i64), ("refCategory1", 1), ("consumeType1", 0)];
        if include_charged {
            magic.extend_from_slice(&[("refId2", 10), ("refCategory2", 1), ("consumeType2", 1)]);
        }
        src.insert(ParamTable::Magic, int_row(1, &magic));
        src.insert(ParamTable::Bullet, int_row(10, &[("atkId_Bullet", 20)]));
        src.insert(ParamTable::AtkPc, int_row(20, &[("spEffectId0", 100)]));
        src.insert(ParamTable::SpEffect, int_row(100, &[]));
        src
    };

    let tap_src = build(false);
    let tap_only = Walker::new(&tap_src)
        .with_options(WalkOptions::params_only())
        .walk(1)
        .unwrap();

    let shared_src = build(true);
    let shared = Walker::new(&shared_src)
        .with_options(WalkOptions::params_only())
        .walk(1)
        .unwrap();

    // Structure: exactly one more edge (the second Magic root), same node set.
    assert_eq!(
        shared.edges.len(),
        tap_only.edges.len() + 1,
        "the shared subtree must not be re-walked and duplicated"
    );
    assert_eq!(shared.nodes.len(), tap_only.nodes.len());

    // Semantics: the whole shared subtree carries both cast types.
    for key in [
        NodeKey::new(NodeKind::Bullet, 10),
        NodeKey::new(NodeKind::Atk, 20),
        NodeKey::new(NodeKind::SpEffect, 100),
    ] {
        let casts = shared
            .cast_reachability
            .get(&key)
            .unwrap_or_else(|| panic!("{key} should have cast reachability"));
        assert!(casts.contains(&CastType::Default), "{key} under Default");
        assert!(casts.contains(&CastType::Charged), "{key} under Charged");
    }

    assert_eq!(
        shared.attack_targets_under(CastType::Charged),
        vec![NodeKey::new(NodeKind::Atk, 20)],
        "the charged branch must report its attack row (the C# reported none)"
    );
}

#[test]
fn unknown_ref_category_probes_tables_then_gives_up() {
    let mut src = FixtureSource::new();
    src.insert(
        ParamTable::Magic,
        int_row(
            1,
            &[
                ("refId1", 10),
                ("refCategory1", 77),
                ("consumeType1", 0), // probes -> Bullet
                ("refId2", 20),
                ("refCategory2", 77),
                ("consumeType2", 0), // probes -> Atk
                ("refId3", 30),
                ("refCategory3", 77),
                ("consumeType3", 0), // matches nothing
            ],
        ),
    );
    src.insert(ParamTable::Bullet, int_row(10, &[]));
    src.insert(ParamTable::AtkPc, int_row(20, &[]));

    let graph = Walker::new(&src)
        .with_options(WalkOptions::params_only())
        .walk(1)
        .unwrap();

    let by_slot = |slot: u8| {
        graph
            .out_edges(graph.root())
            .find(|e| e.field.index == Some(slot))
            .unwrap_or_else(|| panic!("slot {slot} edge missing"))
    };
    assert_eq!(by_slot(1).resolution, EdgeResolution::ProbedBullet);
    assert_eq!(by_slot(2).resolution, EdgeResolution::ProbedAtk);
    assert_eq!(by_slot(3).resolution, EdgeResolution::Unresolvable);
    assert!(graph
        .diagnostics
        .iter()
        .any(|d| d.kind == DiagnosticKind::UnresolvableReference));
}

#[test]
fn graph_json_round_trips_and_reindexes() {
    let src = crate::test_support::fixture_from_csvs("GlintstonePebble");
    let graph = Walker::new(&src).walk(4000).unwrap();

    let json = serde_json::to_string(&graph).expect("graph should serialize");
    let mut restored: SpellGraph = serde_json::from_str(&json).expect("graph should deserialize");
    restored.reindex();

    assert_eq!(restored.magic_id, graph.magic_id);
    assert_eq!(restored.nodes.len(), graph.nodes.len());
    assert_eq!(restored.edges, graph.edges);
    assert_eq!(restored.classifications, graph.classifications);
    assert_eq!(restored.cast_reachability, graph.cast_reachability);
    // Payloads are deliberately not persisted; structure is.
    assert!(restored.nodes.iter().all(|n| n.row.is_none()));

    // ...and hydration puts them back.
    restored.hydrate_rows(&src);
    let resolved_before = graph.nodes.iter().filter(|n| n.row.is_some()).count();
    let resolved_after = restored.nodes.iter().filter(|n| n.row.is_some()).count();
    assert_eq!(resolved_after, resolved_before);
}

#[test]
fn missing_magic_row_is_the_only_fatal_case() {
    let src = FixtureSource::new();
    assert!(matches!(
        Walker::new(&src).walk(4000),
        Err(GraphError::MagicRowMissing(4000))
    ));
}

#[test]
fn generated_row_count_counts_only_craftable_rows() {
    let src = crate::test_support::fixture_from_csvs("GlintstonePebble");
    let graph = Walker::new(&src).walk(4000).unwrap();
    // Bullet 10400000, Bullet 10400099 (absent, but still a row a craft would generate),
    // Atk 40000 — FXR and Magic nodes don't consume generated param rows.
    assert_eq!(graph.generated_row_count(), 3);
    assert!(graph.generated_row_count() <= crate::allocator::ROW_BUDGET_PER_SLOT);
}

// --- Presentation ------------------------------------------------------------------

#[test]
fn presentation_is_opt_in_and_does_not_disturb_reference_structure() {
    let src = crate::test_support::fixture_from_csvs("GlintstonePebble");

    let without = Walker::new(&src)
        .with_options(WalkOptions::params_only())
        .walk(4000)
        .unwrap();
    assert!(
        without.presentation.is_none(),
        "presentation must be opt-in"
    );
    assert!(!without.contains(NodeKey::new(NodeKind::Goods, 4000)));

    let with = Walker::new(&src)
        .with_options(WalkOptions::everything())
        .walk(4000)
        .unwrap();
    assert!(with.presentation.is_some());
    assert!(with.contains(NodeKey::new(NodeKind::Goods, 4000)));

    // The Goods edge exists but is explicitly NOT a followed reference.
    let goods_edge = with
        .out_edges(with.root())
        .find(|e| e.to.kind == NodeKind::Goods)
        .expect("Magic -> Goods edge should exist");
    assert_eq!(goods_edge.resolution, EdgeResolution::IdConvention);
    assert_eq!(
        goods_edge.ref_category, None,
        "an id correspondence has no refCategory"
    );

    // ...and the reference subgraph is byte-for-byte what it was.
    assert_projection_matches(&with, &without);
}

#[test]
fn presentation_resolves_a_readable_name_from_paramdex() {
    let src = crate::test_support::fixture_from_csvs("GlintstonePebble");
    let names = souls_format::NameIndex::open_vendored().expect("vendored names should exist");

    let graph = Walker::new(&src)
        .with_options(WalkOptions::everything())
        .with_names(&names)
        .walk(4000)
        .unwrap();

    let p = graph.presentation.as_ref().unwrap();
    assert_eq!(p.name.as_deref(), Some("[Sorcery] Glintstone Pebble"));
    assert_eq!(p.text_source, TextSource::ParamdexNames);
    assert_eq!(
        (p.summary.as_deref(), p.description.as_deref()),
        (None, None),
        "Paramdex names carry no summary/description — those need FMG (Oodle)"
    );
    // Magic 4000 carries iconId=4000; the Goods row isn't in this fixture, so this
    // exercises the Magic-row fallback.
    assert_eq!(p.icon_id, Some(4000));
}

#[test]
fn presentation_without_a_name_source_still_resolves_structure() {
    let src = crate::test_support::fixture_from_csvs("GlintstonePebble");
    let graph = Walker::new(&src)
        .with_options(WalkOptions::everything())
        .walk(4000)
        .unwrap();

    let p = graph.presentation.as_ref().unwrap();
    assert_eq!(p.name, None);
    assert_eq!(p.text_source, TextSource::None);
    assert_eq!(p.icon_id, Some(4000));
    assert_eq!(p.goods, Some(NodeKey::new(NodeKind::Goods, 4000)));
}

#[test]
fn missing_goods_row_is_recorded_not_fatal() {
    let src = crate::test_support::fixture_from_csvs("GlintstonePebble"); // has no EquipParamGoods rows at all
    let graph = Walker::new(&src)
        .with_options(WalkOptions::everything())
        .walk(4000)
        .unwrap();

    let goods = graph.node(NodeKey::new(NodeKind::Goods, 4000)).unwrap();
    assert_eq!(goods.status, NodeStatus::RowMissing);
    assert!(
        graph.presentation.is_some(),
        "presentation still resolves without a Goods row"
    );
}

// --- Real regulation.bin ----------------------------------------------------------

fn regulation_source() -> Option<crate::source::RegulationSource> {
    let path = souls_format::locate::locate_regulation_bin()?;
    let regulation = souls_format::Regulation::open(&path).ok()?;
    let defs = ParamdefLibrary::open_vendored().ok()?;
    Some(crate::source::RegulationSource::new(
        souls_format::ParamBank::new(regulation, defs),
    ))
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn adula_param_projection_is_19_nodes_42_edges() {
    let Some(src) = regulation_source() else {
        eprintln!("skipping: no regulation.bin found");
        return;
    };
    let graph = Walker::new(&src)
        .with_options(WalkOptions::params_only())
        .walk(4431)
        .expect("Adula's Moonblade should walk");

    let mut keys: Vec<String> = graph.nodes.iter().map(|n| n.key.to_string()).collect();
    keys.sort();
    assert_eq!(
        graph.nodes.len(),
        19,
        "expected 19 nodes, got {}: {keys:#?}",
        graph.nodes.len()
    );
    assert_eq!(
        graph.edges.len(),
        42,
        "expected 42 edges, got {}",
        graph.edges.len()
    );

    // The exact expected node set, not just the count.
    let expect = |kind: NodeKind, ids: &[i64]| {
        for &id in ids {
            assert!(
                graph.contains(NodeKey::new(kind, id)),
                "missing {:?}:{id}",
                kind
            );
        }
        assert_eq!(
            graph.nodes_of_kind(kind).count(),
            ids.len(),
            "unexpected {kind:?} node count"
        );
    };
    expect(NodeKind::Magic, &[4431]);
    expect(
        NodeKind::Bullet,
        &[
            10443100, 10443101, 10443110, 10443111, 10443120, 10443121, 10443130, 10443131,
        ],
    );
    expect(
        NodeKind::Atk,
        &[44310, 44311, 44315, 44316, 44317, 44318, 44319],
    );
    expect(NodeKind::SpEffect, &[1443100, 1443101, 6904]);

    // `aiUseJudgeId` = 12000000 sits between refId3 and refId4 in field order and looks
    // exactly like a bullet id. Any positional read of "ten consecutive refIds" would
    // pull it in as a real reference.
    assert!(
        !graph.edges.iter().any(|e| e.to.id == 12_000_000),
        "aiUseJudgeId leaked into the graph as a reference"
    );
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn adula_speffect_leaves_terminate() {
    let Some(src) = regulation_source() else {
        eprintln!("skipping: no regulation.bin found");
        return;
    };
    for id in [1443100i64, 6904] {
        let row = src.row(ParamTable::SpEffect, id);
        let row = row
            .found()
            .unwrap_or_else(|| panic!("SpEffect {id} should decode"));
        for field in [
            "replaceSpEffectId",
            "cycleOccurrenceSpEffectId",
            "atkOccurrenceSpEffectId",
        ] {
            let v = row
                .get_i64(field)
                .unwrap_or_else(|e| panic!("{id}.{field}: {e}"));
            assert!(
                v <= 0,
                "SpEffect {id}.{field} = {v}; the 19/42 derivation assumes it terminates"
            );
        }
    }
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn undecodable_speffect_row_is_recorded_not_fatal() {
    let Some(src) = regulation_source() else {
        eprintln!("skipping: no regulation.bin found");
        return;
    };
    let mut fixture = FixtureSource::new();
    fixture.insert(
        ParamTable::Magic,
        int_row(
            1,
            &[("refId1", 835), ("refCategory1", 2), ("consumeType1", 0)],
        ),
    );

    // Compose: Magic from the fixture, SpEffect from the real regulation.
    struct Split<'a>(&'a FixtureSource, &'a crate::source::RegulationSource);
    impl ParamSource for Split<'_> {
        fn row(&self, table: ParamTable, id: i64) -> souls_format::RowResult {
            match table {
                ParamTable::Magic => self.0.row(table, id),
                _ => self.1.row(table, id),
            }
        }
    }

    let graph = Walker::new(&Split(&fixture, &src))
        .with_options(WalkOptions::params_only())
        .walk(1)
        .expect("a walk reaching an undecodable row must still succeed");

    let node = graph
        .node(NodeKey::new(NodeKind::SpEffect, 835))
        .expect("the node should exist even though its row won't decode");
    assert!(
        matches!(node.status, NodeStatus::RowUndecodable { .. }),
        "expected RowUndecodable, got {:?}",
        node.status
    );
    assert!(graph
        .diagnostics
        .iter()
        .any(|d| d.kind == DiagnosticKind::RowUndecodable));
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn adula_presentation_resolves_against_real_data() {
    let Some(src) = regulation_source() else {
        eprintln!("skipping: no regulation.bin found");
        return;
    };
    let names = souls_format::NameIndex::open_vendored().unwrap();

    let params_only = Walker::new(&src)
        .with_options(WalkOptions::params_only())
        .walk(4431)
        .unwrap();
    let full = Walker::new(&src)
        .with_options(WalkOptions::everything())
        .with_names(&names)
        .walk(4431)
        .unwrap();

    let p = full.presentation.as_ref().expect("presentation requested");
    assert_eq!(p.name.as_deref(), Some("[Sorcery] Adula's Moonblade"));
    assert_eq!(p.text_source, TextSource::ParamdexNames);

    // Unlike the CSV fixture, the real regulation has the Goods row.
    let goods = full.node(NodeKey::new(NodeKind::Goods, 4431)).unwrap();
    assert_eq!(goods.status, NodeStatus::Resolved);
    assert!(
        p.icon_id.is_some_and(|i| i > 0),
        "a real spell should have an icon id"
    );

    // Enabling presentation must not change the reference subgraph — still 19/42.
    let projected = full.param_projection();
    assert_eq!(projected.nodes.len(), 19);
    assert_eq!(projected.edges.len(), 42);
    assert_projection_matches(&full, &params_only);
}

#[test]
#[ignore = "reads an external, non-committed regulation.bin — see docs/known-offsets.md"]
fn adula_full_graph_reaches_sfx_through_the_vfx_bridge() {
    let Some(src) = regulation_source() else {
        eprintln!("skipping: no regulation.bin found");
        return;
    };
    let params_only = Walker::new(&src)
        .with_options(WalkOptions::params_only())
        .walk(4431)
        .unwrap();
    let full = Walker::new(&src).walk(4431).unwrap();

    // SpEffect 1443101 -> vfxId 260 / vfxId1 40260 -> midstSfxId 4270 / 4020.
    assert!(full.contains(NodeKey::new(NodeKind::SpEffectVfx, 260)));
    assert!(full.contains(NodeKey::new(NodeKind::SpEffectVfx, 40260)));
    assert!(full.contains(NodeKey::new(NodeKind::Fxr, 4270)));
    assert!(full.contains(NodeKey::new(NodeKind::Fxr, 4020)));

    assert!(full.nodes.len() > params_only.nodes.len());
    assert_projection_matches(&full, &params_only);
}
