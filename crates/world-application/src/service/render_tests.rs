use super::*;
use crate::dto::{Actor, OperationRequest, QueryResult};
use world_domain::{BlockPos, BlockState, ChunkPos};
use world_operations::Operation;

#[test]
fn demo_world_contains_terrain_house_water_and_trees() {
    let mut service = WorldService::new();
    let info = service.load_demo_world().expect("demo world loads");
    assert_eq!(info.revision, 1);
    assert!(info.block_count > 100_000);
    assert!(info.chunk_count >= 9);

    let read = |x, y, z| {
        service
            .world()
            .get_block("minecraft:overworld", BlockPos::new(x, y, z))
            .expect("read")
            .to_string()
    };
    assert_eq!(read(0, 64, 0), "minecraft:grass_block");
    assert_eq!(read(2, 67, -8), "minecraft:oak_planks");
    assert_eq!(read(12, 64, 6), "minecraft:water");
    assert_eq!(read(-6, 65, -6), "minecraft:oak_log");
    assert_eq!(read(0, 70, 0), "minecraft:air");

    let history = service.history();
    assert!(history.is_empty(), "demo seeding is not user history");
}

#[test]
fn render_snapshot_returns_requested_chunks_and_revision() {
    let mut service = WorldService::new();
    service.load_demo_world().unwrap();
    let snapshot = service
        .render_snapshot("minecraft:overworld", ChunkPos::new(0, 0), 1)
        .unwrap();
    assert_eq!(snapshot.dimension, "minecraft:overworld");
    assert_eq!(snapshot.revision, 1);
    assert_eq!(snapshot.radius, 1);
    assert_eq!(snapshot.chunks.len(), 9);
    assert!(snapshot.chunks.iter().all(|chunk| chunk.loaded));
    assert!(snapshot
        .chunks
        .iter()
        .all(|chunk| !chunk.sections.is_empty()));
    let section = &snapshot.chunks[4].sections[0];
    assert!(!section.palette.is_empty());
    assert_eq!(
        section.bits_per_index,
        if section.palette.len() == 1 {
            0
        } else {
            section.bits_per_index
        }
    );
}

#[test]
fn render_snapshot_marks_unloaded_chunks() {
    let service = WorldService::new();
    let snapshot = service
        .render_snapshot("minecraft:overworld", ChunkPos::new(100, 100), 0)
        .unwrap();
    assert_eq!(snapshot.chunks.len(), 1);
    assert!(!snapshot.chunks[0].loaded);
    assert!(snapshot.chunks[0].sections.is_empty());
}

#[test]
fn render_radius_is_bounded() {
    let service = WorldService::new();
    let error = service
        .render_snapshot("minecraft:overworld", ChunkPos::new(0, 0), 32)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::LimitExceeded);
}

#[test]
fn render_snapshot_carries_palette_payload_for_mixed_sections() {
    let mut service = WorldService::new();
    service.load_demo_world().unwrap();
    let snapshot = service
        .render_snapshot("minecraft:overworld", ChunkPos::new(0, 0), 0)
        .unwrap();
    let mixed = snapshot.chunks[0]
        .sections
        .iter()
        .find(|section| section.palette.len() > 1)
        .expect("demo world has a mixed section");
    assert!(mixed.bits_per_index >= 1);
    assert!(!mixed.data.is_empty());
    assert!(mixed.non_air > 0);
    assert!(
        mixed.palette[0].is_air() || mixed.palette[0].name().starts_with("minecraft:"),
        "palette states stay namespaced"
    );
}

#[test]
fn world_info_query_matches_service_info() {
    let service = WorldService::new();
    assert_eq!(
        service.query(Query::WorldInfo).unwrap(),
        QueryResult::WorldInfo(service.world_info())
    );
}

#[test]
fn render_chunks_returns_only_requested_chunks() {
    let mut service = WorldService::new();
    service.load_demo_world().unwrap();
    let batch = service
        .render_chunks(
            "minecraft:overworld",
            &[
                ChunkPos::new(0, 0),
                ChunkPos::new(100, 100),
                ChunkPos::new(0, 0),
            ],
        )
        .unwrap();
    assert_eq!(batch.revision, 1);
    assert_eq!(
        batch.chunks.len(),
        2,
        "duplicate chunk requests are deduplicated"
    );
    assert!(batch.chunks[0].loaded);
    assert!(!batch.chunks[1].loaded);
}

#[test]
fn render_chunks_rejects_unbounded_requests() {
    let service = WorldService::new();
    let chunks: Vec<ChunkPos> = (0..=MAX_RENDER_CHUNKS)
        .map(|index| ChunkPos::new(index as i32, 0))
        .collect();
    let error = service
        .render_chunks("minecraft:overworld", &chunks)
        .unwrap_err();
    assert_eq!(error.code, ErrorCode::LimitExceeded);
}

#[test]
fn render_chunks_reflects_dirty_edits_and_revision() {
    let mut service = WorldService::new();
    service.load_demo_world().unwrap();
    let state: BlockState = "minecraft:gold_block".parse().unwrap();
    let result = service
        .execute(OperationRequest {
            project_id: "project-1".to_owned(),
            operation: Operation::SetBlock {
                dimension: "minecraft:overworld".to_owned(),
                pos: BlockPos::new(0, 64, 0),
                state,
            },
            expected_revision: None,
            idempotency_key: None,
            actor: Actor::ui("ui"),
            linked_object_id: None,
            layer_id: None,
        })
        .unwrap();
    assert_eq!(result.changed_blocks, 1);

    let batch = service
        .render_chunks("minecraft:overworld", &[ChunkPos::new(0, 0)])
        .unwrap();
    assert_eq!(batch.revision, result.world_revision);
    let has_gold = batch.chunks[0].sections.iter().any(|section| {
        section
            .palette
            .iter()
            .any(|state| state.name() == "minecraft:gold_block")
    });
    assert!(
        has_gold,
        "dirty chunk snapshot must contain the new block state"
    );
}
