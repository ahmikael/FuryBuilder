use super::*;
use crate::dto::{Actor, OperationRequest, Query, QueryResult};
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
