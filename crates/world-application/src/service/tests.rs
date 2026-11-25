use super::*;
use crate::dto::{
    Actor, DeltaCounts, ObservationMode, ObservationRequest, OperationRequest, Query, QueryResult,
    Slice, SliceAxis, ValidationQuery,
};
use crate::{FloatingFix, Severity, ValidationRuleId};
use world_domain::{BiomeSection, BiomeState, BlockBox, BlockPos, BlockState, ChunkPos};
use world_operations::Operation;

fn state(name: &str) -> BlockState {
    name.parse().expect("valid state")
}

fn actor() -> Actor {
    Actor::ui("test-user")
}

fn request(operation: Operation) -> OperationRequest {
    OperationRequest {
        project_id: "project-1".to_owned(),
        operation,
        expected_revision: None,
        idempotency_key: None,
        actor: actor(),
        linked_object_id: None,
        layer_id: None,
    }
}

fn set_block(pos: BlockPos, name: &str) -> OperationRequest {
    request(Operation::SetBlock {
        dimension: "minecraft:overworld".to_owned(),
        pos,
        state: state(name),
    })
}

#[test]
fn execute_updates_revision_query_and_audit() {
    let mut service = WorldService::new();
    let result = service
        .execute(set_block(BlockPos::new(0, 64, 0), "minecraft:stone"))
        .unwrap();
    assert_eq!(result.changed_blocks, 1);
    assert_eq!(result.world_revision, 1);
    assert!(!result.replayed);
    assert_eq!(service.world_info().block_count, 1);

    let query = service
        .query(Query::GetBlock {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(0, 64, 0),
        })
        .unwrap();
    match query {
        QueryResult::Block { state, .. } => {
            assert_eq!(state, self::state("minecraft:stone"))
        }
        other => panic!("unexpected query result: {other:?}"),
    }

    assert_eq!(service.history().len(), 1);
    assert_eq!(service.audit_log().len(), 1);
    let audit = &service.audit_log()[0];
    assert_eq!(audit.actor_id, "test-user");
    assert_eq!(audit.action, AuditAction::Apply);
    assert_eq!(audit.revision_before, 0);
    assert_eq!(audit.revision_after, 1);
    assert_eq!(audit.parameter_summary["type"], "set_block");
}

#[test]
fn catalog_write_registry_admits_catalog_blocks_while_quick_picks_stay_curated() {
    let mut service = WorldService::new();
    service.set_write_registry(BlockRegistry::from_schema([
        world_domain::BlockSchemaEntry {
            name: "minecraft:stone".to_owned(),
            properties: Default::default(),
            values: Default::default(),
            property_values_complete: false,
            default_properties: Default::default(),
        },
        world_domain::BlockSchemaEntry {
            name: "minecraft:chiseled_deepslate".to_owned(),
            properties: Default::default(),
            values: Default::default(),
            property_values_complete: false,
            default_properties: Default::default(),
        },
    ]));
    assert!(service.registry().is_known("minecraft:stone"));
    assert!(!service.registry().is_known("minecraft:chiseled_deepslate"));

    service
        .execute(set_block(
            BlockPos::new(1, 64, 1),
            "minecraft:chiseled_deepslate",
        ))
        .expect("catalog block is writable");
    let query = service
        .query(Query::GetBlock {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(1, 64, 1),
        })
        .unwrap();
    match query {
        QueryResult::Block { state, .. } => {
            assert_eq!(state, self::state("minecraft:chiseled_deepslate"))
        }
        other => panic!("unexpected query result: {other:?}"),
    }

    let bounds = BlockBox::new(BlockPos::new(1, 64, 1), BlockPos::new(1, 64, 1)).unwrap();
    let mut validation = validation_query(bounds);
    validation.rules = vec![ValidationRuleId::ExportCompatibility];
    let report = service.validate_region(&validation).unwrap();
    assert!(report.findings.is_empty(), "{:?}", report.findings);
}

#[test]
fn preview_never_mutates_world_or_history() {
    let service = WorldService::new();
    let preview = service
        .preview(
            "project-1",
            &Operation::FillCuboid {
                dimension: "minecraft:overworld".to_owned(),
                bounds: BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(4, 67, 4)).unwrap(),
                state: state("minecraft:stone_bricks"),
            },
        )
        .unwrap();
    assert_eq!(preview.affected_blocks, 5 * 4 * 5);
    assert_eq!(preview.changed_blocks, 100);
    assert_eq!(service.world_info().revision, 0);
    assert_eq!(service.world_info().block_count, 0);
    assert!(service.history().is_empty());
}

#[test]
fn stale_expected_revision_is_rejected_without_writes() {
    let mut service = WorldService::new();
    let mut first = set_block(BlockPos::new(0, 64, 0), "minecraft:stone");
    first.expected_revision = Some(42);
    let error = service.execute(first).unwrap_err();
    assert_eq!(error.code, ErrorCode::RevisionMismatch);
    assert_eq!(service.world_info().block_count, 0);
    assert_eq!(service.world_info().revision, 0);
}

#[test]
fn idempotent_replay_does_not_apply_twice() {
    let mut service = WorldService::new();
    let mut request = set_block(BlockPos::new(0, 64, 0), "minecraft:stone");
    request.idempotency_key = Some("request-1".to_owned());
    let first = service.execute(request.clone()).unwrap();
    let second = service.execute(request).unwrap();

    assert!(!first.replayed);
    assert!(second.replayed);
    assert_eq!(first.operation_id, second.operation_id);
    assert_eq!(first.world_revision, second.world_revision);
    assert_eq!(service.world_info().block_count, 1);
    assert_eq!(service.world_info().revision, 1);
    assert_eq!(service.history().len(), 1);
}

#[test]
fn reusing_idempotency_key_with_different_payload_conflicts() {
    let mut service = WorldService::new();
    let mut first = set_block(BlockPos::new(0, 64, 0), "minecraft:stone");
    first.idempotency_key = Some("request-1".to_owned());
    service.execute(first).unwrap();

    let mut second = set_block(BlockPos::new(0, 64, 0), "minecraft:dirt");
    second.idempotency_key = Some("request-1".to_owned());
    let error = service.execute(second).unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(
        service
            .query(Query::GetBlock {
                dimension: "minecraft:overworld".to_owned(),
                pos: BlockPos::new(0, 64, 0),
            })
            .unwrap(),
        QueryResult::Block {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(0, 64, 0),
            state: state("minecraft:stone"),
        }
    );
}

#[test]
fn undo_restores_ten_thousand_blocks_and_redo_reapplies() {
    let mut service = WorldService::new();
    service
        .execute(set_block(BlockPos::new(0, 64, 0), "minecraft:stone"))
        .unwrap();
    let fill = request(Operation::FillCuboid {
        dimension: "minecraft:overworld".to_owned(),
        bounds: BlockBox::new(BlockPos::new(-10, 60, -10), BlockPos::new(10, 70, 10)).unwrap(),
        state: state("minecraft:stone_bricks"),
    });
    let applied = service.execute(fill).unwrap();
    assert_eq!(applied.changed_blocks, 21 * 11 * 21);
    assert_eq!(service.world_info().block_count, 21 * 11 * 21);

    let undo = service
        .undo("project-1", actor(), None)
        .expect("undo must succeed");
    assert_eq!(undo.changed_blocks, 21 * 11 * 21);
    assert_eq!(service.world_info().block_count, 1);
    assert_eq!(
        service
            .query(Query::GetBlock {
                dimension: "minecraft:overworld".to_owned(),
                pos: BlockPos::new(0, 64, 0),
            })
            .unwrap(),
        QueryResult::Block {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(0, 64, 0),
            state: state("minecraft:stone"),
        }
    );
    assert!(service.can_redo());

    let redo = service
        .redo("project-1", actor())
        .expect("redo must succeed");
    assert_eq!(redo.changed_blocks, 21 * 11 * 21);
    assert_eq!(service.world_info().block_count, 21 * 11 * 21);
    assert!(!service.can_redo());
}

#[test]
fn undo_target_reports_the_exact_operation_extent_without_mutating() {
    let mut service = WorldService::new();
    service
        .execute(request(Operation::Batch {
            dimension: "minecraft:overworld".to_owned(),
            changes: vec![
                BlockChange {
                    pos: BlockPos::new(-2, 64, 5),
                    state: state("minecraft:stone"),
                },
                BlockChange {
                    pos: BlockPos::new(3, 70, -1),
                    state: state("minecraft:glass"),
                },
            ],
        }))
        .unwrap();
    let revision = service.world().revision();
    let target = service.undo_target(None).expect("undo target");
    assert_eq!(target.dimension, "minecraft:overworld");
    assert_eq!(
        target.bounds,
        BlockBox::new(BlockPos::new(-2, 64, -1), BlockPos::new(3, 70, 5)).unwrap()
    );
    assert_eq!(service.world().revision(), revision);
    assert!(service.can_undo());

    let error = service.undo_target(Some("op-missing")).unwrap_err();
    assert_eq!(error.code, ErrorCode::NoHistory);
}

#[test]
fn read_only_policy_rejects_writes_but_allows_queries() {
    let mut service = WorldService::new();
    service.set_writes_enabled(false);
    let error = service
        .execute(set_block(BlockPos::new(0, 64, 0), "minecraft:stone"))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::PermissionDenied);
    assert_eq!(
        service.query(Query::WorldInfo).unwrap(),
        QueryResult::WorldInfo(service.world_info())
    );
    service.set_writes_enabled(true);
    assert!(service
        .execute(set_block(BlockPos::new(0, 64, 0), "minecraft:stone"))
        .is_ok());
}

#[test]
fn unknown_target_states_are_rejected_but_imported_predicates_are_allowed() {
    let mut service = WorldService::new();
    let error = service
        .execute(set_block(
            BlockPos::new(0, 64, 0),
            "minecraft:not_a_real_block",
        ))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);

    let replace = request(Operation::Replace {
        dimension: "minecraft:overworld".to_owned(),
        bounds: BlockBox::single(BlockPos::new(0, 64, 0)),
        from: state("modded:unknown_block"),
        to: state("minecraft:stone"),
    });
    assert_eq!(service.execute(replace).unwrap().changed_blocks, 0);
}

#[test]
fn region_summary_reports_palette_and_void_blocks() {
    let mut service = WorldService::new();
    service
        .execute(request(Operation::FillCuboid {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(1, 64, 1)).unwrap(),
            state: state("minecraft:stone"),
        }))
        .unwrap();
    let summary = service
        .query(Query::RegionSummary {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(2, 64, 1)).unwrap(),
        })
        .unwrap();
    match summary {
        QueryResult::RegionSummary(summary) => {
            assert_eq!(summary.blocks, 6);
            assert_eq!(summary.non_air_blocks, 4);
            assert_eq!(summary.palette[0].state, "minecraft:stone");
            assert_eq!(summary.palette[0].count, 4);
            assert_eq!(summary.palette[1].state, "minecraft:air");
            assert_eq!(summary.palette[1].count, 2);
        }
        other => panic!("unexpected query result: {other:?}"),
    }
}

#[test]
fn batch_out_of_bounds_is_atomic() {
    let mut service = WorldService::new();
    let operation = Operation::Batch {
        dimension: "minecraft:overworld".to_owned(),
        changes: vec![
            world_operations::BlockChange {
                pos: BlockPos::new(0, 64, 0),
                state: state("minecraft:stone"),
            },
            world_operations::BlockChange {
                pos: BlockPos::new(0, 400, 0),
                state: state("minecraft:stone"),
            },
        ],
    };
    let error = service.execute(request(operation)).unwrap_err();
    assert_eq!(error.code, ErrorCode::OutOfBounds);
    assert_eq!(service.world_info().block_count, 0);
    assert_eq!(service.world_info().revision, 0);
}

#[test]
fn no_op_operations_do_not_bump_revision_or_history() {
    let mut service = WorldService::new();
    let result = service
        .execute(set_block(BlockPos::new(0, 64, 0), "minecraft:air"))
        .unwrap();
    assert_eq!(result.changed_blocks, 0);
    assert_eq!(result.world_revision, 0);
    assert!(service.history().is_empty());
    assert_eq!(service.world_info().revision, 0);
}

#[test]
fn history_is_bounded_and_oldest_entries_are_pruned() {
    let mut service = WorldService::new().with_max_history(2);
    for index in 0..3 {
        service
            .execute(set_block(BlockPos::new(index, 64, 0), "minecraft:stone"))
            .unwrap();
    }
    assert_eq!(service.history().len(), 2);
    service.undo("project-1", actor(), None).unwrap();
    service.undo("project-1", actor(), None).unwrap();
    let error = service.undo("project-1", actor(), None).unwrap_err();
    assert_eq!(error.code, ErrorCode::NoHistory);
}

#[test]
fn dimensions_are_isolated_in_commands_and_queries() {
    let mut service = WorldService::new();
    service
        .execute(request(Operation::SetBlock {
            dimension: "minecraft:the_nether".to_owned(),
            pos: BlockPos::new(0, 64, 0),
            state: state("minecraft:netherrack"),
        }))
        .unwrap();
    let overworld = service
        .query(Query::GetBlock {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(0, 64, 0),
        })
        .unwrap();
    assert_eq!(
        overworld,
        QueryResult::Block {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(0, 64, 0),
            state: state("minecraft:air"),
        }
    );
    let chunk = service
        .query(Query::ChunkStats {
            dimension: "minecraft:the_nether".to_owned(),
            chunk: ChunkPos::new(0, 0),
        })
        .unwrap();
    assert!(matches!(
        chunk,
        QueryResult::ChunkStats(ChunkStats {
            blocks: 1,
            loaded: true,
            ..
        })
    ));
}

#[test]
fn region_blocks_returns_a_sparse_copy_payload() {
    let mut service = WorldService::new();
    service
        .execute(request(Operation::FillCuboid {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(1, 64, 0)).unwrap(),
            state: state("minecraft:stone_bricks"),
        }))
        .unwrap();
    let result = service
        .query(Query::RegionBlocks {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(-1, 63, -1), BlockPos::new(1, 64, 1)).unwrap(),
        })
        .unwrap();
    match result {
        QueryResult::RegionBlocks(region) => {
            assert_eq!(region.volume, 18);
            assert_eq!(region.blocks.len(), 2);
            assert!(region
                .blocks
                .iter()
                .all(|change| change.state == state("minecraft:stone_bricks")));
            assert!(region
                .blocks
                .iter()
                .all(|change| change.pos.y == 64 && change.pos.z == 0));
        }
        other => panic!("unexpected query result: {other:?}"),
    }
}

#[test]
fn region_blocks_limit_is_enforced_before_reading() {
    let service = WorldService::new();
    let error = service
        .query(Query::RegionBlocks {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 0, 0), BlockPos::new(50, 50, 50)).unwrap(),
        })
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::LimitExceeded);
}

#[test]
fn terrain_brush_is_revisioned_audited_and_reversible() {
    let mut service = WorldService::new();
    let base = service
        .execute(request(Operation::FillCuboid {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(1, 64, 1)).unwrap(),
            state: state("minecraft:stone"),
        }))
        .unwrap();
    assert_eq!(base.world_revision, 1);

    let applied = service
        .execute(request(Operation::TerrainBrush {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(1, 70, 1)).unwrap(),
            kind: world_operations::TerrainBrushKind::Raise,
            material: state("minecraft:dirt"),
            amount: 2,
            seed: 0,
            step: 4,
            target_y: None,
        }))
        .unwrap();
    assert_eq!(applied.changed_blocks, 8);
    assert_eq!(applied.world_revision, 2);
    let summary = &service
        .audit_log()
        .last()
        .expect("audit entry")
        .parameter_summary;
    assert_eq!(summary["type"], "terrain_brush");
    assert_eq!(summary["kind"], "raise");
    assert_eq!(summary["seed"], 0);

    match service
        .query(Query::GetBlock {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(0, 65, 0),
        })
        .unwrap()
    {
        QueryResult::Block { state, .. } => assert_eq!(state, self::state("minecraft:dirt")),
        other => panic!("unexpected query result: {other:?}"),
    }

    let undo = service
        .undo("project-1", actor(), Some(&applied.operation_id))
        .unwrap();
    assert_eq!(undo.changed_blocks, 8);
    match service
        .query(Query::GetBlock {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(0, 65, 0),
        })
        .unwrap()
    {
        QueryResult::Block { state, .. } => assert_eq!(state, self::state("minecraft:air")),
        other => panic!("unexpected query result: {other:?}"),
    }
}

#[test]
fn invalid_terrain_brush_is_rejected_without_history_or_revision() {
    let mut service = WorldService::new();
    let error = service
        .execute(request(Operation::TerrainBrush {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(1, 70, 1)).unwrap(),
            kind: world_operations::TerrainBrushKind::Raise,
            material: state("minecraft:air"),
            amount: 1,
            seed: 0,
            step: 4,
            target_y: None,
        }))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(service.world_info().revision, 0);
    assert!(service.history().is_empty());
}

#[test]
fn heightmap_generation_is_revisioned_audited_and_reversible() {
    let mut service = WorldService::new();
    let bounds = BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(1, 70, 1)).unwrap();
    let base = service
        .execute(request(Operation::FillCuboid {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(1, 63, 1)).unwrap(),
            state: state("minecraft:stone"),
        }))
        .unwrap();
    assert_eq!(base.world_revision, 1);

    let applied = service
        .execute(request(Operation::GenerateHeightmap {
            dimension: "minecraft:overworld".to_owned(),
            bounds,
            seed: 0x5EED,
            scale: 4,
            octaves: 3,
            amplitude: 7,
            base_y: 60,
            surface_depth: 2,
            surface: state("minecraft:grass_block"),
            filler: state("minecraft:dirt"),
        }))
        .unwrap();
    assert!(applied.changed_blocks > 0);
    assert_eq!(applied.world_revision, 2);
    let summary = &service
        .audit_log()
        .last()
        .expect("audit entry")
        .parameter_summary;
    assert_eq!(summary["type"], "generate_heightmap");
    assert_eq!(summary["seed"], 0x5EED);
    assert_eq!(summary["scale"], 4);
    assert_eq!(summary["baseY"], 60);
    assert_eq!(summary["surfaceDepth"], 2);
    assert_eq!(summary["surface"], "minecraft:grass_block");

    match service
        .query(Query::GetBlock {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(0, 70, 0),
        })
        .unwrap()
    {
        QueryResult::Block { state, .. } => assert_eq!(state, self::state("minecraft:air")),
        other => panic!("unexpected query result: {other:?}"),
    }

    let undo = service
        .undo("project-1", actor(), Some(&applied.operation_id))
        .unwrap();
    assert_eq!(undo.changed_blocks, applied.changed_blocks);
    assert_eq!(service.world_info().revision, 3);
    match service
        .query(Query::GetBlock {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(0, 63, 0),
        })
        .unwrap()
    {
        QueryResult::Block { state, .. } => assert_eq!(state, self::state("minecraft:stone")),
        other => panic!("unexpected query result: {other:?}"),
    }
}

#[test]
fn invalid_heightmap_is_rejected_without_history_or_revision() {
    let mut service = WorldService::new();
    let error = service
        .execute(request(Operation::GenerateHeightmap {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(1, 70, 1)).unwrap(),
            seed: 0,
            scale: 8,
            octaves: 3,
            amplitude: 8,
            base_y: 65,
            surface_depth: 1,
            surface: state("minecraft:grass_block"),
            filler: state("minecraft:dirt"),
        }))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert_eq!(service.world_info().revision, 0);
    assert!(service.history().is_empty());
}

fn biome(name: &str) -> BiomeState {
    name.parse().expect("valid biome")
}

#[test]
fn biome_paint_is_revisioned_audited_and_reversible() {
    let mut service = WorldService::new();
    service
        .world
        .install_biomes(
            "minecraft:overworld",
            ChunkPos::new(0, 0),
            0,
            Some(BiomeSection::uniform(biome("minecraft:plains"))),
        )
        .unwrap();
    let applied = service
        .execute(request(Operation::SetBiome {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 0, 0), BlockPos::new(7, 7, 7)).unwrap(),
            biome: biome("minecraft:forest"),
        }))
        .unwrap();
    assert_eq!(applied.changed_blocks, 512);
    assert_eq!(applied.world_revision, 1);
    assert_eq!(applied.dirty_chunks, vec![ChunkPos::new(0, 0)]);
    let summary = &service.audit_log().last().expect("audit").parameter_summary;
    assert_eq!(summary["type"], "set_biome");
    assert_eq!(summary["biome"], "minecraft:forest");

    match service
        .query(Query::GetBiome {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(0, 0, 0),
        })
        .unwrap()
    {
        QueryResult::Biome { biome, .. } => {
            assert_eq!(biome, Some(self::biome("minecraft:forest")))
        }
        other => panic!("unexpected query result: {other:?}"),
    }

    let undo = service
        .undo("project-1", actor(), Some(&applied.operation_id))
        .unwrap();
    assert_eq!(undo.changed_blocks, 512);
    assert_eq!(service.world_info().revision, 2);
    match service
        .query(Query::GetBiome {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(0, 0, 0),
        })
        .unwrap()
    {
        QueryResult::Biome { biome, .. } => {
            assert_eq!(biome, Some(self::biome("minecraft:plains")))
        }
        other => panic!("unexpected query result: {other:?}"),
    }
}

#[test]
fn biome_paint_without_biome_data_is_rejected_without_side_effects() {
    let mut service = WorldService::new();
    let error = service
        .execute(request(Operation::SetBiome {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 0, 0), BlockPos::new(7, 7, 7)).unwrap(),
            biome: biome("minecraft:forest"),
        }))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert!(error.message.contains("biome data is missing"));
    assert_eq!(service.world_info().revision, 0);
    assert!(service.history().is_empty());
}

#[test]
fn road_and_vegetation_generators_are_audited_and_reversible() {
    let mut service = WorldService::new();
    service
        .execute(request(Operation::FillCuboid {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(7, 64, 7)).unwrap(),
            state: state("minecraft:grass_block"),
        }))
        .unwrap();
    let road = service
        .execute(request(Operation::GenerateRoad {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(7, 70, 7)).unwrap(),
            from: BlockPos::new(0, 64, 3),
            to: BlockPos::new(7, 64, 3),
            width: 1,
            meander: 0,
            seed: 0x5EED,
            clearance: 2,
            surface: state("minecraft:stone_bricks"),
        }))
        .unwrap();
    assert_eq!(road.changed_blocks, 8);
    assert_eq!(road.world_revision, 2);
    let summary = &service.audit_log().last().expect("audit").parameter_summary;
    assert_eq!(summary["type"], "generate_road");
    assert_eq!(summary["width"], 1);
    assert_eq!(summary["surface"], "minecraft:stone_bricks");

    let vegetation = service
        .execute(request(Operation::GenerateVegetation {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(7, 70, 7)).unwrap(),
            seed: 0x5EED,
            ground_cover: 100,
            tree_chance: 0,
            tree_spacing: 5,
            plant: state("minecraft:short_grass"),
            trunk: state("minecraft:oak_log"),
            leaves: state("minecraft:oak_leaves"),
        }))
        .unwrap();
    assert_eq!(vegetation.changed_blocks, 64);
    assert_eq!(vegetation.world_revision, 3);
    let summary = &service.audit_log().last().expect("audit").parameter_summary;
    assert_eq!(summary["type"], "generate_vegetation");
    assert_eq!(summary["treeChance"], 0);
    assert_eq!(summary["plant"], "minecraft:short_grass");

    service.undo("project-1", actor(), None).unwrap();
    service.undo("project-1", actor(), None).unwrap();
    match service
        .query(Query::GetBlock {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(0, 64, 3),
        })
        .unwrap()
    {
        QueryResult::Block { state, .. } => {
            assert_eq!(state.name(), "minecraft:grass_block");
        }
        other => panic!("unexpected query result: {other:?}"),
    }
    match service
        .query(Query::GetBlock {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(0, 65, 3),
        })
        .unwrap()
    {
        QueryResult::Block { state, .. } => {
            assert_eq!(state.name(), "minecraft:air");
        }
        other => panic!("unexpected query result: {other:?}"),
    }
}

#[test]
fn structure_site_query_is_bounded_and_does_not_mutate_the_world() {
    let mut service = WorldService::new();
    service
        .execute(request(Operation::FillCuboid {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(15, 64, 15)).unwrap(),
            state: state("minecraft:stone"),
        }))
        .unwrap();
    let revision = service.world_info().revision;
    let result = service
        .query(Query::FindStructureSites {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(15, 70, 15)).unwrap(),
            width: 3,
            depth: 3,
            step: 3,
            max_slope: 0,
            clearance: 2,
            limit: 4,
        })
        .unwrap();
    match result {
        QueryResult::StructureSites(result) => {
            assert_eq!(result.sites.len(), 4);
            assert_eq!(result.limit, 4);
            assert!(result.scanned_candidates >= 4);
            assert!(result.sites.iter().all(|site| site.origin.y == 65));
        }
        other => panic!("unexpected query result: {other:?}"),
    }
    assert_eq!(service.world_info().revision, revision);
    assert_eq!(service.history().len(), 1);
}

#[test]
fn invalid_generator_parameters_are_rejected_without_history() {
    let mut service = WorldService::new();
    let error = service
        .execute(request(Operation::GenerateRiver {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(7, 70, 7)).unwrap(),
            from: BlockPos::new(0, 64, 0),
            to: BlockPos::new(7, 64, 7),
            width: 1,
            meander: 0,
            seed: 0,
            depth: 0,
            water: state("minecraft:water[level=0]"),
            bed: state("minecraft:sand"),
        }))
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);
    assert!(error.message.contains("river depth"));
    assert_eq!(service.world_info().revision, 0);
    assert!(service.history().is_empty());
}

#[test]
fn workspace_policy_is_queryable_and_protected_regions_block_writes() {
    use workspace_model::WorkspacePolicy;

    let mut service = WorldService::new();
    let first = service
        .execute(set_block(BlockPos::new(0, 64, 0), "minecraft:stone"))
        .unwrap();
    assert_eq!(first.world_revision, 1);

    let mut policy = WorkspacePolicy::default();
    policy
        .push_protected_region(
            "Spawn",
            "minecraft:overworld",
            BlockBox::new(BlockPos::new(-1, 63, -1), BlockPos::new(1, 65, 1)).unwrap(),
        )
        .unwrap();
    service.set_workspace_policy(policy.clone()).unwrap();
    assert_eq!(
        service.world_info().revision,
        1,
        "policy changes are not world edits"
    );

    match service.query(Query::WorkspacePolicy).unwrap() {
        QueryResult::WorkspacePolicy(returned) => assert_eq!(returned, policy),
        other => panic!("unexpected query result: {other:?}"),
    }

    let blocked = service
        .execute(set_block(BlockPos::new(0, 64, 0), "minecraft:glass"))
        .unwrap_err();
    assert_eq!(blocked.code, ErrorCode::PermissionDenied);
    assert!(blocked.message.contains("protected region `Spawn`"));
    assert_eq!(service.world_info().revision, 1);
    assert_eq!(service.history().len(), 1);
    assert_eq!(service.audit_log().len(), 1);
    assert_eq!(
        service
            .preview(
                "project-1",
                &Operation::SetBlock {
                    dimension: "minecraft:overworld".to_owned(),
                    pos: BlockPos::new(0, 64, 0),
                    state: state("minecraft:glass"),
                }
            )
            .unwrap_err()
            .code,
        ErrorCode::PermissionDenied
    );

    // Undo is a restoration, not a new construction decision; protection does not
    // strand existing history.
    let undone = service.undo("project-1", actor(), None).unwrap();
    assert_eq!(undone.world_revision, 2);
    assert_eq!(
        service
            .world()
            .get_block("minecraft:overworld", BlockPos::new(0, 64, 0))
            .unwrap(),
        state("minecraft:air")
    );

    // A second layer is recorded on new operations and can be locked independently.
    let mut policy = service.workspace_policy();
    policy.push_layer("Details").unwrap();
    policy.layers[0].locked = true;
    service.set_workspace_policy(policy).unwrap();
    let blocked = service
        .execute(set_block(BlockPos::new(3, 64, 3), "minecraft:stone"))
        .unwrap_err();
    assert_eq!(blocked.code, ErrorCode::PermissionDenied);
    assert!(blocked.message.contains("locked"));

    let mut policy = service.workspace_policy();
    policy.layers[0].locked = false;
    policy.active_layer_id = Some("details".to_owned());
    service.set_workspace_policy(policy).unwrap();
    service
        .execute(set_block(BlockPos::new(3, 64, 3), "minecraft:stone"))
        .unwrap();
    let history = service.history();
    assert_eq!(history.last().unwrap().layer_id.as_deref(), Some("details"));
    assert_eq!(
        service.audit_log().last().unwrap().layer_id.as_deref(),
        Some("details")
    );

    let mut policy = service.workspace_policy();
    policy.active_mask_id = Some("missing".to_owned());
    assert_eq!(
        service.set_workspace_policy(policy).unwrap_err().code,
        ErrorCode::InvalidArgument
    );
}

#[test]
fn active_mask_bounds_operations_and_unknown_layers_are_rejected() {
    use workspace_model::WorkspacePolicy;

    let mut service = WorldService::new();
    let mut policy = WorkspacePolicy::default();
    let mask = policy
        .push_mask(
            "Allowed",
            "minecraft:overworld",
            BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(15, 70, 15)).unwrap(),
        )
        .unwrap()
        .id
        .clone();
    policy.active_mask_id = Some(mask);
    service.set_workspace_policy(policy).unwrap();

    let outside = service
        .execute(set_block(BlockPos::new(20, 64, 0), "minecraft:stone"))
        .unwrap_err();
    assert_eq!(outside.code, ErrorCode::PermissionDenied);
    assert!(outside.message.contains("active mask `Allowed`"));
    assert_eq!(service.world_info().revision, 0);

    service
        .execute(set_block(BlockPos::new(5, 64, 5), "minecraft:stone"))
        .unwrap();
    assert_eq!(service.world_info().revision, 1);

    let mut request = set_block(BlockPos::new(6, 64, 6), "minecraft:stone");
    request.layer_id = Some("missing-layer".to_owned());
    let unknown = service.execute(request).unwrap_err();
    assert_eq!(unknown.code, ErrorCode::InvalidArgument);
    assert!(unknown.message.contains("does not exist"));
    assert_eq!(service.world_info().revision, 1);

    let mut policy = service.workspace_policy();
    policy
        .push_mask(
            "Wrong dimension",
            "minecraft:moon",
            BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(1, 70, 1)).unwrap(),
        )
        .unwrap();
    assert_eq!(
        service.set_workspace_policy(policy).unwrap_err().code,
        ErrorCode::NotFound
    );
}

#[test]
fn scene_objects_are_queryable_and_inspect_world_blocks_without_mutating() {
    use scene_model::ObjectKind;
    use std::collections::BTreeMap;

    let mut service = WorldService::new();
    service
        .execute(set_block(BlockPos::new(4, 64, 4), "minecraft:stone"))
        .unwrap();
    service
        .execute(set_block(BlockPos::new(5, 64, 4), "minecraft:glass"))
        .unwrap();
    let revision = service.world_info().revision;

    let district = service
        .add_scene_object(
            "Old Town",
            ObjectKind::District,
            "minecraft:overworld",
            BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(16, 80, 16)).unwrap(),
            None,
            BTreeMap::new(),
            Some("Historic center".to_owned()),
        )
        .unwrap();
    let building = service
        .add_scene_object(
            "Town Hall",
            ObjectKind::Building,
            "minecraft:overworld",
            BlockBox::new(BlockPos::new(4, 64, 4), BlockPos::new(6, 64, 4)).unwrap(),
            Some(district.id.clone()),
            BTreeMap::from([("style".to_owned(), "nordic".to_owned())]),
            None,
        )
        .unwrap();
    assert_eq!(service.world_info().revision, revision);
    assert_eq!(building.id, "town-hall");
    assert_eq!(building.parent_id.as_deref(), Some("old-town"));

    match service.query(Query::SceneObjects).unwrap() {
        QueryResult::SceneObjects(objects) => {
            assert_eq!(objects.len(), 2);
            assert_eq!(objects[0].kind, ObjectKind::District);
        }
        other => panic!("unexpected query result: {other:?}"),
    }
    match service
        .query(Query::InspectSceneObject {
            object_id: "town-hall".to_owned(),
        })
        .unwrap()
    {
        QueryResult::SceneObject(inspection) => {
            assert_eq!(inspection.object.name, "Town Hall");
            assert_eq!(inspection.volume, 3);
            assert_eq!(inspection.non_air_blocks, 2);
            assert_eq!(
                inspection
                    .palette
                    .iter()
                    .map(|entry| entry.count)
                    .sum::<u64>(),
                3
            );
            assert!(inspection
                .palette
                .iter()
                .any(|entry| entry.state == "minecraft:stone" && entry.count == 1));
            assert!(inspection.children.is_empty());
        }
        other => panic!("unexpected query result: {other:?}"),
    }
    match service
        .query(Query::InspectSceneObject {
            object_id: "old-town".to_owned(),
        })
        .unwrap()
    {
        QueryResult::SceneObject(inspection) => {
            assert_eq!(inspection.children.len(), 1);
            assert_eq!(inspection.children[0].id, "town-hall");
        }
        other => panic!("unexpected query result: {other:?}"),
    }

    assert_eq!(
        service
            .query(Query::InspectSceneObject {
                object_id: "missing".to_owned(),
            })
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        service
            .add_scene_object(
                "Bad",
                ObjectKind::Other,
                "minecraft:moon",
                BlockBox::single(BlockPos::new(0, 64, 0)),
                None,
                BTreeMap::new(),
                None,
            )
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    assert_eq!(
        service.remove_scene_object("old-town").unwrap_err().code,
        ErrorCode::Conflict
    );
    assert_eq!(service.world_info().revision, revision);
}

#[test]
fn scene_metadata_is_validated_and_replaced_without_history() {
    use scene_model::{ObjectKind, Scene};
    use std::collections::BTreeMap;

    let mut service = WorldService::new();
    let mut scene = Scene::new();
    scene
        .push(
            "Nether Hub",
            ObjectKind::Landmark,
            "minecraft:the_nether",
            BlockBox::single(BlockPos::new(0, 64, 0)),
            None,
            BTreeMap::new(),
            None,
        )
        .unwrap();
    service.set_scene(scene.clone()).unwrap();
    assert_eq!(service.scene(), &scene);
    assert_eq!(service.world_info().revision, 0);
    assert!(service.history().is_empty());

    let mut invalid = scene.clone();
    invalid.schema_version = 99;
    assert_eq!(
        service.set_scene(invalid).unwrap_err().code,
        ErrorCode::UnsupportedVersion
    );

    let mut missing_dimension = scene.clone();
    missing_dimension.objects[0].dimension = "minecraft:moon".to_owned();
    assert_eq!(
        service.set_scene(missing_dimension).unwrap_err().code,
        ErrorCode::NotFound
    );
    assert_eq!(service.scene(), &scene);
}

fn parametric_spec(name: &str, origin: BlockPos) -> scene_model::ParametricSpec {
    scene_model::ParametricSpec {
        name: name.to_owned(),
        generator: scene_model::GeneratorKind::House,
        generator_version: scene_model::HOUSE_GENERATOR_VERSION,
        dimension: "minecraft:overworld".to_owned(),
        origin,
        rotation: scene_model::Rotation::None,
        params: scene_model::HouseParams {
            width: 5,
            depth: 4,
            height: 3,
            wall: "minecraft:stone_bricks".to_owned(),
            floor: "minecraft:oak_planks".to_owned(),
            roof: "minecraft:dark_oak_planks".to_owned(),
            window: Some("minecraft:glass".to_owned()),
        },
        layer_id: None,
    }
}

#[test]
fn parametric_create_applies_deterministic_overlay_and_records_baseline() {
    let mut service = WorldService::new();
    service
        .execute(set_block(
            BlockPos::new(10, 65, -20),
            "minecraft:gold_block",
        ))
        .unwrap();
    let revision = service.world_info().revision;

    let result = service
        .create_parametric_object(
            "test-project",
            actor(),
            parametric_spec("Stone House", BlockPos::new(10, 64, -20)),
        )
        .unwrap();
    let object = result.object;
    assert_eq!(object.id, "stone-house");
    assert!(object.generated_blocks > 50);
    assert_eq!(object.bounds.min, BlockPos::new(10, 64, -20));
    assert_eq!(object.bounds.max, BlockPos::new(14, 68, -17));
    let operation = result.operation.expect("operation");
    assert_eq!(operation.world_revision, revision + 1);
    assert!(operation.changed_blocks > 0);
    assert_eq!(
        service
            .world()
            .get_block("minecraft:overworld", BlockPos::new(10, 64, -20))
            .unwrap()
            .to_string(),
        "minecraft:oak_planks"
    );
    assert_eq!(
        service
            .world()
            .get_block("minecraft:overworld", BlockPos::new(10, 65, -20))
            .unwrap()
            .to_string(),
        "minecraft:stone_bricks"
    );
    assert_eq!(
        service
            .world()
            .get_block("minecraft:overworld", BlockPos::new(12, 65, -20))
            .unwrap()
            .to_string(),
        "minecraft:air"
    );
    let inspection = service.inspect_parametric_object("stone-house").unwrap();
    assert_eq!(inspection.conflict_count, 0);
    assert_eq!(inspection.generated_blocks, object.generated_blocks);
    assert_eq!(inspection.object.rotation, scene_model::Rotation::None);
    assert_eq!(service.parametric_summaries().unwrap().len(), 1);
    assert_eq!(service.world_info().revision, revision + 1);
    assert_eq!(
        service
            .audit_log()
            .last()
            .unwrap()
            .linked_object_id
            .as_deref(),
        Some("stone-house")
    );
}

#[test]
fn parametric_manual_edits_are_reported_and_override_is_explicit() {
    let mut service = WorldService::new();
    service
        .execute(set_block(
            BlockPos::new(10, 65, -20),
            "minecraft:gold_block",
        ))
        .unwrap();
    service
        .create_parametric_object(
            "test-project",
            actor(),
            parametric_spec("Conflict House", BlockPos::new(10, 64, -20)),
        )
        .unwrap();
    service
        .execute(set_block(BlockPos::new(10, 65, -20), "minecraft:glass"))
        .unwrap();
    let revision = service.world_info().revision;
    let inspection = service.inspect_parametric_object("conflict-house").unwrap();
    assert!(inspection.conflict_count >= 1);
    assert_eq!(inspection.conflicts[0].found.to_string(), "minecraft:glass");

    let mut taller = parametric_spec("Conflict House", BlockPos::new(10, 64, -20));
    taller.params.height = 4;
    let error = service
        .update_parametric_object(
            "test-project",
            actor(),
            "conflict-house",
            taller.clone(),
            scene_model::ParametricConflictPolicy::Fail,
        )
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(service.world_info().revision, revision);
    assert_eq!(
        service
            .world()
            .get_block("minecraft:overworld", BlockPos::new(10, 65, -20))
            .unwrap()
            .to_string(),
        "minecraft:glass"
    );

    let result = service
        .update_parametric_object(
            "test-project",
            actor(),
            "conflict-house",
            taller,
            scene_model::ParametricConflictPolicy::Override,
        )
        .unwrap();
    assert!(!result.conflicts.is_empty());
    assert_eq!(
        service
            .world()
            .get_block("minecraft:overworld", BlockPos::new(10, 65, -20))
            .unwrap()
            .to_string(),
        "minecraft:stone_bricks"
    );
    assert!(
        service
            .inspect_parametric_object("conflict-house")
            .unwrap()
            .conflict_count
            == 0
    );
    let revision_after = service.world_info().revision;
    assert!(revision_after > revision);

    let rolled_back = service
        .rollback_parametric_object("test-project", actor(), "conflict-house", false)
        .unwrap();
    assert!(rolled_back.operation.is_some());
    assert_eq!(
        service
            .world()
            .get_block("minecraft:overworld", BlockPos::new(10, 65, -20))
            .unwrap()
            .to_string(),
        "minecraft:gold_block"
    );
    assert!(service.parametric_summaries().unwrap().is_empty());
    assert_eq!(
        service
            .inspect_parametric_object("conflict-house")
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
}

#[test]
fn parametric_transform_restores_old_footprint_and_rollback_restores_baseline() {
    let mut service = WorldService::new();
    service
        .create_parametric_object(
            "test-project",
            actor(),
            parametric_spec("Moving House", BlockPos::new(10, 64, -20)),
        )
        .unwrap();
    let revision = service.world_info().revision;
    let mut moved = parametric_spec("Moving House", BlockPos::new(15, 64, -20));
    moved.rotation = scene_model::Rotation::Cw90;
    let result = service
        .update_parametric_object(
            "test-project",
            actor(),
            "moving-house",
            moved,
            scene_model::ParametricConflictPolicy::Fail,
        )
        .unwrap();
    assert!(result.operation.is_some());
    assert_eq!(
        service
            .world()
            .get_block("minecraft:overworld", BlockPos::new(10, 65, -20))
            .unwrap()
            .to_string(),
        "minecraft:air",
        "the old footprint is restored to its baseline"
    );
    assert_eq!(
        service
            .world()
            .get_block("minecraft:overworld", BlockPos::new(15, 64, -20))
            .unwrap()
            .to_string(),
        "minecraft:oak_planks"
    );
    assert_eq!(result.object.rotation, scene_model::Rotation::Cw90);
    assert_eq!(result.object.bounds.max, BlockPos::new(18, 68, -16));

    service
        .rollback_parametric_object("test-project", actor(), "moving-house", false)
        .unwrap();
    assert_eq!(
        service
            .world()
            .get_block("minecraft:overworld", BlockPos::new(15, 64, -20))
            .unwrap()
            .to_string(),
        "minecraft:air"
    );
    assert!(service.parametric_summaries().unwrap().is_empty());
    assert!(service.world_info().revision > revision);
}

#[test]
fn parametric_bake_and_failed_rollback_leave_manual_blocks_intact() {
    let mut service = WorldService::new();
    service
        .create_parametric_object(
            "test-project",
            actor(),
            parametric_spec("Baked House", BlockPos::new(10, 64, -20)),
        )
        .unwrap();
    let revision = service.world_info().revision;
    let baked = service.bake_parametric_object("baked-house").unwrap();
    assert!(baked.operation.is_none());
    assert_eq!(service.world_info().revision, revision);
    assert!(service.parametric_summaries().unwrap().is_empty());
    assert_eq!(
        service
            .world()
            .get_block("minecraft:overworld", BlockPos::new(10, 64, -20))
            .unwrap()
            .to_string(),
        "minecraft:oak_planks"
    );

    service
        .create_parametric_object(
            "test-project",
            actor(),
            parametric_spec("Protected House", BlockPos::new(20, 64, -20)),
        )
        .unwrap();
    service
        .execute(set_block(
            BlockPos::new(20, 65, -20),
            "minecraft:diamond_block",
        ))
        .unwrap();
    let revision = service.world_info().revision;
    let error = service
        .rollback_parametric_object("test-project", actor(), "protected-house", false)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::Conflict);
    assert_eq!(service.world_info().revision, revision);
    service
        .rollback_parametric_object("test-project", actor(), "protected-house", true)
        .unwrap();
    assert_eq!(
        service
            .world()
            .get_block("minecraft:overworld", BlockPos::new(20, 65, -20))
            .unwrap()
            .to_string(),
        "minecraft:air"
    );
    assert!(service.parametric_summaries().unwrap().is_empty());
}

#[test]
fn parametric_rejects_invalid_recipes_and_unknown_dimensions() {
    let mut service = WorldService::new();
    let mut bad_material = parametric_spec("Bad Material", BlockPos::new(0, 64, 0));
    bad_material.params.wall = "not-a-block".to_owned();
    assert_eq!(
        service
            .create_parametric_object("test-project", actor(), bad_material)
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    let mut bad_dimension = parametric_spec("Bad Dimension", BlockPos::new(0, 64, 0));
    bad_dimension.dimension = "minecraft:moon".to_owned();
    assert_eq!(
        service
            .create_parametric_object("test-project", actor(), bad_dimension)
            .unwrap_err()
            .code,
        ErrorCode::NotFound
    );
    let mut bad_size = parametric_spec("Tiny", BlockPos::new(0, 64, 0));
    bad_size.params.width = 2;
    assert_eq!(
        service
            .create_parametric_object("test-project", actor(), bad_size)
            .unwrap_err()
            .code,
        ErrorCode::InvalidArgument
    );
    assert!(service.parametric_summaries().unwrap().is_empty());
}

#[test]
fn observe_renders_deterministic_maps_and_an_exact_revision_diff() {
    let mut service = WorldService::new();
    service
        .execute(set_block(BlockPos::new(0, 64, 0), "minecraft:stone"))
        .unwrap();
    service
        .execute(set_block(BlockPos::new(0, 65, 0), "minecraft:grass_block"))
        .unwrap();
    let bounds = BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(0, 65, 0)).unwrap();

    let surface = service
        .observe(&ObservationRequest {
            dimension: "minecraft:overworld".to_owned(),
            bounds,
            mode: ObservationMode::MaterialMap,
            slice: None,
            from_revision: None,
        })
        .unwrap();
    assert_eq!(surface.world_revision, 2);
    assert_eq!((surface.width, surface.height), (1, 1));
    assert_eq!(surface.blocks_per_pixel, 1);
    assert!(surface.image.starts_with("data:image/png;base64,"));
    assert!(surface.image_bytes > 0);
    assert_eq!(surface.sampled_units, 1);
    assert_eq!(surface.non_air_units, 1);
    assert!(surface
        .legend
        .iter()
        .any(|entry| entry.key == "minecraft:grass_block"));
    assert!(!surface.partial);
    assert!(surface
        .notes
        .iter()
        .any(|note| note.contains("deterministic")));

    let repeat = service
        .observe(&ObservationRequest {
            dimension: "minecraft:overworld".to_owned(),
            bounds,
            mode: ObservationMode::MaterialMap,
            slice: None,
            from_revision: None,
        })
        .unwrap();
    assert_eq!(surface, repeat);

    let cross = service
        .observe(&ObservationRequest {
            dimension: "minecraft:overworld".to_owned(),
            bounds,
            mode: ObservationMode::CrossSection,
            slice: Some(Slice {
                axis: SliceAxis::Y,
                coordinate: 64,
            }),
            from_revision: None,
        })
        .unwrap();
    assert_eq!((cross.width, cross.height), (1, 1));
    assert_eq!(cross.non_air_units, 1);
    assert!(cross
        .legend
        .iter()
        .any(|entry| entry.key == "state:minecraft:stone"));

    let diff = service
        .observe(&ObservationRequest {
            dimension: "minecraft:overworld".to_owned(),
            bounds,
            mode: ObservationMode::RevisionDiff,
            slice: None,
            from_revision: Some(1),
        })
        .unwrap();
    assert_eq!(
        diff.counts,
        DeltaCounts {
            added: 1,
            removed: 0,
            changed: 0
        }
    );
    assert_eq!(diff.from_revision, Some(1));
    assert_eq!(diff.world_revision, 2);
}

#[test]
fn observation_rejects_invalid_mode_combinations_and_missing_history() {
    let mut service = WorldService::new().with_max_history(1);
    for y in 64..=66 {
        service
            .execute(set_block(BlockPos::new(0, y, 0), "minecraft:stone"))
            .unwrap();
    }
    let bounds = BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(0, 66, 0)).unwrap();

    let missing_history = service
        .observe(&ObservationRequest {
            dimension: "minecraft:overworld".to_owned(),
            bounds,
            mode: ObservationMode::RevisionDiff,
            slice: None,
            from_revision: Some(0),
        })
        .unwrap_err();
    assert_eq!(missing_history.code, ErrorCode::NoHistory);

    let wrong_slice = service
        .observe(&ObservationRequest {
            dimension: "minecraft:overworld".to_owned(),
            bounds,
            mode: ObservationMode::MaterialMap,
            slice: Some(Slice {
                axis: SliceAxis::Y,
                coordinate: 64,
            }),
            from_revision: None,
        })
        .unwrap_err();
    assert_eq!(wrong_slice.code, ErrorCode::InvalidArgument);

    let missing_from = service
        .observe(&ObservationRequest {
            dimension: "minecraft:overworld".to_owned(),
            bounds,
            mode: ObservationMode::RevisionDiff,
            slice: None,
            from_revision: None,
        })
        .unwrap_err();
    assert_eq!(missing_from.code, ErrorCode::InvalidArgument);

    let newer_from = service
        .observe(&ObservationRequest {
            dimension: "minecraft:overworld".to_owned(),
            bounds,
            mode: ObservationMode::RevisionDiff,
            slice: None,
            from_revision: Some(99),
        })
        .unwrap_err();
    assert_eq!(newer_from.code, ErrorCode::InvalidArgument);
}

fn validation_query(bounds: BlockBox) -> ValidationQuery {
    ValidationQuery {
        dimension: "minecraft:overworld".to_owned(),
        bounds,
        rules: Vec::new(),
        allowed_bounds: None,
        path: None,
        max_surface_materials: None,
        floating_fix: FloatingFix::Support,
    }
}

#[test]
fn validation_reports_findings_at_the_authoritative_revision() {
    let mut service = WorldService::new();
    service
        .execute(set_block(BlockPos::new(0, 64, 0), "minecraft:stone"))
        .unwrap();
    let bounds = BlockBox::new(BlockPos::new(0, 63, 0), BlockPos::new(0, 64, 0)).unwrap();
    let mut query = validation_query(bounds);
    query.rules = vec![ValidationRuleId::FloatingBlocks];

    let report = service.validate_region(&query).unwrap();
    assert_eq!(report.schema_version, 1);
    assert_eq!(report.world_revision, 1);
    assert_eq!(report.rules, vec![ValidationRuleId::FloatingBlocks]);
    assert_eq!(report.findings.len(), 1);
    assert_eq!(report.counts.warning, 1);
    assert_eq!(report.counts.error, 0);
    assert_eq!(report.findings[0].rule_id, ValidationRuleId::FloatingBlocks);
    assert_eq!(report.findings[0].position_count, 1);
    let fix = report.findings[0]
        .suggested_fix
        .as_ref()
        .expect("support suggestion");
    assert!(matches!(&fix.operation, Operation::Batch { .. }));
    assert!(report.notes.iter().any(|note| note.contains("read-only")));
    assert!(!report.truncated);
}

#[test]
fn validation_injects_workspace_protection_and_rejects_missing_inputs() {
    let mut service = WorldService::new();
    let mut policy = service.workspace_policy();
    policy
        .push_protected_region(
            "Spawn",
            "minecraft:overworld",
            BlockBox::new(BlockPos::new(0, 60, 0), BlockPos::new(8, 80, 8)).unwrap(),
        )
        .unwrap();
    service.set_workspace_policy(policy).unwrap();
    let bounds = BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(4, 70, 4)).unwrap();

    let mut query = validation_query(bounds);
    query.rules = vec![ValidationRuleId::ProtectedRegion];
    let report = service.validate_region(&query).unwrap();
    assert_eq!(report.findings.len(), 1);
    assert_eq!(
        report.findings[0].rule_id,
        ValidationRuleId::ProtectedRegion
    );
    assert_eq!(report.findings[0].severity, Severity::Error);
    assert!(report.findings[0].message.contains("Spawn"));

    let mut missing_envelope = validation_query(bounds);
    missing_envelope.rules = vec![ValidationRuleId::OutOfBounds];
    let error = service.validate_region(&missing_envelope).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);

    let mut missing_path = validation_query(bounds);
    missing_path.rules = vec![ValidationRuleId::PathContinuity];
    let error = service.validate_region(&missing_path).unwrap_err();
    assert_eq!(error.code, ErrorCode::InvalidArgument);

    let mut bad_limit = validation_query(bounds);
    bad_limit.rules = vec![ValidationRuleId::Palette];
    bad_limit.max_surface_materials = Some(0);
    let error = service.validate_region(&bad_limit).unwrap_err();
    assert_eq!(error.code, ErrorCode::LimitExceeded);
}
