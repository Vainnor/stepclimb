// SPDX-License-Identifier: AGPL-3.0-or-later
import { describe, expect, it } from "vitest";
import { fileShow } from "./file-item";

describe("fileShow", () => {
  it("frames a PDF where the browser shows PDFs and offers the download where it does not", () => {
    expect(fileShow("application/pdf", true)).toBe("pdf");
    expect(fileShow("application/pdf", false)).toBe("pdf-fallback");
  });

  it("shows the image types the server serves inline", () => {
    for (const mime of ["image/png", "image/jpeg", "image/gif", "image/webp"]) {
      expect(fileShow(mime, false)).toBe("image");
    }
  });

  it("offers every other type as a download only", () => {
    for (const mime of ["application/octet-stream", "image/svg+xml", "text/plain; charset=utf-8"]) {
      expect(fileShow(mime, true)).toBe("download");
    }
  });
});
