use super::*;
use crate::dto::{Actor, ActorSource, AuditAction, OperationRequest};
use world_domain::{BlockBox, BlockPos, BlockState};
use world_operations::{BlockChange, Operation};

fn state(name: &str) -> BlockState {
    name.parse().expect("valid state")
}

#[test]
fn audit_records_actor_source_object_and_timestamps() {
    let mut service = WorldService::new();
    let request = OperationRequest {
        project_id: "project-1".to_owned(),
        operation: Operation::FillCuboid {
            dimension: "minecraft:overworld".to_owned(),
            bounds: BlockBox::new(BlockPos::new(0, 64, 0), BlockPos::new(2, 64, 2)).unwrap(),
            state: state("minecraft:stone_bricks"),
        },
        expected_revision: None,
        idempotency_key: None,
        actor: Actor {
            id: "agent-1".to_owned(),
            source: ActorSource::Mcp,
        },
        linked_object_id: Some("object-42".to_owned()),
        layer_id: None,
    };
    let result = service.execute(request).unwrap();
    let audit = &service.audit_log()[0];

    assert_eq!(audit.operation_id, result.operation_id);
    assert_eq!(audit.project_id, "project-1");
    assert_eq!(audit.actor_id, "agent-1");
    assert_eq!(audit.source, ActorSource::Mcp);
    assert_eq!(audit.action, AuditAction::Apply);
    assert_eq!(audit.linked_object_id.as_deref(), Some("object-42"));
    assert_eq!(audit.changed_blocks, 9);
    assert!(audit.timestamp_ms > 0);
    assert_eq!(audit.parameter_summary["type"], "fill_cuboid");
    assert_eq!(audit.parameter_summary["state"], "minecraft:stone_bricks");
}

#[test]
fn batch_parameter_summary_is_bounded() {
    let mut service = WorldService::new();
    let changes: Vec<BlockChange> = (0..1000)
        .map(|index| BlockChange {
            pos: BlockPos::new(index % 100, 64, index / 100),
            state: state("minecraft:stone"),
        })
        .collect();
    let request = OperationRequest {
        project_id: "project-1".to_owned(),
        operation: Operation::Batch {
            dimension: "minecraft:overworld".to_owned(),
            changes,
        },
        expected_revision: None,
        idempotency_key: None,
        actor: Actor::ui("ui"),
        linked_object_id: None,
        layer_id: None,
    };
    service.execute(request).unwrap();
    let audit = &service.audit_log()[0];
    assert_eq!(audit.changed_blocks, 1000);
    assert_eq!(audit.parameter_summary["changes"], 1000);
    let serialized = serde_json::to_string(&audit.parameter_summary).unwrap();
    assert!(
        serialized.len() < 200,
        "parameter summary must not dump every block: {serialized}"
    );
}
