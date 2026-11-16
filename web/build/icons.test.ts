// SPDX-License-Identifier: AGPL-3.0-or-later
// @vitest-environment node
import { readFileSync } from "node:fs";
import { inflateSync } from "node:zlib";
import { describe, expect, it } from "vitest";
import { solidPng, themeColor } from "./icons";

/** The width, height, color type and every pixel of an 8-bit RGB PNG without filters. */
function decode(png: Buffer) {
  expect(png.subarray(0, 8)).toEqual(Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]));
  const chunks = new Map<string, Buffer>();
  for (let at = 8; at < png.length; ) {
    const length = png.readUInt32BE(at);
    chunks.set(png.toString("latin1", at + 4, at + 8), png.subarray(at + 8, at + 8 + length));
    at += 12 + length;
  }
  const header = chunks.get("IHDR") as Buffer;
  const width = header.readUInt32BE(0);
  const height = header.readUInt32BE(4);
  const raw = inflateSync(chunks.get("IDAT") as Buffer);
  const pixels = new Set<string>();
  const stride = 1 + width * 3;
  for (let y = 0; y < height; y++) {
    expect(raw[y * stride]).toBe(0);
    for (let x = 0; x < width; x++) {
      const i = y * stride + 1 + x * 3;
      pixels.add(`${raw[i]},${raw[i + 1]},${raw[i + 2]}`);
    }
  }
  return { width, height, depth: header[8], colorType: header[9], pixels, end: chunks.has("IEND") };
}

describe("the default icons (spec 15 B1)", () => {
  it("are squares of one color at the size asked for", () => {
    for (const size of [192, 512]) {
      const icon = decode(solidPng(size, [0, 115, 115]));
      expect(icon).toMatchObject({ width: size, height: size, depth: 8, colorType: 2, end: true });
      expect([...icon.pixels]).toEqual(["0,115,115"]);
    }
  });

  it("take index.html's theme-color, the default brand's", () => {
    const html = readFileSync(new URL("../index.html", import.meta.url), "utf8");
    expect(themeColor(html)).toEqual([0x00, 0x73, 0x73]);
    expect(() => themeColor("<head></head>")).toThrow(/theme-color/);
  });
});
