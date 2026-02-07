// @vitest-environment jsdom
import { EditorState } from "@codemirror/state";
import { EditorView } from "@codemirror/view";
import { describe, expect, it, vi } from "vitest";
import {
  markdownForUploadedFile,
  type DropUploadProgress,
  type DroppedFileUploader,
  uploadDroppedFiles,
} from "./extension.js";

describe("dragDropUploadExtension helpers", () => {
  it("builds image markdown syntax for uploaded image files", () => {
    const file = new File(["image"], "diagram.png", { type: "image/png" });
    expect(markdownForUploadedFile(file, { url: "https://cdn.example/diagram.png" })).toBe(
      "![diagram](<https://cdn.example/diagram.png>)",
    );
  });

  it("builds link markdown syntax for uploaded non-image files", () => {
    const file = new File(["doc"], "guide.pdf", { type: "application/pdf" });
    expect(markdownForUploadedFile(file, { url: "https://cdn.example/guide.pdf" })).toBe(
      "[guide.pdf](<https://cdn.example/guide.pdf>)",
    );
  });
});

describe("uploadDroppedFiles", () => {
  it("uploads multiple files in order and inserts markdown links", async () => {
    const progressUpdates: DropUploadProgress[] = [];
    const uploadFile: DroppedFileUploader = async (file, onProgress) => {
      onProgress(33);
      onProgress(100);
      return {
        url: `https://uploads.example/${encodeURIComponent(file.name)}`,
      };
    };
    const view = createView("before\nafter");

    const inserted = await uploadDroppedFiles(
      view,
      [
        new File(["img"], "preview.png", { type: "image/png" }),
        new File(["txt"], "notes.txt", { type: "text/plain" }),
      ],
      "before\n".length,
      {
        onProgress: (progress) => progressUpdates.push(progress),
        uploadFile,
      },
    );

    expect(inserted).toEqual([
      "![preview](<https://uploads.example/preview.png>)",
      "[notes.txt](<https://uploads.example/notes.txt>)",
    ]);
    expect(view.state.doc.toString()).toBe(
      [
        "before",
        "![preview](<https://uploads.example/preview.png>)",
        "[notes.txt](<https://uploads.example/notes.txt>)after",
      ].join("\n"),
    );
    expect(progressUpdates.at(-1)).toEqual({
      completedFiles: 2,
      currentFileName: null,
      currentFilePercent: 100,
      failedFiles: 0,
      phase: "completed",
      totalFiles: 2,
    });

    view.destroy();
  });

  it("keeps successful uploads when one file fails", async () => {
    const onError = vi.fn();
    const view = createView("seed");
    const uploadFile: DroppedFileUploader = async (file, onProgress) => {
      onProgress(100);
      if (file.name === "broken.png") {
        throw new Error("upload failed");
      }
      return { url: `https://uploads.example/${file.name}` };
    };

    const inserted = await uploadDroppedFiles(
      view,
      [
        new File(["bad"], "broken.png", { type: "image/png" }),
        new File(["ok"], "report.md", { type: "text/markdown" }),
      ],
      0,
      {
        onError,
        uploadFile,
      },
    );

    expect(inserted).toEqual(["[report.md](<https://uploads.example/report.md>)"]);
    expect(view.state.doc.toString()).toBe(
      "[report.md](<https://uploads.example/report.md>)seed",
    );
    expect(onError).toHaveBeenCalledTimes(1);
    expect(onError.mock.calls[0]?.[0]).toBeInstanceOf(Error);
    expect(onError.mock.calls[0]?.[1]).toBeInstanceOf(File);

    view.destroy();
  });
});

function createView(source: string): EditorView {
  const parent = document.createElement("div");
  document.body.appendChild(parent);
  return new EditorView({
    parent,
    state: EditorState.create({
      doc: source,
    }),
  });
}
