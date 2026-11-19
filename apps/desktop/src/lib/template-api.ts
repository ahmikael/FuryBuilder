import { invokeCommand } from "@/lib/ipc";
import type { Bounds3 } from "@/renderer/types";
import type { StructureInsertOutcome } from "@/project/structure-types";
import type {
  TemplateEntry,
  TemplateInsertArgs,
  TemplateInsertPreview,
  TemplatePreviewArgs,
} from "@/project/template-types";

export function listTemplates(): Promise<TemplateEntry[]> {
  return invokeCommand<TemplateEntry[]>("template_list");
}

export function saveSelectionTemplate(
  name: string,
  dimension: string,
  bounds: Bounds3,
): Promise<TemplateEntry> {
  return invokeCommand<TemplateEntry>("template_save_selection", {
    name,
    dimension,
    bounds,
  });
}

/** Imports a user-picked Java .nbt file as a verified library template. */
export function importTemplate(name: string, payload: string): Promise<TemplateEntry> {
  return invokeCommand<TemplateEntry>("template_import", { name, payload });
}

export function deleteTemplate(templateId: string): Promise<TemplateEntry> {
  return invokeCommand<TemplateEntry>("template_delete", { templateId });
}

export function previewTemplate(args: TemplatePreviewArgs): Promise<TemplateInsertPreview> {
  return invokeCommand<TemplateInsertPreview>("template_preview", {
    templateId: args.templateId,
    dimension: args.dimension,
    origin: args.origin,
    includeAir: args.includeAir,
  });
}

export function insertTemplate(args: TemplateInsertArgs): Promise<StructureInsertOutcome> {
  return invokeCommand<StructureInsertOutcome>("template_insert", {
    templateId: args.templateId,
    dimension: args.dimension,
    origin: args.origin,
    includeAir: args.includeAir,
    layerId: args.layerId ?? null,
  });
}
