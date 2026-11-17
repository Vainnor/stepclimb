// SPDX-License-Identifier: AGPL-3.0-or-later
//! The `PageDoc` schema of the OpenAPI document (docs/specs/m1/07-pages-and-the-editor.md B7,
//! B8, API), so API clients can generate page bodies.
//!
//! These types only describe the JSON; `stepclimb_core::page_doc` parses and checks it, with
//! rules a schema does not carry (lengths, the link rules, the 1 MiB and 20,000-node limits).
//! `tests/pages.rs` checks a body with every node, mark and attribute against this schema, so
//! the two cannot drift apart unseen.

use utoipa::{
    ToSchema,
    openapi::schema::{Object, ObjectBuilder, Type},
};

/// A page body: a ProseMirror document as Tiptap serializes it. At most 1 MiB serialized and
/// 20,000 nodes. Any node, mark or attribute not listed here is refused.
#[derive(ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageDoc {
    #[serde(rename = "type")]
    pub kind: PageDocKind,
    /// One or more blocks.
    pub content: Vec<PageBlock>,
}

/// The root node's type.
#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum PageDocKind {
    Doc,
}

/// A top-level block.
#[derive(ToSchema)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum PageBlock {
    Paragraph {
        #[schema(required = false, nullable = false)]
        content: Option<Vec<PageInline>>,
    },
    Heading {
        attrs: PageHeadingAttrs,
        #[schema(required = false, nullable = false)]
        content: Option<Vec<PageInline>>,
    },
    BulletList {
        /// One or more.
        content: Vec<PageListItem>,
    },
    OrderedList {
        #[schema(required = false, nullable = false)]
        attrs: Option<PageOrderedListAttrs>,
        /// One or more.
        content: Vec<PageListItem>,
    },
    Blockquote {
        /// One or more.
        content: Vec<PageNestedBlock>,
    },
    HorizontalRule,
    CodeBlock {
        #[schema(required = false, nullable = false)]
        attrs: Option<PageCodeBlockAttrs>,
        #[schema(required = false, nullable = false)]
        content: Option<Vec<PageCodeText>>,
    },
    Table {
        /// One or more rows.
        content: Vec<PageTableRow>,
    },
    Image {
        attrs: PageImageAttrs,
    },
    Callout {
        attrs: PageCalloutAttrs,
        /// One or more.
        content: Vec<PageNestedBlock>,
    },
    VideoEmbed {
        attrs: PageVideoEmbedAttrs,
    },
    Transcript {
        /// One or more lines.
        content: Vec<PageTranscriptLine>,
    },
}

/// A block inside a blockquote, a callout, a list item or a table cell.
#[derive(ToSchema)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
#[schema(no_recursion)]
pub enum PageNestedBlock {
    Paragraph {
        #[schema(required = false, nullable = false)]
        content: Option<Vec<PageInline>>,
    },
    BulletList {
        content: Vec<PageListItem>,
    },
    OrderedList {
        #[schema(required = false, nullable = false)]
        attrs: Option<PageOrderedListAttrs>,
        content: Vec<PageListItem>,
    },
    CodeBlock {
        #[schema(required = false, nullable = false)]
        attrs: Option<PageCodeBlockAttrs>,
        #[schema(required = false, nullable = false)]
        content: Option<Vec<PageCodeText>>,
    },
}

/// A list item: a paragraph first, then any nested blocks.
#[derive(ToSchema)]
#[serde(deny_unknown_fields)]
#[schema(no_recursion)]
pub struct PageListItem {
    #[serde(rename = "type")]
    pub kind: PageListItemKind,
    pub content: Vec<PageNestedBlock>,
}

#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum PageListItemKind {
    ListItem,
}

/// A table row of one or more cells.
#[derive(ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageTableRow {
    #[serde(rename = "type")]
    pub kind: PageTableRowKind,
    pub content: Vec<PageTableCell>,
}

#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum PageTableRowKind {
    TableRow,
}

/// A data or header cell.
#[derive(ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageTableCell {
    #[serde(rename = "type")]
    pub kind: PageTableCellKind,
    #[schema(required = false, nullable = false)]
    pub attrs: Option<PageCellAttrs>,
    /// One or more.
    pub content: Vec<PageNestedBlock>,
}

#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum PageTableCellKind {
    TableCell,
    TableHeader,
}

/// A cell's spans, each 1 to 20; 1 when omitted.
#[derive(ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageCellAttrs {
    #[schema(required = false, nullable = false, minimum = 1)]
    pub colspan: Option<u8>,
    #[schema(required = false, nullable = false, minimum = 1)]
    pub rowspan: Option<u8>,
    /// Always `null`: column widths are not kept.
    #[schema(required = false, schema_with = null_only)]
    pub colwidth: Option<()>,
}

/// A member whose only value is `null`.
fn null_only() -> Object {
    ObjectBuilder::new().schema_type(Type::Null).build()
}

/// A heading's level, 2 to 4; the page title is the level 1.
#[derive(ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageHeadingAttrs {
    #[schema(minimum = 2)]
    pub level: u8,
}

/// The number an ordered list starts at; 1 when omitted.
#[derive(ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageOrderedListAttrs {
    #[schema(required = false, nullable = false)]
    pub start: Option<u32>,
}

/// A code block's language for highlighting, one of `bash`, `c`, `cpp`, `csharp`, `css`, `go`,
/// `html`, `java`, `javascript`, `json`, `kotlin`, `markdown`, `php`, `python`, `ruby`, `rust`,
/// `sql`, `swift`, `typescript`, `xml`, `yaml`, or `null` for none.
#[derive(ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageCodeBlockAttrs {
    #[schema(required = false)]
    pub language: Option<String>,
}

/// A code block's text: no marks.
#[derive(ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageCodeText {
    #[serde(rename = "type")]
    pub kind: PageTextKind,
    /// Not empty; no control character but tab and newline.
    pub text: String,
}

#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum PageTextKind {
    Text,
}

/// An image: a PNG, JPEG, GIF or WebP course file of the page's course.
#[derive(ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageImageAttrs {
    #[schema(value_type = String, format = "uuid")]
    pub course_file_id: String,
    /// The image's text alternative; empty only when `decorative`. No control character.
    pub alt: String,
    /// Whether the image is decorative; `false` when omitted.
    #[schema(required = false, nullable = false)]
    pub decorative: Option<bool>,
}

/// A callout's kind.
#[derive(ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageCalloutAttrs {
    pub kind: PageCalloutKind,
}

#[derive(ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PageCalloutKind {
    Info,
    Tip,
    Warning,
    Danger,
}

/// A YouTube or Vimeo video by id; the server builds the player's URL.
#[derive(ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageVideoEmbedAttrs {
    pub provider: PageVideoProvider,
    /// YouTube: 11 characters of `A-Z`, `a-z`, `0-9`, `_` and `-`. Vimeo: 1 to 20 digits.
    pub video_id: String,
    /// The player's accessible name: 1 to 200 characters, not blank.
    pub title: String,
}

#[derive(ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum PageVideoProvider {
    Youtube,
    Vimeo,
}

/// A line of a transcript: who speaks, and what.
#[derive(ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageTranscriptLine {
    #[serde(rename = "type")]
    pub kind: PageTranscriptLineKind,
    pub attrs: PageTranscriptLineAttrs,
    #[schema(required = false, nullable = false)]
    pub content: Option<Vec<PageInline>>,
}

#[derive(ToSchema)]
#[serde(rename_all = "camelCase")]
pub enum PageTranscriptLineKind {
    TranscriptLine,
}

/// A transcript line's speaker label, chosen per block: 1 to 40 characters, not blank.
#[derive(ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageTranscriptLineAttrs {
    pub speaker: String,
}

/// Text or a line break inside a paragraph, a heading or a transcript line.
#[derive(ToSchema)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum PageInline {
    Text {
        /// Not empty; no control character but tab and newline.
        text: String,
        /// Each mark at most once.
        #[schema(required = false, nullable = false)]
        marks: Option<Vec<PageMark>>,
    },
    HardBreak,
}

/// A mark on text.
#[derive(ToSchema)]
#[serde(tag = "type", rename_all = "camelCase", deny_unknown_fields)]
pub enum PageMark {
    Bold,
    Italic,
    Underline,
    Strike,
    Code,
    Link { attrs: PageLinkAttrs },
    Subscript,
    Superscript,
}

/// A link's target: an absolute `https`, `http` or `mailto` URL, or a path on this site that
/// starts with one `/`.
#[derive(ToSchema)]
#[serde(deny_unknown_fields)]
pub struct PageLinkAttrs {
    pub href: String,
}
