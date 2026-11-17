// SPDX-License-Identifier: AGPL-3.0-or-later
// A short label for a course file's type (docs/specs/m1/16-learning-ui-components.md B20a): the
// kinds the server's sniffer recognizes (docs/specs/m1/06-files.md B6) by name, any other file by
// the extension of its name, and a file with neither as "File". The full MIME type stays
// available beside the label for those who need it.
import type { Strings } from "@stepclimb/ui";

/** The MIME type without its parameters (`text/plain; charset=utf-8` -> `text/plain`). */
function essence(mime: string): string {
  return (mime.split(";")[0] ?? "").trim().toLowerCase();
}

const OOXML = "application/vnd.openxmlformats-officedocument.";
const ODF = "application/vnd.oasis.opendocument.";

/** The label of a MIME type the page names, or undefined. */
function knownLabel(type: string, t: Strings["t"]): string | undefined {
  switch (type) {
    case "application/pdf":
      return t("files.type.pdf");
    case `${OOXML}wordprocessingml.document`:
      return t("files.type.word");
    case `${OOXML}spreadsheetml.sheet`:
      return t("files.type.excel");
    case `${OOXML}presentationml.presentation`:
      return t("files.type.powerPoint");
    case `${ODF}text`:
      return t("files.type.odfText");
    case `${ODF}spreadsheet`:
      return t("files.type.odfSpreadsheet");
    case `${ODF}presentation`:
      return t("files.type.odfPresentation");
    case "text/plain":
      return t("files.type.text");
    case "application/ogg":
      return t("files.type.audioVideo");
    default:
      return undefined;
  }
}

/** The label of a type group, `image/*` and the like, or undefined. */
function groupLabel(type: string, t: Strings["t"]): string | undefined {
  if (type.startsWith("image/")) return t("files.type.image");
  if (type.startsWith("video/")) return t("files.type.video");
  if (type.startsWith("audio/")) return t("files.type.audio");
  return undefined;
}

/** The extension of `name`, upper-cased, or undefined when it has none. */
function extensionOf(name: string): string | undefined {
  const dot = name.lastIndexOf(".");
  if (dot <= 0 || dot === name.length - 1) return undefined;
  return name.slice(dot + 1).toUpperCase();
}

/** A short label for the type of the file `name` that the server read as `mime`. */
export function fileTypeLabel(mime: string, name: string, t: Strings["t"]): string {
  const type = essence(mime);
  const known = knownLabel(type, t) ?? groupLabel(type, t);
  if (known !== undefined) return known;
  const extension = extensionOf(name);
  return extension === undefined ? t("files.type.other") : t("files.type.extension", { extension });
}
