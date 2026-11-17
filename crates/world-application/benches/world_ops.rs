//! Manual benchmark harness for the world command/query hot paths.
//!
//! Run with: `cargo bench -p furybuilder-world-application --bench world_ops`.
//! Results are intentionally printed as plain lines so they can be recorded in
//! `docs/testing/perf-runs.md` without a specific benchmark framework.

use std::hint::black_box;
use std::time::Instant;

use world_application::{Actor, OperationRequest, WorldService};
use world_domain::{BlockBox, BlockPos, BlockState};
use world_operations::Operation;

fn state(name: &str) -> BlockState {
    name.parse().expect("valid state")
}

fn request(operation: Operation) -> OperationRequest {
    OperationRequest {
        project_id: "bench-project".to_owned(),
        operation,
        expected_revision: None,
        idempotency_key: None,
        actor: Actor::ui("bench"),
        linked_object_id: None,
    }
}

fn main() {
    let mut service = WorldService::new();
    let bounds =
        BlockBox::new(BlockPos::new(0, 0, 0), BlockPos::new(99, 31, 99)).expect("valid bounds");
    let fill = Operation::FillCuboid {
        dimension: "minecraft:overworld".to_owned(),
        bounds,
        state: state("minecraft:stone"),
    };

    let started = Instant::now();
    let applied = service.execute(request(fill)).expect("fill succeeds");
    let fill_time = started.elapsed();
    let blocks = applied.changed_blocks as f64;
    println!(
        "fill: {} blocks in {:.2} ms ({:.0} blocks/s), revision {}",
        applied.changed_blocks,
        fill_time.as_secs_f64() * 1000.0,
        blocks / fill_time.as_secs_f64(),
        applied.world_revision
    );

    let reads = 100_000u64;
    let started = Instant::now();
    let mut checksum = 0usize;
    for index in 0..reads {
        let pos = BlockPos::new(
            (index % 100) as i32,
            ((index / 100) % 32) as i32,
            ((index / 3_200) % 100) as i32,
        );
        let block = service
            .world()
            .get_block("minecraft:overworld", pos)
            .expect("read succeeds");
        checksum += block.name().len();
    }
    let read_time = started.elapsed();
    println!(
        "reads: {} blocks in {:.2} ms ({:.0} reads/s), checksum {}",
        reads,
        read_time.as_secs_f64() * 1000.0,
        reads as f64 / read_time.as_secs_f64(),
        black_box(checksum)
    );

    let started = Instant::now();
    let undo = service
        .undo("bench-project", Actor::ui("bench"), None)
        .expect("undo succeeds");
    let undo_time = started.elapsed();
    println!(
        "undo: {} blocks in {:.2} ms, revision {}",
        undo.changed_blocks,
        undo_time.as_secs_f64() * 1000.0,
        undo.world_revision
    );

    let started = Instant::now();
    let redo = service
        .redo("bench-project", Actor::ui("bench"))
        .expect("redo succeeds");
    let redo_time = started.elapsed();
    println!(
        "redo: {} blocks in {:.2} ms, revision {}",
        redo.changed_blocks,
        redo_time.as_secs_f64() * 1000.0,
        redo.world_revision
    );
}
