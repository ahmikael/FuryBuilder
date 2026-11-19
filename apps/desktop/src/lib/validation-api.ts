import { invokeCommand } from "@/lib/ipc";
import type { ValidationQuery, ValidationReport } from "@/project/validation-types";

/** Read-only validation report from the Rust Core at the current revision. */
export function validateRegion(request: ValidationQuery): Promise<ValidationReport> {
  return invokeCommand<ValidationReport>("validate_region", { request });
}
