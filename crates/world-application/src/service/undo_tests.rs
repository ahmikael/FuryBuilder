use super::*;
use crate::dto::{Actor, OperationRequest, Query, QueryResult};
use world_domain::{BlockBox, BlockPos, BlockState};
use world_operations::Operation;

fn state(name: &str) -> BlockState {
    name.parse().expect("valid state")
}

fn actor() -> Actor {
    Actor::ui("undo-test")
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

fn fill(name: &str, min: BlockPos, max: BlockPos) -> OperationRequest {
    request(Operation::FillCuboid {
        dimension: "minecraft:overworld".to_owned(),
        bounds: BlockBox::new(min, max).unwrap(),
        state: state(name),
    })
}

#[test]
fn redo_stack_is_cleared_by_a_new_operation() {
    let mut service = WorldService::new();
    service
        .execute(fill(
            "minecraft:stone",
            BlockPos::new(0, 64, 0),
            BlockPos::new(2, 64, 2),
        ))
        .unwrap();
    service.undo("project-1", actor(), None).unwrap();
    assert!(service.can_redo());

    service
        .execute(fill(
            "minecraft:dirt",
            BlockPos::new(0, 65, 0),
            BlockPos::new(0, 65, 0),
        ))
        .unwrap();
    assert!(!service.can_redo());
    assert!(matches!(
        service.redo("project-1", actor()),
        Err(AppError {
            code: ErrorCode::NoHistory,
            ..
        })
    ));
}

#[test]
fn undo_accepts_the_latest_operation_id() {
    let mut service = WorldService::new();
    let applied = service
        .execute(fill(
            "minecraft:stone",
            BlockPos::new(-1, 64, -1),
            BlockPos::new(1, 64, 1),
        ))
        .unwrap();
    let undo = service
        .undo("project-1", actor(), Some(&applied.operation_id))
        .expect("latest operation can be undone by id");
    assert_eq!(undo.changed_blocks, 9);
    assert_eq!(service.world_info().block_count, 0);
}

#[test]
fn undo_is_scoped_to_the_operation_dimension() {
    let mut service = WorldService::new();
    service
        .execute(request(Operation::SetBlock {
            dimension: "minecraft:overworld".to_owned(),
            pos: BlockPos::new(0, 64, 0),
            state: state("minecraft:stone"),
        }))
        .unwrap();
    service
        .execute(request(Operation::SetBlock {
            dimension: "minecraft:the_nether".to_owned(),
            pos: BlockPos::new(0, 64, 0),
            state: state("minecraft:netherrack"),
        }))
        .unwrap();
    service.undo("project-1", actor(), None).unwrap();

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
    assert!(service
        .query(Query::GetBlock {
            dimension: "minecraft:the_nether".to_owned(),
            pos: BlockPos::new(0, 64, 0),
        })
        .is_ok());
}

#[test]
fn undo_and_redo_bump_revision_and_are_audited() {
    let mut service = WorldService::new();
    service
        .execute(fill(
            "minecraft:glass",
            BlockPos::new(0, 70, 0),
            BlockPos::new(1, 70, 1),
        ))
        .unwrap();
    let undo = service.undo("project-1", actor(), None).unwrap();
    assert_eq!(undo.world_revision, 2);
    let redo = service.redo("project-1", actor()).unwrap();
    assert_eq!(redo.world_revision, 3);
    let actions: Vec<_> = service
        .audit_log()
        .iter()
        .map(|entry| entry.action.clone())
        .collect();
    assert_eq!(
        actions,
        vec![AuditAction::Apply, AuditAction::Undo, AuditAction::Redo]
    );
}

#[test]
fn undo_with_empty_history_returns_no_history() {
    let mut service = WorldService::new();
    let error = service.undo("project-1", actor(), None).unwrap_err();
    assert_eq!(error.code, ErrorCode::NoHistory);
    let error = service.redo("project-1", actor()).unwrap_err();
    assert_eq!(error.code, ErrorCode::NoHistory);
}
