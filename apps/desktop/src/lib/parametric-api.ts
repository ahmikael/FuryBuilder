import { invokeCommand } from "@/lib/ipc";
import type {
  ParametricApplyResult,
  ParametricConflictPolicy,
  ParametricInspection,
  ParametricObjectSummary,
  ParametricSpec,
} from "@/project/parametric-types";

export function fetchParametricObjects(): Promise<ParametricObjectSummary[]> {
  return invokeCommand<ParametricObjectSummary[]>("parametric_list");
}

export function inspectParametricObject(objectId: string): Promise<ParametricInspection> {
  return invokeCommand<ParametricInspection>("parametric_inspect", { objectId });
}

export function createParametricObject(spec: ParametricSpec): Promise<ParametricApplyResult> {
  return invokeCommand<ParametricApplyResult>("parametric_create", { spec });
}

export function updateParametricObject(
  objectId: string,
  spec: ParametricSpec,
  conflictPolicy: ParametricConflictPolicy,
): Promise<ParametricApplyResult> {
  return invokeCommand<ParametricApplyResult>("parametric_update", {
    objectId,
    spec,
    conflictPolicy,
  });
}

export function bakeParametricObject(objectId: string): Promise<ParametricApplyResult> {
  return invokeCommand<ParametricApplyResult>("parametric_bake", { objectId });
}

export function rollbackParametricObject(
  objectId: string,
  force: boolean,
): Promise<ParametricApplyResult> {
  return invokeCommand<ParametricApplyResult>("parametric_rollback", { objectId, force });
}
