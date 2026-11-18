import { invokeCommand } from "@/lib/ipc";
import type { OpenedProjectPayload } from "@/project/types";
import type {
  StructureExportPreview,
  StructureExportResult,
  StructureImportPreview,
  StructureImportProjectArgs,
  StructureInsertArgs,
  StructureInsertOutcome,
} from "@/project/structure-types";
import type { Bounds3 } from "@/renderer/types";

/** Keep uploads well below the backend's IPC and NBT limits. */
export const MAX_STRUCTURE_FILE_BYTES = 32 * 1024 * 1024;

/**
 * Reads a local structure file chosen through the browser file picker and returns base64.
 * The UI never sends filesystem paths: bytes are the only thing crossing IPC, and the user
 * explicitly picked the file.
 */
export async function readStructureFile(file: File): Promise<string> {
  if (!/\.nbt$/i.test(file.name)) {
    throw new Error(
      "Choose a Minecraft Java structure (.nbt) file. Sponge .schem is not supported yet.",
    );
  }
  if (file.size <= 0) {
    throw new Error("The selected structure file is empty.");
  }
  if (file.size > MAX_STRUCTURE_FILE_BYTES) {
    throw new Error(
      `The selected file is ${file.size.toLocaleString()} bytes; the import limit is ${MAX_STRUCTURE_FILE_BYTES.toLocaleString()} bytes.`,
    );
  }
  const bytes = new Uint8Array(await file.arrayBuffer());
  let binary = "";
  const chunkSize = 0x2000;
  for (let offset = 0; offset < bytes.length; offset += chunkSize) {
    binary += String.fromCharCode(...bytes.subarray(offset, offset + chunkSize));
  }
  return btoa(binary);
}

export function previewStructureImport(payload: string): Promise<StructureImportPreview> {
  return invokeCommand<StructureImportPreview>("structure_import_preview", { payload });
}

export function importStructureProject(
  args: StructureImportProjectArgs,
): Promise<OpenedProjectPayload> {
  return invokeCommand<OpenedProjectPayload>("structure_import_project", {
    name: args.name,
    sourceFileName: args.sourceFileName,
    payload: args.payload,
    dimension: args.dimension,
    origin: args.origin,
    includeAir: args.includeAir,
    editorState: args.editorState,
  });
}

export function insertStructure(args: StructureInsertArgs): Promise<StructureInsertOutcome> {
  return invokeCommand<StructureInsertOutcome>("structure_insert", {
    payload: args.payload,
    dimension: args.dimension,
    origin: args.origin,
    includeAir: args.includeAir,
  });
}

export function previewStructureExport(
  dimension: string,
  bounds: Bounds3 | null,
): Promise<StructureExportPreview> {
  return invokeCommand<StructureExportPreview>("structure_export_preview", {
    dimension,
    bounds,
  });
}

export function exportStructure(args: {
  dimension: string;
  bounds: Bounds3 | null;
  fileName: string;
  expectedChecksum: string;
}): Promise<StructureExportResult> {
  return invokeCommand<StructureExportResult>("structure_export", {
    dimension: args.dimension,
    bounds: args.bounds,
    fileName: args.fileName,
    expectedChecksum: args.expectedChecksum,
  });
}
