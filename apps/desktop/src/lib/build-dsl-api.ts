import { invokeCommand } from "@/lib/ipc";
import type { BuildDslPreview, BuildRecipe } from "@/project/build-dsl-types";

/** Compiles and previews through the Rust Core. Nothing is written by this call. */
export function previewBuildDsl(recipe: BuildRecipe): Promise<BuildDslPreview> {
  return invokeCommand<BuildDslPreview>("build_dsl_preview", { recipe });
}
