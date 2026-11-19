import { invokeCommand } from "@/lib/ipc";
import type { Observation, ObservationRequest } from "@/project/observation-types";

/** Read-only deterministic map from the Rust Core at the current revision. */
export function observeRegion(request: ObservationRequest): Promise<Observation> {
  return invokeCommand<Observation>("observe", { request });
}
