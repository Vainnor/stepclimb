// SPDX-License-Identifier: AGPL-3.0-or-later
// The short type labels of the files list (docs/specs/m1/16-learning-ui-components.md B20a).
import { createStrings } from "@stepclimb/ui";
import { describe, expect, it } from "vitest";
import { catalogs } from "../messages";
import { fileTypeLabel } from "./file-type";

const { t } = createStrings({ catalogs, isDevelopment: true });

describe("fileTypeLabel", () => {
  it("names every type the server's sniffer gives", () => {
    const cases: [string, string][] = [
      ["application/pdf", "PDF"],
      ["image/png", "Image"],
      ["image/webp", "Image"],
      ["video/mp4", "Video"],
      ["audio/mpeg", "Audio"],
      ["audio/ogg", "Audio"],
      ["application/ogg", "Audio or video"],
      ["text/plain; charset=utf-8", "Plain text"],
      ["application/vnd.openxmlformats-officedocument.wordprocessingml.document", "Word document"],
      ["application/vnd.openxmlformats-officedocument.spreadsheetml.sheet", "Excel spreadsheet"],
      [
        "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        "PowerPoint presentation",
      ],
      ["application/vnd.oasis.opendocument.text", "OpenDocument text"],
      ["application/vnd.oasis.opendocument.spreadsheet", "OpenDocument spreadsheet"],
      ["application/vnd.oasis.opendocument.presentation", "OpenDocument presentation"],
    ];
    for (const [mime, label] of cases) expect(fileTypeLabel(mime, "x", t), mime).toBe(label);
  });

  it("falls back to the name's extension, then to File", () => {
    expect(fileTypeLabel("application/octet-stream", "archive.tar.gz", t)).toBe("GZ file");
    expect(fileTypeLabel("application/octet-stream", "notes.", t)).toBe("File");
    expect(fileTypeLabel("application/octet-stream", ".profile", t)).toBe("File");
    expect(fileTypeLabel("application/octet-stream", "README", t)).toBe("File");
  });
});
