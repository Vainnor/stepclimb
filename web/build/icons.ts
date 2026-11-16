// SPDX-License-Identifier: AGPL-3.0-or-later
// Stepclimb's default app icons (docs/specs/m1/15-installable-web-app-and-offline-reading.md B1):
// squares of the default brand's light `brand` color, the `theme-color` of `index.html` (the
// server's tests hold that equal to the default brand), written into `dist/icons/` by the build.
// The manifest lists them; `index.html` links the 192 px one as the `apple-touch-icon`. An
// organization's own icons arrive with logo upload (spec 12 Q6).
import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import { crc32, deflateSync } from "node:zlib";
import type { Plugin } from "vite";

/** The icons, by file name under `dist/`, and their size in px. */
export const ICONS: readonly (readonly [string, number])[] = [
  ["icons/icon-192.png", 192],
  ["icons/icon-512.png", 512],
  // Full bleed, so any mask a platform cuts stays inside the color.
  ["icons/icon-maskable-512.png", 512],
];

const INDEX = fileURLToPath(new URL("../index.html", import.meta.url));

/** The `#rrggbb` of `html`'s `theme-color` meta, as three bytes. */
export function themeColor(html: string): [number, number, number] {
  const match = /<meta name="theme-color" content="#([0-9a-f]{6})"/.exec(html);
  if (!match?.[1]) throw new Error('index.html has no <meta name="theme-color" content="#rrggbb">');
  const hex = match[1];
  return [0, 2, 4].map((i) => Number.parseInt(hex.slice(i, i + 2), 16)) as [number, number, number];
}

function chunk(type: string, data: Buffer): Buffer {
  const length = Buffer.alloc(4);
  length.writeUInt32BE(data.length);
  const body = Buffer.concat([Buffer.from(type, "latin1"), data]);
  const crc = Buffer.alloc(4);
  crc.writeUInt32BE(crc32(body));
  return Buffer.concat([length, body, crc]);
}

/** A `size` x `size` PNG of one opaque color: 8-bit RGB, no filter. */
export function solidPng(size: number, [r, g, b]: [number, number, number]): Buffer {
  const header = Buffer.alloc(13);
  header.writeUInt32BE(size, 0);
  header.writeUInt32BE(size, 4);
  header.set([8, 2, 0, 0, 0], 8); // bit depth 8, RGB, deflate, filter set 0, no interlace
  const row = Buffer.alloc(1 + size * 3);
  for (let x = 0; x < size; x++) row.set([r, g, b], 1 + x * 3);
  const pixels = Buffer.concat(Array.from({ length: size }, () => row));
  return Buffer.concat([
    Buffer.from([0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]),
    chunk("IHDR", header),
    chunk("IDAT", deflateSync(pixels)),
    chunk("IEND", Buffer.alloc(0)),
  ]);
}

export function appIcons(): Plugin {
  return {
    name: "stepclimb-app-icons",
    apply: "build",
    generateBundle() {
      const color = themeColor(readFileSync(INDEX, "utf8"));
      for (const [fileName, size] of ICONS) {
        this.emitFile({ type: "asset", fileName, source: solidPng(size, color) });
      }
    },
  };
}
