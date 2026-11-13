# Content and IP Rules (always loaded)

Stepclimb is AGPL-3.0-or-later; `packages/api-client` is Apache-2.0 (`LICENSE`, ADR 0002). Every
committed file must be ours to publish under those licenses or carry its own compatible license.

---

## Other learning platforms

- **Canvas and Moodle are UX references only.** Never copy or translate their code, templates,
  strings or schemas into Stepclimb. Learning from their documentation and from using them is
  fine.
- Moodle compatibility is not a goal (ADR 0004). Import formats (GIFT, Moodle XML, QTI) come
  after 1.0 and never shape Stepclimb's own data model.
- Adapted UI code (shadcn, Kibo UI patterns and the like, all MIT) keeps its notice in
  `packages/ui/THIRD_PARTY.md`.

## ATC and VATSIM material

- **No vNAS assets**: no CRC or ERAM screenshots, symbols, fonts, sounds or files. The ATC pack
  draws its own scope-style visuals; command syntax comes from vNAS's public documentation,
  summarized in our own words.
- **FAA NASR and CIFP** data is public domain; record the cycle and source URL with any data
  derived from it and attribute the FAA in the docs.
- **OpenStreetMap** data, if it is ever used (taxi routes), is ODbL: attribution required and
  derived databases shared alike. Decide its use in an ADR first.
- VATSIM, VATUSA and vZDC names and logos are their owners'. vZDC's branding lives in vZDC's
  instance configuration, never in this repository except the plan's example theme values.

## Course material and member data

- **No copyrighted course material in git**: no vZDC or VATUSA course content, SOPs, LOAs or exam
  questions, no textbooks or manuals. Test fixtures are written for the test.
- **No real member data in git**: no names, emails, CIDs, rosters, exports or database dumps.
  They live in `private/` (gitignored); content-guard refuses writes there and git-guard refuses a
  forced add.
- Local reference material lives in `references/` (gitignored), listed in `references/INDEX.md`
  with its license. Cite by `INDEX.md` id or URL; summarize, never paste long passages.

## Fonts and images

- Fonts are OFL (Atkinson Hyperlegible Next and Mono, Source Serif 4, Roboto), installed from
  `@fontsource`; their license travels with the package.
- No image, icon or illustration whose license is unknown. Icons are Phosphor (MIT) only.
