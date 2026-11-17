// SPDX-License-Identifier: AGPL-3.0-or-later
//! The page document model (docs/specs/m1/07-pages-and-the-editor.md B7 to B13).
//!
//! A page body is a ProseMirror document as Tiptap serializes it: `{ "type": "doc", "content":
//! [...] }`. [`PageDoc::parse_new`] (a body to save) and [`PageDoc::parse_stored`] (a body read
//! back) read one into the typed model below, which lists every node, mark and attribute M1
//! allows; anything else is an [`Invalid`] naming the JSON Pointer of the
//! first offense, and nothing is dropped. [`PageDoc::to_value`] writes the model back as Tiptap
//! JSON with every attribute spelled out, so the stored body is the canonical form of what was
//! parsed and two saves of the same document compare equal.
//!
//! Where a node may go:
//!
//! - the document holds one or more of every block below;
//! - a blockquote, a callout, a list item and a table cell hold one or more *nested* blocks:
//!   paragraphs, bullet and ordered lists and code blocks. A list item starts with a paragraph.
//!   Images, embeds, tables, callouts and transcripts are top-level blocks only;
//! - paragraphs, headings and transcript lines hold text and hard breaks;
//! - a code block holds text without marks.
//!
//! Node and mark names are Tiptap's (`bulletList`, `codeBlock`, `hardBreak`); the nodes Tiptap
//! does not ship are `callout`, `videoEmbed`, `transcript` and `transcriptLine`, and the image
//! node names a course file rather than a URL (B10). Their attributes are snake_case.

mod parse;

use serde_json::{Map, Value, json};
use uuid::Uuid;

use crate::{auth_flows, items, users::is_format_char};

/// The largest body, in bytes of its compact JSON serialization (B7).
pub const MAX_BODY_BYTES: usize = 1_048_576;

/// The most nodes a body holds, the document and text nodes included (B7).
pub const MAX_NODES: usize = 20_000;

/// The largest `colspan` or `rowspan` of a table cell (B8).
pub const MAX_CELL_SPAN: u8 = 20;

/// The longest transcript speaker label, in characters (B11).
pub const MAX_SPEAKER_CHARS: usize = 40;

/// The longest video embed title, in characters: the course title limit.
pub const MAX_EMBED_TITLE_CHARS: usize = 200;

/// The languages a code block may name (B8). The browser highlights by this name (B17).
pub const CODE_LANGUAGES: &[&str] = &[
    "bash",
    "c",
    "cpp",
    "csharp",
    "css",
    "go",
    "html",
    "java",
    "javascript",
    "json",
    "kotlin",
    "markdown",
    "php",
    "python",
    "ruby",
    "rust",
    "sql",
    "swift",
    "typescript",
    "xml",
    "yaml",
];

/// The stored file types an image node may name (B10a): what spec 06 B6 sniffs for PNG, JPEG,
/// GIF and WebP.
pub const IMAGE_TYPES: &[&str] = &["image/png", "image/jpeg", "image/gif", "image/webp"];

/// A page body.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PageDoc {
    /// One or more top-level blocks.
    pub content: Vec<Block>,
}

/// A block node. Which blocks may go where is in the module documentation.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Block {
    Paragraph(Vec<Inline>),
    /// Level 2 to 4; the page title is the level 1.
    Heading {
        level: u8,
        content: Vec<Inline>,
    },
    BulletList(Vec<ListItem>),
    OrderedList {
        start: u32,
        items: Vec<ListItem>,
    },
    Blockquote(Vec<Block>),
    HorizontalRule,
    /// `language` is one of [`CODE_LANGUAGES`]; the text is the block's text nodes joined.
    CodeBlock {
        language: Option<String>,
        text: String,
    },
    /// One or more rows of one or more cells.
    Table(Vec<Vec<Cell>>),
    Image(Image),
    Callout {
        kind: CalloutKind,
        content: Vec<Block>,
    },
    VideoEmbed(VideoEmbed),
    /// One or more lines (B11).
    Transcript(Vec<TranscriptLine>),
}

/// A list item: a paragraph, then any nested blocks.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ListItem(pub Vec<Block>);

/// A table cell.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cell {
    /// A header cell (`tableHeader`) rather than a data cell (`tableCell`).
    pub header: bool,
    /// 1 to [`MAX_CELL_SPAN`].
    pub colspan: u8,
    /// 1 to [`MAX_CELL_SPAN`].
    pub rowspan: u8,
    /// One or more nested blocks.
    pub content: Vec<Block>,
}

/// An image: a course file of the page's course (B10).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Image {
    pub course_file_id: Uuid,
    /// Empty only when `decorative`.
    pub alt: String,
    pub decorative: bool,
}

/// A callout's kind.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CalloutKind {
    Info,
    Tip,
    Warning,
    Danger,
}

impl CalloutKind {
    const ALL: [Self; 4] = [Self::Info, Self::Tip, Self::Warning, Self::Danger];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Info => "info",
            Self::Tip => "tip",
            Self::Warning => "warning",
            Self::Danger => "danger",
        }
    }
}

/// Where an embedded video lives (B12).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoProvider {
    Youtube,
    Vimeo,
}

impl VideoProvider {
    const ALL: [Self; 2] = [Self::Youtube, Self::Vimeo];

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Youtube => "youtube",
            Self::Vimeo => "vimeo",
        }
    }

    /// Whether `id` has the provider's video id format: 11 characters of `[A-Za-z0-9_-]` for
    /// YouTube, 1 to 20 ASCII digits for Vimeo.
    #[must_use]
    pub fn id_is_valid(self, id: &str) -> bool {
        match self {
            Self::Youtube => {
                id.len() == 11
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            }
            Self::Vimeo => (1..=20).contains(&id.len()) && id.bytes().all(|b| b.is_ascii_digit()),
        }
    }
}

/// A YouTube or Vimeo video, by id only: the server builds the URL (B12, B16).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct VideoEmbed {
    pub provider: VideoProvider,
    pub video_id: String,
    /// The embed's accessible name: 1 to [`MAX_EMBED_TITLE_CHARS`] characters, not blank.
    pub title: String,
}

/// A line of a transcript block (B11).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct TranscriptLine {
    /// 1 to [`MAX_SPEAKER_CHARS`] characters, not blank. The core knows no label.
    pub speaker: String,
    pub content: Vec<Inline>,
}

/// An inline node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Inline {
    /// Not empty; no control character but tab and newline.
    Text {
        text: String,
        marks: Vec<Mark>,
    },
    HardBreak,
}

/// A mark on text; each at most once per text node.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Mark {
    Bold,
    Italic,
    Underline,
    Strike,
    Code,
    /// The form [`link_target`] stores (B9).
    Link {
        href: String,
    },
    Subscript,
    Superscript,
}

impl Mark {
    /// Tiptap's name of the mark.
    #[must_use]
    pub fn name(&self) -> &'static str {
        match self {
            Self::Bold => "bold",
            Self::Italic => "italic",
            Self::Underline => "underline",
            Self::Strike => "strike",
            Self::Code => "code",
            Self::Link { .. } => "link",
            Self::Subscript => "subscript",
            Self::Superscript => "superscript",
        }
    }
}

/// Why a body was refused: the first offense, in document order.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Invalid {
    /// The path from the body's root to the offending member, one segment per object key or
    /// array index; empty for the body itself.
    pub path: Vec<String>,
    /// What is wrong, for logs and tests; the API answers only the pointer.
    pub reason: &'static str,
}

impl std::fmt::Display for Invalid {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "the page body is invalid at {:?}: {}",
            self.pointer(),
            self.reason
        )
    }
}

impl std::error::Error for Invalid {}

impl Invalid {
    /// The JSON Pointer (RFC 6901) of [`Self::path`], relative to the body.
    #[must_use]
    pub fn pointer(&self) -> String {
        self.path
            .iter()
            .map(|segment| format!("/{}", segment.replace('~', "~0").replace('/', "~1")))
            .collect()
    }
}

impl PageDoc {
    /// The body of a page created without one: a document with one empty paragraph, as Tiptap
    /// writes an empty editor.
    #[must_use]
    pub fn empty() -> Self {
        Self {
            content: vec![Block::Paragraph(Vec::new())],
        }
    }

    /// `value` as a page body read back from the database, or the first rule of its structure
    /// it breaks (B7, B8). It skips the size check: a stored body's size was checked when it was
    /// saved. A body to save goes through [`Self::parse_new`].
    pub fn parse_stored(value: &Value) -> Result<Self, Invalid> {
        parse::document(value)
    }

    /// [`Self::parse_stored`] for a body about to be saved, which also refuses, at its root, a body
    /// whose stored form ([`Self::to_value`], every attribute written out) is over
    /// [`MAX_BODY_BYTES`] serialized. The limit is on what is stored, so a saved body always
    /// restores and saves again. The submitted JSON is bounded by the request body limit before
    /// it gets here.
    pub fn parse_new(value: &Value) -> Result<Self, Invalid> {
        let doc = Self::parse_stored(value)?;
        if doc.stored_size() > MAX_BODY_BYTES {
            return Err(Invalid {
                path: Vec::new(),
                reason: "the stored body is larger than 1 MiB",
            });
        }
        Ok(doc)
    }

    /// The size in bytes of the stored form's compact JSON.
    #[must_use]
    pub fn stored_size(&self) -> usize {
        self.to_value().to_string().len()
    }

    /// The body as Tiptap JSON, every attribute written out.
    #[must_use]
    pub fn to_value(&self) -> Value {
        node("doc", None, blocks(&self.content))
    }

    /// The course files the body names as images, each with the path of its `course_file_id`
    /// attribute, in document order. Images are top-level blocks only, so each path is
    /// `content/<index>/attrs/course_file_id`.
    #[must_use]
    pub fn images(&self) -> Vec<(Uuid, Vec<String>)> {
        self.content
            .iter()
            .enumerate()
            .filter_map(|(index, block)| match block {
                Block::Image(image) => Some((
                    image.course_file_id,
                    vec![
                        "content".to_owned(),
                        index.to_string(),
                        "attrs".to_owned(),
                        "course_file_id".to_owned(),
                    ],
                )),
                _ => None,
            })
            .collect()
    }
}

/// The characters no stored link target may hold: they could end an HTML attribute or start
/// markup in a renderer that forgot to escape one.
const PATH_REFUSED: &[char] = &['"', '\'', '<', '>', '`', '\\'];

/// `href` as the link target B9 allows, in the form to store, or `None`.
///
/// An absolute `https` or `http` URL as an external link item takes it
/// (`items::url_is_valid`), or a `mailto:` URL, is serialized by the `url` crate
/// (`Url::as_str`: scheme and host lowercased, some characters in a path, query or fragment
/// percent-encoded). That encoding does not cover every character that matters in HTML: a
/// `mailto:` address is an opaque path where only control characters are encoded, a host may
/// hold `"`, `'` and a backtick (and `%22` there is decoded to `"`), and `'` is never encoded in
/// a path or fragment. So the serialized form is checked again, as a same-origin path is: a
/// target holding any of `"`, `'`, `<`, `>`, a backtick, `\` or a control character is refused,
/// whatever its kind. A same-origin path also keeps spec m0/08 B19's rule (it starts with one
/// `/`, not `//` or `/\`, printable ASCII, no `.` or `..` segment, at most 2,048 bytes) and is
/// stored as given.
#[must_use]
pub fn link_target(href: &str) -> Option<String> {
    let stored = stored_form(href)?;
    let plain = !stored.contains(PATH_REFUSED) && !stored.chars().any(char::is_control);
    plain.then_some(stored)
}

/// The form [`link_target`] would store for `href` before its character check, or `None` when
/// `href` is no target B9 allows.
fn stored_form(href: &str) -> Option<String> {
    if href.starts_with('/') {
        return auth_flows::is_safe_path(href).then(|| href.to_owned());
    }
    let scheme = href.get(..7).map(str::to_ascii_lowercase);
    let valid = if scheme.as_deref() == Some("mailto:") {
        mailto_is_valid(href)
    } else {
        items::url_is_valid(href)
    };
    if !valid {
        return None;
    }
    url::Url::parse(href)
        .ok()
        .map(|url| url.as_str().to_owned())
}

/// A `mailto:` URL with an address part: at most [`items::MAX_URL_CHARS`] characters, no white
/// space, control or format character, and one the `url` crate parses.
fn mailto_is_valid(href: &str) -> bool {
    let plain = !href
        .chars()
        .any(|c| c.is_whitespace() || c.is_control() || is_format_char(c));
    plain
        && href.chars().count() <= items::MAX_URL_CHARS
        && href.len() > "mailto:".len()
        && url::Url::parse(href).is_ok_and(|url| url.scheme() == "mailto")
}

/// A node object: `type`, then `attrs` when given, then `content` when it is not empty.
fn node(kind: &str, attrs: Option<Value>, content: Vec<Value>) -> Value {
    let mut object = Map::new();
    object.insert("type".to_owned(), Value::from(kind));
    if let Some(attrs) = attrs {
        object.insert("attrs".to_owned(), attrs);
    }
    if !content.is_empty() {
        object.insert("content".to_owned(), Value::Array(content));
    }
    Value::Object(object)
}

fn blocks(blocks: &[Block]) -> Vec<Value> {
    blocks.iter().map(block).collect()
}

fn block(block: &Block) -> Value {
    match block {
        Block::Paragraph(content) => node("paragraph", None, inlines(content)),
        Block::Heading { level, content } => {
            node("heading", Some(json!({ "level": level })), inlines(content))
        }
        Block::BulletList(items) => node("bulletList", None, list_items(items)),
        Block::OrderedList { start, items } => node(
            "orderedList",
            Some(json!({ "start": start })),
            list_items(items),
        ),
        Block::Blockquote(content) => node("blockquote", None, blocks(content)),
        Block::HorizontalRule => node("horizontalRule", None, Vec::new()),
        Block::CodeBlock { language, text } => code_block(language.as_deref(), text),
        Block::Table(rows) => node(
            "table",
            None,
            rows.iter().map(|cells| row(cells.as_slice())).collect(),
        ),
        Block::Callout { kind, content } => node(
            "callout",
            Some(json!({ "kind": kind.as_str() })),
            blocks(content),
        ),
        Block::Image(image) => image_node(image),
        Block::VideoEmbed(embed) => embed_node(embed),
        Block::Transcript(lines) => node("transcript", None, lines.iter().map(line).collect()),
    }
}

fn code_block(language: Option<&str>, text: &str) -> Value {
    let content = if text.is_empty() {
        Vec::new()
    } else {
        vec![json!({ "type": "text", "text": text })]
    };
    node("codeBlock", Some(json!({ "language": language })), content)
}

fn image_node(image: &Image) -> Value {
    let attrs = json!({
        "course_file_id": image.course_file_id.to_string(),
        "alt": image.alt,
        "decorative": image.decorative,
    });
    node("image", Some(attrs), Vec::new())
}

fn embed_node(embed: &VideoEmbed) -> Value {
    let attrs = json!({
        "provider": embed.provider.as_str(),
        "video_id": embed.video_id,
        "title": embed.title,
    });
    node("videoEmbed", Some(attrs), Vec::new())
}

fn line(line: &TranscriptLine) -> Value {
    node(
        "transcriptLine",
        Some(json!({ "speaker": line.speaker })),
        inlines(&line.content),
    )
}

fn list_items(items: &[ListItem]) -> Vec<Value> {
    items
        .iter()
        .map(|item| node("listItem", None, blocks(&item.0)))
        .collect()
}

fn row(cells: &[Cell]) -> Value {
    let cells = cells
        .iter()
        .map(|cell| {
            let kind = if cell.header {
                "tableHeader"
            } else {
                "tableCell"
            };
            let attrs =
                json!({ "colspan": cell.colspan, "rowspan": cell.rowspan, "colwidth": null });
            node(kind, Some(attrs), blocks(&cell.content))
        })
        .collect();
    node("tableRow", None, cells)
}

fn inlines(content: &[Inline]) -> Vec<Value> {
    content
        .iter()
        .map(|inline| match inline {
            Inline::HardBreak => node("hardBreak", None, Vec::new()),
            Inline::Text { text, marks } => {
                let mut object = Map::new();
                object.insert("type".to_owned(), Value::from("text"));
                object.insert("text".to_owned(), Value::from(text.as_str()));
                if !marks.is_empty() {
                    object.insert(
                        "marks".to_owned(),
                        marks.iter().map(mark).collect::<Vec<_>>().into(),
                    );
                }
                Value::Object(object)
            }
        })
        .collect()
}

fn mark(mark: &Mark) -> Value {
    match mark {
        Mark::Link { href } => json!({ "type": "link", "attrs": { "href": href } }),
        other => json!({ "type": other.name() }),
    }
}

#[cfg(test)]
mod tests;
