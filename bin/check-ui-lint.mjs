#!/usr/bin/env node
// SPDX-License-Identifier: AGPL-3.0-or-later
// Stepclimb UI's pattern lint (docs/specs/m0/13-base-ui-components.md B18, B19, B30; Q2): fails on
// raw colors, Tailwind's removed palette, physical-direction utilities and properties, and emoji.
// Biome covers the two checks that need imports: icon packages (`noRestrictedImports`) and color or
// type utilities in a Stepclimb UI component's `className` (packages/ui/lint/classname-styling.grit).
//
// Scope: every tracked or new TS, TSX and CSS file outside packages/tokens (which defines the
// colors). In TS and TSX only string literals, template text and JSX text are read, through the
// TypeScript parser, so a comment or an identifier never matches. In CSS, comments are skipped,
// colors are read from declaration values and utilities from `@apply`. Emoji are checked in
// packages/ui and web/src only.
//
// Usage: check-ui-lint.mjs [FILE...]   (no files: the whole repository)
import { execFileSync } from "node:child_process";
import { readFileSync } from "node:fs";
import { pathToFileURL } from "node:url";
import ts from "typescript";

const SOURCE = /\.(ts|tsx|css)$/;
const SKIP = [/^packages\/tokens\//, /^bin\/fixtures\//, /(^|\/)node_modules\//];
const EMOJI_SCOPE = [/^packages\/ui\//, /^web\/src\//];

// CSS named colors (CSS Color 4). `transparent` and `currentcolor` are keywords, not raw colors;
// system colors (Canvas, CanvasText) stay allowed for forced-colors mode.
const NAMED = `aliceblue antiquewhite aqua aquamarine azure beige bisque black blanchedalmond blue
blueviolet brown burlywood cadetblue chartreuse chocolate coral cornflowerblue cornsilk crimson cyan
darkblue darkcyan darkgoldenrod darkgray darkgreen darkgrey darkkhaki darkmagenta darkolivegreen
darkorange darkorchid darkred darksalmon darkseagreen darkslateblue darkslategray darkslategrey
darkturquoise darkviolet deeppink deepskyblue dimgray dimgrey dodgerblue firebrick floralwhite
forestgreen fuchsia gainsboro ghostwhite gold goldenrod gray green greenyellow grey honeydew hotpink
indianred indigo ivory khaki lavender lavenderblush lawngreen lemonchiffon lightblue lightcoral
lightcyan lightgoldenrodyellow lightgray lightgreen lightgrey lightpink lightsalmon lightseagreen
lightskyblue lightslategray lightslategrey lightsteelblue lightyellow lime limegreen linen magenta
maroon mediumaquamarine mediumblue mediumorchid mediumpurple mediumseagreen mediumslateblue
mediumspringgreen mediumturquoise mediumvioletred midnightblue mintcream mistyrose moccasin
navajowhite navy oldlace olive olivedrab orange orangered orchid palegoldenrod palegreen
paleturquoise palevioletred papayawhip peachpuff peru pink plum powderblue purple rebeccapurple red
rosybrown royalblue saddlebrown salmon seagreen seashell sienna silver skyblue slateblue slategray
slategrey snow springgreen steelblue tan teal thistle tomato turquoise violet wheat white whitesmoke
yellow yellowgreen`
  .split(/\s+/)
  .filter(Boolean);
const NAMED_RE = new RegExp(`(?<![\\w-])(?:${NAMED.join("|")})(?![\\w-])`, "gi");

const HEX = /(?<![\w&#-])#(?:[0-9a-f]{8}|[0-9a-f]{6}|[0-9a-f]{3,4})(?![\w-])/gi;
const COLOR_FN = /(?<![\w-])(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\(/gi;
const ARBITRARY_COLOR = new RegExp(
  `(?<![\\w-])[a-z-]+-\\[(?:color:)?(?:#[0-9a-f]{3,8}|(?:rgba?|hsla?|hwb|lab|lch|oklab|oklch|color)\\(|(?:${NAMED.join("|")})\\])[^\\]\\s]*\\]?`,
  "gi",
);
const PALETTE_NAMES =
  "slate|gray|zinc|neutral|stone|red|orange|amber|yellow|lime|green|emerald|teal|cyan|sky|blue|indigo|violet|purple|fuchsia|pink|rose";
const COLOR_UTILITIES =
  "bg|text|border(?:-[xytrblse])?|ring|ring-offset|outline|fill|stroke|decoration|divide|placeholder|caret|accent|shadow|inset-shadow|drop-shadow|from|via|to";
const PALETTE = new RegExp(
  `(?<![\\w-])(?:${COLOR_UTILITIES})-(?:(?:${PALETTE_NAMES})-(?:50|[1-9]00|950)|black|white)(?:\\/\\d+)?(?![\\w-])`,
  "g",
);
// A Tailwind length: a number or fraction, auto, px, full, or an arbitrary `[...]` or `(...)`.
const LENGTH = String.raw`(?:\d[\d./]*|auto|px|full|\[[^\]\s]*\]|\([^)\s]*\))`;
const PHYSICAL_UTILITY = new RegExp(
  String.raw`(?<![\w-])-?(?:(?:scroll-)?[mp][lr]-${LENGTH}|(?:left|right)-${LENGTH}|border-[lr](?:-[\w-]+)?|rounded-[tb]?[lr](?:-[\w-]+)?|(?:text|float|clear)-(?:left|right))(?![\w-])`,
  "g",
);
const PHYSICAL_PROPERTY =
  /(?<![\w-])(?:(?:margin|padding|scroll-margin|scroll-padding|border)-(?:left|right)(?:-[a-z]+)?|border-(?:top|bottom)-(?:left|right)-radius|left|right)\s*:/g;
const PHYSICAL_VALUE = /(?<![\w-])(?:text-align|float|clear)\s*:\s*(?:left|right)(?![\w-])/g;
const EMOJI = /\p{Emoji_Presentation}|\p{Extended_Pictographic}️/gu;

const TOKEN_FIX =
  "use a token utility such as `bg-brand` or `text-fg-default`, or a `var(--sc-*)` token";
const LOGICAL_FIX =
  "use a logical one: `ms-`, `me-`, `ps-`, `pe-`, `start-`, `end-`, `text-start`, `rounded-s`";

/** A finding: the rule broken, where, and the fix. */
function finding(file, line, rule, text, message) {
  return { file, line, rule, text, message };
}

/** Matches of `re` in `text`, as [index, matched text]. */
function* matches(re, text) {
  re.lastIndex = 0;
  for (let m = re.exec(text); m !== null; m = re.exec(text)) yield [m.index, m[0]];
}

/** Color and utility findings in one piece of text (a string literal, a CSS value, an @apply). */
function scanText(text, at, isCss) {
  const out = [];
  const arbitrary = [];
  for (const [i, m] of matches(ARBITRARY_COLOR, text)) {
    arbitrary.push([i, i + m.length]);
    out.push([at(i), "raw-color", m, `\`${m}\` is an arbitrary color; ${TOKEN_FIX}`]);
  }
  const inArbitrary = (i) => arbitrary.some(([s, e]) => i >= s && i < e);
  const colors = [HEX, COLOR_FN, ...(isCss ? [NAMED_RE] : [])];
  for (const re of colors) {
    for (const [i, m] of matches(re, text)) {
      if (inArbitrary(i)) continue;
      out.push([at(i), "raw-color", m, `\`${m}\` is a raw color; ${TOKEN_FIX}`]);
    }
  }
  if (!isCss) {
    const whole = text.trim().toLowerCase();
    if (NAMED.includes(whole)) {
      out.push([at(0), "raw-color", whole, `\`${whole}\` is a named color; ${TOKEN_FIX}`]);
    }
  }
  for (const [i, m] of matches(PALETTE, text)) {
    out.push([
      at(i),
      "palette",
      m,
      `\`${m}\` is from Tailwind's removed palette and draws nothing; ${TOKEN_FIX}`,
    ]);
  }
  for (const [i, m] of matches(PHYSICAL_UTILITY, text)) {
    out.push([at(i), "logical-properties", m, `\`${m}\` is a physical direction; ${LOGICAL_FIX}`]);
  }
  return out;
}

function lineOf(text, index) {
  let line = 1;
  for (let i = 0; i < index; i++) if (text.charCodeAt(i) === 10) line++;
  return line;
}

/** Blank comments in CSS, keeping offsets and line numbers. */
function blankCssComments(text) {
  return text.replace(/\/\*[\s\S]*?\*\//g, (c) => c.replace(/[^\n]/g, " "));
}

function scanCss(file, source) {
  const text = blankCssComments(source);
  const found = [];
  const at = (base) => (i) => lineOf(text, base + i);
  // Declaration values: `prop: value` up to `;` or `}`. Custom property names and var() references
  // are blanked so a token name such as `--sc-line-strong` is not read as a color.
  for (const [i, m] of matches(/(?<![\w-])(-?[a-z][\w-]*)\s*:([^;{}]*)[;}]/gi, text)) {
    const valueStart = i + m.indexOf(":") + 1;
    const value = m.slice(m.indexOf(":") + 1, -1).replace(/--[\w-]+/g, (v) => " ".repeat(v.length));
    for (const [line, rule, t, msg] of scanText(value, at(valueStart), true)) {
      if (rule === "logical-properties") continue;
      found.push(finding(file, line, rule, t, msg));
    }
  }
  for (const [i, m] of matches(/@apply\s+([^;]+);/g, text)) {
    for (const [line, rule, t, msg] of scanText(m, at(i), false)) {
      found.push(finding(file, line, rule, t, msg));
    }
  }
  for (const re of [PHYSICAL_PROPERTY, PHYSICAL_VALUE]) {
    for (const [i, m] of matches(re, text)) {
      found.push(
        finding(
          file,
          lineOf(text, i),
          "logical-properties",
          m,
          `\`${m}\` is a physical direction; use \`inset-inline-start\`, \`margin-inline-end\`, \`text-align: start\` and the like`,
        ),
      );
    }
  }
  return found;
}

/** String literals, template text and JSX text of a TS or TSX file, as [start offset, text]. */
function* tsTexts(file, source) {
  const kind = file.endsWith(".tsx") ? ts.ScriptKind.TSX : ts.ScriptKind.TS;
  const sf = ts.createSourceFile(file, source, ts.ScriptTarget.Latest, true, kind);
  const stack = [sf];
  while (stack.length > 0) {
    const node = stack.pop();
    switch (node.kind) {
      case ts.SyntaxKind.StringLiteral:
      case ts.SyntaxKind.NoSubstitutionTemplateLiteral:
      case ts.SyntaxKind.TemplateHead:
      case ts.SyntaxKind.TemplateMiddle:
      case ts.SyntaxKind.TemplateTail:
      case ts.SyntaxKind.JsxText:
        yield [node.getStart(sf), node.getText(sf), node.kind === ts.SyntaxKind.JsxText];
        break;
      default:
        break;
    }
    node.forEachChild((child) => {
      stack.push(child);
    });
  }
}

function scanTs(file, source) {
  const found = [];
  const emoji = EMOJI_SCOPE.some((re) => re.test(file));
  for (const [start, raw, isJsxText] of tsTexts(file, source)) {
    // A literal's text includes its quotes; JSX text has none.
    const body = isJsxText ? raw : raw.slice(1, -1);
    const offset = isJsxText ? start : start + 1;
    const at = (i) => lineOf(source, offset + i);
    if (!isJsxText) {
      for (const [line, rule, t, msg] of scanText(body, at, false)) {
        found.push(finding(file, line, rule, t, msg));
      }
    }
    if (emoji) {
      for (const [i, m] of matches(EMOJI, body)) {
        found.push(
          finding(
            file,
            at(i),
            "emoji",
            m,
            "emoji in UI text; icons are Phosphor only, through `Icon`",
          ),
        );
      }
    }
  }
  return found;
}

/** Findings for one file, by its repository-relative path. */
export function checkSource(file, source) {
  if (!SOURCE.test(file) || SKIP.some((re) => re.test(file))) return [];
  const found = file.endsWith(".css") ? scanCss(file, source) : scanTs(file, source);
  return found.sort((a, b) => a.line - b.line);
}

function repositoryFiles() {
  const out = execFileSync("git", ["ls-files", "--cached", "--others", "--exclude-standard"], {
    encoding: "utf8",
  });
  return [...new Set(out.split("\n").filter(Boolean))].sort();
}

function main(args) {
  const files = (args.length > 0 ? args : repositoryFiles()).filter(
    (f) => SOURCE.test(f) && !SKIP.some((re) => re.test(f)),
  );
  const found = [];
  let checked = 0;
  for (const file of files) {
    let source;
    try {
      source = readFileSync(file, "utf8");
    } catch (err) {
      // A file listed by git but deleted in the working tree has nothing to check.
      if (err.code === "ENOENT") continue;
      throw err;
    }
    checked++;
    found.push(...checkSource(file, source));
  }
  for (const f of found) console.error(`${f.file}:${f.line}: ${f.rule}: ${f.message}`);
  if (found.length > 0) {
    console.error(`ui-lint: FAILED (${found.length} findings in ${checked} files)`);
    return 1;
  }
  console.log(`ui-lint: ok (${checked} files)`);
  return 0;
}

if (import.meta.url === pathToFileURL(process.argv[1] ?? "").href) {
  process.exitCode = main(process.argv.slice(2));
}
