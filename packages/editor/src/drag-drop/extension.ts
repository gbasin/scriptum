import type { Extension } from "@codemirror/state";
import { EditorView } from "@codemirror/view";

const IMAGE_FILE_EXTENSIONS = new Set([
  ".apng",
  ".avif",
  ".bmp",
  ".gif",
  ".jpeg",
  ".jpg",
  ".png",
  ".svg",
  ".webp",
]);

export interface DroppedFileUploadResult {
  readonly label?: string;
  readonly url: string;
}

export interface DropUploadProgress {
  readonly completedFiles: number;
  readonly currentFileName: string | null;
  readonly currentFilePercent: number;
  readonly failedFiles: number;
  readonly phase: "uploading" | "completed";
  readonly totalFiles: number;
}

export type DroppedFileUploader = (
  file: File,
  onProgress: (percent: number) => void,
) => Promise<DroppedFileUploadResult>;

export interface DragDropUploadOptions {
  readonly onError?: (error: Error, file: File) => void;
  readonly onProgress?: (progress: DropUploadProgress) => void;
  readonly uploadFile: DroppedFileUploader;
}

function normalizePercent(value: number): number {
  if (!Number.isFinite(value)) {
    return 0;
  }
  return Math.max(0, Math.min(100, Math.round(value)));
}

function sanitizeLabel(value: string): string {
  return value
    .replace(/\\/g, "\\\\")
    .replace(/\[/g, "\\[")
    .replace(/\]/g, "\\]");
}

function extensionFromName(name: string): string {
  const lastDot = name.lastIndexOf(".");
  if (lastDot <= 0 || lastDot === name.length - 1) {
    return "";
  }
  return name.slice(lastDot).toLowerCase();
}

function basenameWithoutExtension(name: string): string {
  const lastDot = name.lastIndexOf(".");
  if (lastDot <= 0) {
    return name;
  }
  return name.slice(0, lastDot);
}

export function isImageFile(file: Pick<File, "name" | "type">): boolean {
  if (file.type.toLowerCase().startsWith("image/")) {
    return true;
  }
  return IMAGE_FILE_EXTENSIONS.has(extensionFromName(file.name));
}

export function markdownForUploadedFile(
  file: Pick<File, "name" | "type">,
  result: DroppedFileUploadResult,
): string {
  const linkUrl = `<${result.url}>`;
  if (isImageFile(file)) {
    const fallbackAlt = basenameWithoutExtension(file.name).trim() || "image";
    const altText = sanitizeLabel(
      (result.label ?? fallbackAlt).trim() || "image",
    );
    return `![${altText}](${linkUrl})`;
  }
  const label = sanitizeLabel((result.label ?? file.name).trim() || "file");
  return `[${label}](${linkUrl})`;
}

function emitProgress(
  callback: ((progress: DropUploadProgress) => void) | undefined,
  progress: DropUploadProgress,
): void {
  callback?.(progress);
}

export async function uploadDroppedFiles(
  view: Pick<EditorView, "dispatch" | "state">,
  files: readonly File[],
  position: number,
  options: DragDropUploadOptions,
): Promise<string[]> {
  const totalFiles = files.length;
  if (totalFiles === 0) {
    return [];
  }

  const snippets: string[] = [];
  let completedFiles = 0;
  let failedFiles = 0;

  emitProgress(options.onProgress, {
    completedFiles,
    currentFileName: files[0]?.name ?? null,
    currentFilePercent: 0,
    failedFiles,
    phase: "uploading",
    totalFiles,
  });

  for (const file of files) {
    const currentFileName = file.name;
    let currentFilePercent = 0;

    emitProgress(options.onProgress, {
      completedFiles,
      currentFileName,
      currentFilePercent,
      failedFiles,
      phase: "uploading",
      totalFiles,
    });

    try {
      const result = await options.uploadFile(file, (percent) => {
        currentFilePercent = normalizePercent(percent);
        emitProgress(options.onProgress, {
          completedFiles,
          currentFileName,
          currentFilePercent,
          failedFiles,
          phase: "uploading",
          totalFiles,
        });
      });

      snippets.push(markdownForUploadedFile(file, result));
      completedFiles += 1;
      emitProgress(options.onProgress, {
        completedFiles,
        currentFileName,
        currentFilePercent: 100,
        failedFiles,
        phase: "uploading",
        totalFiles,
      });
    } catch (error) {
      failedFiles += 1;
      const normalizedError =
        error instanceof Error ? error : new Error("file upload failed");
      options.onError?.(normalizedError, file);
      emitProgress(options.onProgress, {
        completedFiles,
        currentFileName,
        currentFilePercent: 100,
        failedFiles,
        phase: "uploading",
        totalFiles,
      });
    }
  }

  if (snippets.length > 0) {
    const insertion = snippets.join("\n");
    view.dispatch({
      changes: {
        from: position,
        insert: insertion,
        to: position,
      },
      selection: {
        anchor: position + insertion.length,
      },
    });
  }

  emitProgress(options.onProgress, {
    completedFiles,
    currentFileName: null,
    currentFilePercent: 100,
    failedFiles,
    phase: "completed",
    totalFiles,
  });

  return snippets;
}

function droppedFilesFromTransfer(dataTransfer: DataTransfer | null): File[] {
  if (!dataTransfer || dataTransfer.files.length === 0) {
    return [];
  }
  return Array.from(dataTransfer.files);
}

function dropPosition(view: EditorView, event: DragEvent): number {
  const position =
    view.posAtCoords({ x: event.clientX, y: event.clientY }) ??
    view.state.selection.main.head;
  return Math.max(0, Math.min(position, view.state.doc.length));
}

export function dragDropUpload(options: DragDropUploadOptions): Extension {
  return EditorView.domEventHandlers({
    dragover(event) {
      const files = droppedFilesFromTransfer(event.dataTransfer);
      if (files.length === 0) {
        return false;
      }

      event.preventDefault();
      event.dataTransfer!.dropEffect = "copy";
      return true;
    },
    drop(event, view) {
      const files = droppedFilesFromTransfer(event.dataTransfer);
      if (files.length === 0) {
        return false;
      }

      event.preventDefault();
      const position = dropPosition(view, event);
      void uploadDroppedFiles(view, files, position, options);
      return true;
    },
  });
}

export const dragDropUploadExtension = dragDropUpload;
