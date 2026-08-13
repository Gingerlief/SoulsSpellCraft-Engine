// Docs: docs/spellcraft-engine/graph/walk.md

use std::collections::HashSet;

use souls_format::RowResult;

use super::model::*;
use crate::source::{ParamSource, ParamTable};
use souls_format::sfx_dir::FxrResult;

pub trait SfxSource {
    fn fxr(&self, sfx_id: i32) -> FxrResult;
}

impl SfxSource for souls_format::SfxDirectory {
    fn fxr(&self, sfx_id: i32) -> FxrResult {
        self.get(sfx_id)
    }
}

// --- The edge table ---------------------------------------------------------------
//
// Every followed field lives here rather than scattered across match arms, so that
// GRAPH_SCHEMA_VERSION has one obvious thing to be bumped alongside. Each entry was
// verified against the vendored PARAMDEF XML.

const MAGIC_SFX_FIELDS: &[&str] = &["castSfxId", "fireSfxId", "effectSfxId"];

const BULLET_CHILD_FIELDS: &[&str] = &["HitBulletID", "intervalCreateBulletId"];

const BULLET_SFX_FIELDS: &[&str] = &[
    "sfxId_Bullet",
    "sfxId_Hit",
    "sfxId_Flick",
    "sfxId_ForceErase",
];

const SPEFFECT_CHAIN_FIELDS: &[&str] = &[
    "replaceSpEffectId",
    "cycleOccurrenceSpEffectId",
    "atkOccurrenceSpEffectId",
];

const SPEFFECT_VFX_SFX_FIELDS: &[&str] = &["initSfxId", "midstSfxId", "finishSfxId"];

const SP_EFFECT_SLOTS: u8 = 5;
const ATK_TRACE_SFX_SLOTS: u8 = 8;
const SPEFFECT_VFX_SLOTS: u8 = 8;
const MAGIC_REF_SLOTS: u8 = 10;

// Fields deliberately NOT followed, having verified they carry flags/types/material ids
// rather than references: Bullet `sfxPostureType`, `bulletSfxDeleteType_*`,
// `isInheritSfxToChild`, `isAttackSFX`, `isIgnoreSfxIfHitWater`,
// `isDisableHitSfx_byChrAndObj`, `followDmypoly_forSfxPose`; Atk `repeatHitSfx`,
// `atkPow_forSfx`, `atkDir_forSfx`, `defSfxMaterial*`; SpEffect `addFootEffectSfxId`
// (foot-material effect, not a spell FXR); SpEffectVfx `*DmyId`.

fn is_valid_reference(id: i64) -> bool {
    id > 0
}

#[derive(Debug, Clone)]
pub struct WalkOptions {
    pub follow_sfx: bool,
    pub follow_speffect_vfx: bool,
    pub include_presentation: bool,
    pub atk_tables: Vec<ParamTable>,
    pub max_depth: Option<u32>,
}

impl Default for WalkOptions {
    fn default() -> Self {
        WalkOptions {
            follow_sfx: true,
            follow_speffect_vfx: true,
            include_presentation: false,
            atk_tables: vec![ParamTable::AtkPc],
            max_depth: None,
        }
    }
}

impl WalkOptions {
    pub fn params_only() -> Self {
        WalkOptions {
            follow_sfx: false,
            follow_speffect_vfx: false,
            include_presentation: false,
            ..Default::default()
        }
    }

    pub fn everything() -> Self {
        WalkOptions {
            include_presentation: true,
            ..Default::default()
        }
    }
}

#[derive(Debug, thiserror::Error)]
pub enum GraphError {
    #[error("Magic row {0} not found")]
    MagicRowMissing(i64),
    #[error("Magic row {id} could not be decoded: {error}")]
    MagicRowUndecodable { id: i64, error: String },
}

pub trait NameSource {
    fn name(&self, list_stem: &str, id: i64) -> Option<String>;
}

impl NameSource for souls_format::NameIndex {
    fn name(&self, list_stem: &str, id: i64) -> Option<String> {
        self.get(list_stem, id)
    }
}

pub struct Walker<'a> {
    source: &'a dyn ParamSource,
    sfx: Option<&'a dyn SfxSource>,
    names: Option<&'a dyn NameSource>,
    options: WalkOptions,
}

impl<'a> Walker<'a> {
    pub fn new(source: &'a dyn ParamSource) -> Self {
        Walker {
            source,
            sfx: None,
            names: None,
            options: WalkOptions::default(),
        }
    }

    pub fn with_names(mut self, names: &'a dyn NameSource) -> Self {
        self.names = Some(names);
        self
    }

    pub fn with_options(mut self, options: WalkOptions) -> Self {
        self.options = options;
        self
    }

    pub fn with_sfx(mut self, sfx: &'a dyn SfxSource) -> Self {
        self.sfx = Some(sfx);
        self
    }

    pub fn walk(&self, magic_id: i64) -> Result<SpellGraph, GraphError> {
        let root = NodeKey::new(NodeKind::Magic, magic_id);
        let magic_row = match self.source.row(ParamTable::Magic, magic_id) {
            RowResult::Found(r) => r,
            RowResult::Missing => return Err(GraphError::MagicRowMissing(magic_id)),
            RowResult::Undecodable { error } => {
                return Err(GraphError::MagicRowUndecodable {
                    id: magic_id,
                    error,
                })
            }
        };

        let mut ctx = WalkCtx {
            graph: SpellGraph::new(magic_id),
            visited: HashSet::new(),
            source: self.source,
            sfx: self.sfx,
            names: self.names,
            options: &self.options,
        };

        let mut magic_node = Node::new(root, NodeStatus::Resolved);
        magic_node.row = Some(magic_row.clone());
        ctx.graph.add_node(magic_node);
        ctx.visited.insert(root);

        // Magic refId1..10, dispatched by refCategory.
        for slot in 1..=MAGIC_REF_SLOTS {
            let Some(target_id) = ctx.field(root, &magic_row, &format!("refId{slot}")) else {
                continue;
            };
            if !is_valid_reference(target_id) {
                continue;
            }

            let category = ctx.field(root, &magic_row, &format!("refCategory{slot}"));
            let consume = ctx.field(root, &magic_row, &format!("consumeType{slot}"));
            let cast_type = CastType::from_consume_type(consume.unwrap_or(0) as i32);
            let field = EdgeField::indexed("refId", slot);

            if consume == Some(1) {
                ctx.graph.classifications.insert(Classification::Charged);
            }

            match category {
                Some(1) => {
                    ctx.graph.classifications.insert(Classification::Projectile);
                    ctx.reference(
                        root,
                        NodeKind::Bullet,
                        target_id,
                        field,
                        cast_type,
                        EdgeResolution::Declared,
                        category.map(|c| c as i32),
                        consume.map(|c| c as i32),
                        None,
                    );
                }
                Some(0) => {
                    ctx.graph
                        .classifications
                        .insert(Classification::DirectAttack);
                    ctx.reference(
                        root,
                        NodeKind::Atk,
                        target_id,
                        field,
                        cast_type,
                        EdgeResolution::Declared,
                        category.map(|c| c as i32),
                        consume.map(|c| c as i32),
                        None,
                    );
                }
                Some(2) => ctx.reference(
                    root,
                    NodeKind::SpEffect,
                    target_id,
                    field,
                    cast_type,
                    EdgeResolution::Declared,
                    category.map(|c| c as i32),
                    consume.map(|c| c as i32),
                    None,
                ),
                other => {
                    // Unrecognized refCategory: probe Bullet then Atk, matching the C#'s
                    // ResolveUnknown ordering.
                    let (kind, resolution) = if ctx.source.has_row(ParamTable::Bullet, target_id) {
                        (NodeKind::Bullet, EdgeResolution::ProbedBullet)
                    } else if ctx.atk_table_for(target_id).is_some() {
                        (NodeKind::Atk, EdgeResolution::ProbedAtk)
                    } else {
                        ctx.graph.diagnose(Diagnostic {
                            node: Some(root),
                            field: Some(format!("refId{slot}")),
                            kind: DiagnosticKind::UnresolvableReference,
                            detail: format!(
                                "refCategory={other:?} is unrecognized and id {target_id} matched no table"
                            ),
                        });
                        ctx.graph.add_edge(Edge {
                            from: root,
                            to: NodeKey::new(NodeKind::Bullet, target_id),
                            field: EdgeField::indexed("refId", slot),
                            ref_category: other.map(|c| c as i32),
                            consume_type: consume.map(|c| c as i32),
                            cast_type,
                            resolution: EdgeResolution::Unresolvable,
                            source_action: None,
                        });
                        continue;
                    };
                    ctx.reference(
                        root,
                        kind,
                        target_id,
                        field,
                        cast_type,
                        resolution,
                        other.map(|c| c as i32),
                        consume.map(|c| c as i32),
                        None,
                    );
                }
            }
        }

        // Magic-level cast/fire/effect visuals.
        if self.options.follow_sfx {
            for name in MAGIC_SFX_FIELDS {
                if let Some(id) = ctx.field(root, &magic_row, name) {
                    if is_valid_reference(id) {
                        ctx.fxr_reference(root, id, EdgeField::plain(name), CastType::Default);
                    }
                }
            }
        }

        if self.options.include_presentation {
            ctx.resolve_presentation(magic_id, &magic_row);
        }

        let mut graph = ctx.graph;
        // Matches the C#: "Status" is set whenever the spell references any SpEffect at
        // all, including one whose row didn't resolve — the reference itself is the fact.
        if graph.nodes_of_kind(NodeKind::SpEffect).next().is_some() {
            graph.classifications.insert(Classification::Status);
        }
        graph.reindex();
        super::cast::annotate_cast_reachability(&mut graph);
        Ok(graph)
    }
}

struct WalkCtx<'a> {
    graph: SpellGraph,
    visited: HashSet<NodeKey>,
    source: &'a dyn ParamSource,
    sfx: Option<&'a dyn SfxSource>,
    names: Option<&'a dyn NameSource>,
    options: &'a WalkOptions,
}

impl WalkCtx<'_> {
    fn resolve_presentation(&mut self, magic_id: i64, magic_row: &souls_format::ParamRow) {
        let root = NodeKey::new(NodeKind::Magic, magic_id);
        let goods_key = NodeKey::new(NodeKind::Goods, magic_id);

        let goods_result = self.source.row(ParamTable::Goods, magic_id);
        let goods_row = goods_result.found().cloned();

        let status = match &goods_result {
            souls_format::RowResult::Found(_) => NodeStatus::Resolved,
            souls_format::RowResult::Missing => NodeStatus::RowMissing,
            souls_format::RowResult::Undecodable { error } => NodeStatus::RowUndecodable {
                error: error.clone(),
            },
        };

        // Not every Magic row has an inventory counterpart (NPC-only spells, unused rows),
        // so a missing Goods row is normal — record it and carry on.
        if !status.is_resolved() {
            self.graph.diagnose(Diagnostic {
                node: Some(goods_key),
                field: None,
                kind: DiagnosticKind::RowMissing,
                detail: format!("no EquipParamGoods row {magic_id}; spell has no inventory item"),
            });
        }

        let mut node = Node::new(goods_key, status);
        node.row = goods_row.clone();
        self.graph.add_node(node);
        self.graph.add_edge(Edge {
            from: root,
            to: goods_key,
            field: EdgeField::plain("<id correspondence>"),
            ref_category: None,
            consume_type: None,
            cast_type: CastType::Default,
            resolution: EdgeResolution::IdConvention,
            source_action: None,
        });

        // Icon: prefer the Goods row (the inventory icon) and fall back to the Magic row,
        // which carries its own `iconId`.
        let icon_id = goods_row
            .as_ref()
            .and_then(|r| r.get_i64("iconId").ok())
            .or_else(|| magic_row.get_i64("iconId").ok())
            .filter(|&id| id > 0);

        let name = self.names.and_then(|n| {
            n.name("EquipParamGoods", magic_id)
                .or_else(|| n.name("Magic", magic_id))
        });
        let text_source = if name.is_some() {
            TextSource::ParamdexNames
        } else {
            TextSource::None
        };

        self.graph.presentation = Some(Presentation {
            goods: Some(goods_key),
            icon_id,
            name,
            summary: None,
            description: None,
            text_source,
        });
    }

    fn field(&mut self, owner: NodeKey, row: &souls_format::ParamRow, name: &str) -> Option<i64> {
        match row.get_i64(name) {
            Ok(v) => Some(v),
            Err(e) => {
                let kind = match e {
                    souls_format::paramdef::FieldError::Missing => DiagnosticKind::FieldMissing,
                    souls_format::paramdef::FieldError::WrongType { .. } => {
                        DiagnosticKind::FieldWrongType
                    }
                };
                self.graph.diagnose(Diagnostic {
                    node: Some(owner),
                    field: Some(name.to_string()),
                    kind,
                    detail: e.to_string(),
                });
                None
            }
        }
    }

    fn atk_table_for(&self, id: i64) -> Option<ParamTable> {
        self.options
            .atk_tables
            .iter()
            .copied()
            .find(|&t| self.source.has_row(t, id))
    }

    #[allow(clippy::too_many_arguments)]
    fn reference(
        &mut self,
        from: NodeKey,
        kind: NodeKind,
        id: i64,
        field: EdgeField,
        cast_type: CastType,
        resolution: EdgeResolution,
        ref_category: Option<i32>,
        consume_type: Option<i32>,
        source_action: Option<u32>,
    ) {
        let to = NodeKey::new(kind, id);
        self.graph.add_edge(Edge {
            from,
            to,
            field,
            ref_category,
            consume_type,
            cast_type,
            resolution,
            source_action,
        });
        self.visit(to);
    }

    fn fxr_reference(&mut self, from: NodeKey, sfx_id: i64, field: EdgeField, cast_type: CastType) {
        self.reference(
            from,
            NodeKind::Fxr,
            sfx_id,
            field,
            cast_type,
            EdgeResolution::Declared,
            None,
            None,
            None,
        );
    }

    fn visit(&mut self, key: NodeKey) {
        if key.kind == NodeKind::Fxr {
            self.visit_fxr(key);
            return;
        }

        // Resolve the row (this is also what determines node status).
        let table = match key.kind {
            NodeKind::Atk => self.atk_table_for(key.id).unwrap_or(ParamTable::AtkPc),
            other => other.table().expect("non-Fxr kinds have a table"),
        };
        if key.kind == NodeKind::Atk && table == ParamTable::AtkNpc {
            self.graph.diagnose(Diagnostic {
                node: Some(key),
                field: None,
                kind: DiagnosticKind::ResolvedFromNpcAtk,
                detail: "resolved from AtkParam_Npc rather than AtkParam_Pc".to_string(),
            });
        }

        let result = self.source.row(table, key.id);
        let (status, row) = match &result {
            RowResult::Found(r) => (NodeStatus::Resolved, Some(r.clone())),
            RowResult::Missing => (NodeStatus::RowMissing, None),
            RowResult::Undecodable { error } => (
                NodeStatus::RowUndecodable {
                    error: error.clone(),
                },
                None,
            ),
        };

        match &status {
            NodeStatus::RowMissing => self.graph.diagnose(Diagnostic {
                node: Some(key),
                field: None,
                kind: DiagnosticKind::RowMissing,
                detail: format!("{key} not present in {}", table.entry_suffix()),
            }),
            NodeStatus::RowUndecodable { error } => self.graph.diagnose(Diagnostic {
                node: Some(key),
                field: None,
                kind: DiagnosticKind::RowUndecodable,
                detail: error.clone(),
            }),
            _ => {}
        }

        let mut node = Node::new(key, status);
        node.row = row.clone();
        self.graph.add_node(node);

        // Gate AFTER the node is added, so a node reached twice still exists exactly once
        // and its incoming edges are all recorded.
        if !self.visited.insert(key) {
            return;
        }

        let Some(row) = row else { return }; // unresolved: recorded, not expanded

        match key.kind {
            NodeKind::Bullet => self.expand_bullet(key, &row),
            NodeKind::Atk => self.expand_atk(key, &row),
            NodeKind::SpEffect => self.expand_speffect(key, &row),
            NodeKind::SpEffectVfx => self.expand_speffect_vfx(key, &row),
            // Magic is expanded by `walk` itself; Fxr goes through `visit_fxr`; Goods is a
            // presentation leaf — nothing in it references another spell row.
            NodeKind::Magic | NodeKind::Fxr | NodeKind::Goods => {}
        }
    }

    fn visit_fxr(&mut self, key: NodeKey) {
        let result = self.sfx.map(|s| s.fxr(key.id as i32));
        let (status, fxr) = match &result {
            None => (NodeStatus::FxrFileMissing, None),
            Some(FxrResult::Found(f)) => (NodeStatus::Resolved, Some(f.clone())),
            Some(FxrResult::Missing) => (NodeStatus::FxrFileMissing, None),
            Some(FxrResult::Unparseable { error }) => (
                NodeStatus::FxrUnparseable {
                    error: error.clone(),
                },
                None,
            ),
        };

        if let Some(FxrResult::Unparseable { error }) = &result {
            self.graph.diagnose(Diagnostic {
                node: Some(key),
                field: None,
                kind: DiagnosticKind::FxrUnparseable,
                detail: error.clone(),
            });
        }

        let mut node = Node::new(key, status);
        node.fxr = fxr.clone();
        self.graph.add_node(node);

        if !self.visited.insert(key) {
            return;
        }

        // Proxy children: an emitter FXR spawning the actually-visible effect.
        if let Some(fxr) = fxr {
            for (ordinal, target) in fxr.proxy_targets().into_iter().enumerate() {
                self.reference(
                    key,
                    NodeKind::Fxr,
                    target as i64,
                    EdgeField::plain("SFXReference.fields1[0]"),
                    CastType::Default,
                    EdgeResolution::Declared,
                    None,
                    None,
                    Some(ordinal as u32),
                );
            }
        }
    }

    fn expand_bullet(&mut self, key: NodeKey, row: &souls_format::ParamRow) {
        if let Some(atk) = self.field(key, row, "atkId_Bullet") {
            if is_valid_reference(atk) {
                self.reference(
                    key,
                    NodeKind::Atk,
                    atk,
                    EdgeField::plain("atkId_Bullet"),
                    CastType::Default,
                    EdgeResolution::Declared,
                    None,
                    None,
                    None,
                );
            }
        }

        for name in BULLET_CHILD_FIELDS {
            if let Some(child) = self.field(key, row, name) {
                if is_valid_reference(child) {
                    self.graph
                        .classifications
                        .insert(Classification::ChildBullet);
                    self.reference(
                        key,
                        NodeKind::Bullet,
                        child,
                        EdgeField::plain(name),
                        CastType::Default,
                        EdgeResolution::Declared,
                        None,
                        None,
                        None,
                    );
                }
            }
        }

        if let Some(sp) = self.field(key, row, "spEffectIDForShooter") {
            if is_valid_reference(sp) {
                self.reference(
                    key,
                    NodeKind::SpEffect,
                    sp,
                    EdgeField::plain("spEffectIDForShooter"),
                    CastType::Default,
                    EdgeResolution::Declared,
                    None,
                    None,
                    None,
                );
            }
        }

        self.expand_speffect_slots(key, row);

        if self.options.follow_sfx {
            for name in BULLET_SFX_FIELDS {
                if let Some(id) = self.field(key, row, name) {
                    if is_valid_reference(id) {
                        self.fxr_reference(key, id, EdgeField::plain(name), CastType::Default);
                    }
                }
            }
        }
    }

    fn expand_atk(&mut self, key: NodeKey, row: &souls_format::ParamRow) {
        self.expand_speffect_slots(key, row);

        if self.options.follow_sfx {
            for slot in 0..ATK_TRACE_SFX_SLOTS {
                if let Some(id) = self.field(key, row, &format!("traceSfxId{slot}")) {
                    if is_valid_reference(id) {
                        self.fxr_reference(
                            key,
                            id,
                            EdgeField::indexed("traceSfxId", slot),
                            CastType::Default,
                        );
                    }
                }
            }
        }
    }

    fn expand_speffect_slots(&mut self, key: NodeKey, row: &souls_format::ParamRow) {
        for slot in 0..SP_EFFECT_SLOTS {
            if let Some(sp) = self.field(key, row, &format!("spEffectId{slot}")) {
                if is_valid_reference(sp) {
                    self.reference(
                        key,
                        NodeKind::SpEffect,
                        sp,
                        EdgeField::indexed("spEffectId", slot),
                        CastType::Default,
                        EdgeResolution::Declared,
                        None,
                        None,
                        None,
                    );
                }
            }
        }
    }

    fn expand_speffect(&mut self, key: NodeKey, row: &souls_format::ParamRow) {
        for name in SPEFFECT_CHAIN_FIELDS {
            if let Some(next) = self.field(key, row, name) {
                if is_valid_reference(next) {
                    self.reference(
                        key,
                        NodeKind::SpEffect,
                        next,
                        EdgeField::plain(name),
                        CastType::Default,
                        EdgeResolution::Declared,
                        None,
                        None,
                        None,
                    );
                }
            }
        }

        if !self.options.follow_speffect_vfx {
            return;
        }
        // `vfxId` then `vfxId1..7` — note the first has no numeric suffix.
        for slot in 0..SPEFFECT_VFX_SLOTS {
            let name = if slot == 0 {
                "vfxId".to_string()
            } else {
                format!("vfxId{slot}")
            };
            if let Some(vfx) = self.field(key, row, &name) {
                if is_valid_reference(vfx) {
                    self.reference(
                        key,
                        NodeKind::SpEffectVfx,
                        vfx,
                        EdgeField::indexed("vfxId", slot),
                        CastType::Default,
                        EdgeResolution::Declared,
                        None,
                        None,
                        None,
                    );
                }
            }
        }
    }

    fn expand_speffect_vfx(&mut self, key: NodeKey, row: &souls_format::ParamRow) {
        if !self.options.follow_sfx {
            return;
        }
        for name in SPEFFECT_VFX_SFX_FIELDS {
            if let Some(id) = self.field(key, row, name) {
                if is_valid_reference(id) {
                    self.fxr_reference(key, id, EdgeField::plain(name), CastType::Default);
                }
            }
        }
    }
}
