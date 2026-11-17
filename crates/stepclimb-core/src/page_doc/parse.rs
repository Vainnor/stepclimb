// SPDX-License-Identifier: AGPL-3.0-or-later
//! [`super::PageDoc::parse`]: Tiptap JSON to the typed model, refusing the first member the
//! schema does not allow with its path.
//!
//! The recursion follows the JSON's nesting, which serde_json bounds at 128 levels when it
//! parses a request or a stored body.

use serde_json::{Map, Value};
use uuid::Uuid;

use super::{
    Block, CODE_LANGUAGES, CalloutKind, Cell, Image, Inline, Invalid, ListItem, MAX_BODY_BYTES,
    MAX_CELL_SPAN, MAX_EMBED_TITLE_CHARS, MAX_NODES, MAX_SPEAKER_CHARS, Mark, PageDoc,
    TranscriptLine, VideoEmbed, VideoProvider, href_is_valid,
};

/// A path from the body's root.
type Path = Vec<String>;

fn at(path: &Path, segment: impl ToString) -> Path {
    let mut child = path.clone();
    child.push(segment.to_string());
    child
}

fn refuse<T>(path: Path, reason: &'static str) -> Result<T, Invalid> {
    Err(Invalid { path, reason })
}

/// Where a block is: the document takes every block, the inside of a blockquote, callout, list
/// item or table cell only nested ones.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Place {
    Document,
    Nested,
}

/// The blocks a [`Place::Nested`] position takes.
const NESTED: &[&str] = &["paragraph", "bulletList", "orderedList", "codeBlock"];

/// The attributes each node type takes; a type not listed takes none.
const ATTRS: &[(&str, &[&str])] = &[
    ("heading", &["level"]),
    ("orderedList", &["start"]),
    ("codeBlock", &["language"]),
    ("tableCell", &["colspan", "rowspan", "colwidth"]),
    ("tableHeader", &["colspan", "rowspan", "colwidth"]),
    ("image", &["course_file_id", "alt", "decorative"]),
    ("callout", &["kind"]),
    ("videoEmbed", &["provider", "video_id", "title"]),
    ("transcriptLine", &["speaker"]),
];

/// Every node type but `text`, which [`Parser::inline`] reads.
const KINDS: &[&str] = &[
    "doc",
    "paragraph",
    "heading",
    "bulletList",
    "orderedList",
    "listItem",
    "blockquote",
    "horizontalRule",
    "hardBreak",
    "codeBlock",
    "table",
    "tableRow",
    "tableHeader",
    "tableCell",
    "image",
    "callout",
    "videoEmbed",
    "transcript",
    "transcriptLine",
];

/// The node types that take no `content`.
const LEAVES: &[&str] = &["horizontalRule", "hardBreak", "image", "videoEmbed"];

/// A node object with its members checked against its type.
struct Node<'a> {
    kind: &'a str,
    attrs: Option<&'a Map<String, Value>>,
    content: &'a [Value],
    path: Path,
}

impl<'a> Node<'a> {
    /// The attribute `name`, if given.
    fn attr(&self, name: &str) -> Option<&'a Value> {
        self.attrs.and_then(|attrs| attrs.get(name))
    }

    fn attr_path(&self, name: &str) -> Path {
        at(&at(&self.path, "attrs"), name)
    }

    /// The attribute `name`, which must be given.
    fn required(&self, name: &str) -> Result<&'a Value, Invalid> {
        self.attr(name).map_or_else(
            || refuse(self.attr_path(name), "a required attribute is missing"),
            Ok,
        )
    }

    fn string(&self, name: &str) -> Result<&'a str, Invalid> {
        self.required(name)?
            .as_str()
            .map_or_else(|| refuse(self.attr_path(name), "not a string"), Ok)
    }

    /// An integer attribute in `range`, or `default` when it is absent.
    fn integer(
        &self,
        name: &str,
        range: std::ops::RangeInclusive<u64>,
        default: u64,
    ) -> Result<u64, Invalid> {
        let Some(value) = self.attr(name) else {
            return Ok(default);
        };
        match value.as_u64() {
            Some(n) if range.contains(&n) => Ok(n),
            _ => refuse(self.attr_path(name), "not an integer in range"),
        }
    }

    /// One of `choices` by name.
    fn choice<T: Copy>(
        &self,
        name: &str,
        choices: &[T],
        of: fn(T) -> &'static str,
    ) -> Result<T, Invalid> {
        let value = self.string(name)?;
        choices
            .iter()
            .copied()
            .find(|c| of(*c) == value)
            .map_or_else(|| refuse(self.attr_path(name), "not an allowed value"), Ok)
    }

    fn child_path(&self, index: usize) -> Path {
        at(&at(&self.path, "content"), index)
    }

    /// The children, which must be at least one.
    fn non_empty(&self) -> Result<&'a [Value], Invalid> {
        if self.content.is_empty() {
            return refuse(at(&self.path, "content"), "needs at least one child");
        }
        Ok(self.content)
    }
}

/// Counts nodes across the whole body.
struct Parser {
    nodes: usize,
}

/// A whole body (B7).
pub(super) fn document(value: &Value) -> Result<PageDoc, Invalid> {
    // Serializing a `Value` cannot fail: its keys are strings.
    let size = serde_json::to_vec(value).map_or(usize::MAX, |bytes| bytes.len());
    if size > MAX_BODY_BYTES {
        return refuse(Vec::new(), "the body is larger than 1 MiB");
    }
    let mut parser = Parser { nodes: 0 };
    let root = parser.node(value, Vec::new())?;
    if root.kind != "doc" {
        return refuse(vec!["type".to_owned()], "the root is not a doc");
    }
    let content = root
        .non_empty()?
        .iter()
        .enumerate()
        .map(|(i, child)| parser.block(child, root.child_path(i), Place::Document))
        .collect::<Result<_, _>>()?;
    Ok(PageDoc { content })
}

impl Parser {
    /// `value` as a node: an object with a string `type`, and only the members that type takes.
    /// Text nodes are read by [`Self::inline`]; here `text` and `marks` are unknown members.
    fn node<'a>(&mut self, value: &'a Value, path: Path) -> Result<Node<'a>, Invalid> {
        let Some(object) = value.as_object() else {
            return refuse(path, "a node is not an object");
        };
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return refuse(path, "the body has more than 20,000 nodes");
        }
        let Some(kind) = object.get("type").and_then(Value::as_str) else {
            return refuse(at(&path, "type"), "a node has no string type");
        };
        if !KINDS.contains(&kind) {
            return refuse(
                at(&path, "type"),
                "an unknown node, or text where it is not allowed",
            );
        }
        if let Some(key) = object
            .keys()
            .find(|key| !matches!(key.as_str(), "type" | "attrs" | "content"))
        {
            return refuse(at(&path, key), "a member this node does not take");
        }
        let attrs = self.attrs(object, kind, &path)?;
        let content = match object.get("content") {
            None => &[][..],
            Some(_) if LEAVES.contains(&kind) => {
                return refuse(at(&path, "content"), "this node takes no content");
            }
            Some(Value::Array(children)) => children.as_slice(),
            Some(_) => return refuse(at(&path, "content"), "content is not an array"),
        };
        Ok(Node {
            kind,
            attrs,
            content,
            path,
        })
    }

    /// The node's `attrs`, refusing one its type does not take.
    fn attrs<'a>(
        &self,
        object: &'a Map<String, Value>,
        kind: &str,
        path: &Path,
    ) -> Result<Option<&'a Map<String, Value>>, Invalid> {
        let Some(attrs) = object.get("attrs") else {
            return Ok(None);
        };
        let Some(attrs) = attrs.as_object() else {
            return refuse(at(path, "attrs"), "attrs is not an object");
        };
        let allowed = ATTRS
            .iter()
            .find(|(name, _)| *name == kind)
            .map_or(&[][..], |(_, names)| *names);
        if let Some(key) = attrs.keys().find(|key| !allowed.contains(&key.as_str())) {
            return refuse(
                at(&at(path, "attrs"), key),
                "an attribute this node does not take",
            );
        }
        Ok(Some(attrs))
    }

    fn blocks(&mut self, parent: &Node<'_>, place: Place) -> Result<Vec<Block>, Invalid> {
        parent
            .non_empty()?
            .iter()
            .enumerate()
            .map(|(i, child)| self.block(child, parent.child_path(i), place))
            .collect()
    }

    fn block(&mut self, value: &Value, path: Path, place: Place) -> Result<Block, Invalid> {
        let node = self.node(value, path)?;
        if place == Place::Nested && !NESTED.contains(&node.kind) {
            return refuse(at(&node.path, "type"), "this block is not allowed here");
        }
        match node.kind {
            "paragraph" => Ok(Block::Paragraph(self.inlines(&node)?)),
            "heading" => {
                node.required("level")?;
                Ok(Block::Heading {
                    level: narrow(node.integer("level", 2..=4, 2)?),
                    content: self.inlines(&node)?,
                })
            }
            "bulletList" => Ok(Block::BulletList(self.list_items(&node)?)),
            "orderedList" => Ok(Block::OrderedList {
                start: narrow(node.integer("start", 0..=u64::from(u32::MAX), 1)?),
                items: self.list_items(&node)?,
            }),
            "blockquote" => Ok(Block::Blockquote(self.blocks(&node, Place::Nested)?)),
            "horizontalRule" => Ok(Block::HorizontalRule),
            "codeBlock" => self.code_block(&node),
            "table" => self.table(&node),
            "image" => image(&node),
            "callout" => Ok(Block::Callout {
                kind: node.choice("kind", &CalloutKind::ALL, CalloutKind::as_str)?,
                content: self.blocks(&node, Place::Nested)?,
            }),
            "videoEmbed" => video_embed(&node),
            "transcript" => self.transcript(&node),
            _ => refuse(at(&node.path, "type"), "an unknown block"),
        }
    }

    fn list_items(&mut self, list: &Node<'_>) -> Result<Vec<ListItem>, Invalid> {
        let mut items = Vec::new();
        for (i, value) in list.non_empty()?.iter().enumerate() {
            let item = self.node(value, list.child_path(i))?;
            if item.kind != "listItem" {
                return refuse(at(&item.path, "type"), "a list holds list items only");
            }
            let content = self.blocks(&item, Place::Nested)?;
            if !matches!(content.first(), Some(Block::Paragraph(_))) {
                return refuse(
                    at(&item.child_path(0), "type"),
                    "a list item starts with a paragraph",
                );
            }
            items.push(ListItem(content));
        }
        Ok(items)
    }

    fn code_block(&mut self, node: &Node<'_>) -> Result<Block, Invalid> {
        let language = match node.attr("language") {
            None | Some(Value::Null) => None,
            Some(Value::String(name)) if CODE_LANGUAGES.contains(&name.as_str()) => {
                Some(name.clone())
            }
            Some(_) => return refuse(node.attr_path("language"), "not a listed language"),
        };
        let mut text = String::new();
        for (i, value) in node.content.iter().enumerate() {
            match self.inline(value, node.child_path(i))? {
                Inline::Text { text: part, marks } if marks.is_empty() => text.push_str(&part),
                Inline::Text { .. } => {
                    return refuse(at(&node.child_path(i), "marks"), "code takes no marks");
                }
                Inline::HardBreak => {
                    return refuse(at(&node.child_path(i), "type"), "code holds text only");
                }
            }
        }
        Ok(Block::CodeBlock { language, text })
    }

    fn table(&mut self, table: &Node<'_>) -> Result<Block, Invalid> {
        let mut rows = Vec::new();
        for (r, value) in table.non_empty()?.iter().enumerate() {
            let row = self.node(value, table.child_path(r))?;
            if row.kind != "tableRow" {
                return refuse(at(&row.path, "type"), "a table holds rows only");
            }
            let mut cells = Vec::new();
            for (c, value) in row.non_empty()?.iter().enumerate() {
                cells.push(self.cell(value, row.child_path(c))?);
            }
            rows.push(cells);
        }
        Ok(Block::Table(rows))
    }

    fn cell(&mut self, value: &Value, path: Path) -> Result<Cell, Invalid> {
        let cell = self.node(value, path)?;
        let header = match cell.kind {
            "tableHeader" => true,
            "tableCell" => false,
            _ => return refuse(at(&cell.path, "type"), "a row holds cells only"),
        };
        if cell.attr("colwidth").is_some_and(|width| !width.is_null()) {
            return refuse(
                cell.attr_path("colwidth"),
                "column widths are not supported",
            );
        }
        let span = 1..=u64::from(MAX_CELL_SPAN);
        Ok(Cell {
            header,
            colspan: narrow(cell.integer("colspan", span.clone(), 1)?),
            rowspan: narrow(cell.integer("rowspan", span, 1)?),
            content: self.blocks(&cell, Place::Nested)?,
        })
    }

    fn transcript(&mut self, node: &Node<'_>) -> Result<Block, Invalid> {
        let mut lines = Vec::new();
        for (i, value) in node.non_empty()?.iter().enumerate() {
            let line = self.node(value, node.child_path(i))?;
            if line.kind != "transcriptLine" {
                return refuse(at(&line.path, "type"), "a transcript holds lines only");
            }
            let speaker = label(&line, "speaker", MAX_SPEAKER_CHARS)?;
            lines.push(TranscriptLine {
                speaker,
                content: self.inlines(&line)?,
            });
        }
        Ok(Block::Transcript(lines))
    }

    fn inlines(&mut self, parent: &Node<'_>) -> Result<Vec<Inline>, Invalid> {
        parent
            .content
            .iter()
            .enumerate()
            .map(|(i, child)| self.inline(child, parent.child_path(i)))
            .collect()
    }

    /// A text node or a hard break.
    fn inline(&mut self, value: &Value, path: Path) -> Result<Inline, Invalid> {
        if value.get("type").and_then(Value::as_str) != Some("text") {
            let node = self.node(value, path)?;
            return match node.kind {
                "hardBreak" => Ok(Inline::HardBreak),
                _ => refuse(at(&node.path, "type"), "an unknown inline node"),
            };
        }
        let Some(object) = value.as_object() else {
            return refuse(path, "a node is not an object");
        };
        self.nodes += 1;
        if self.nodes > MAX_NODES {
            return refuse(path, "the body has more than 20,000 nodes");
        }
        if let Some(key) = object
            .keys()
            .find(|key| !matches!(key.as_str(), "type" | "text" | "marks"))
        {
            return refuse(at(&path, key), "a member a text node does not take");
        }
        let text = match object.get("text") {
            Some(Value::String(text)) if !text.is_empty() => text,
            _ => return refuse(at(&path, "text"), "text is not a non-empty string"),
        };
        if !text_is_plain(text) {
            return refuse(at(&path, "text"), "a control character");
        }
        let marks = match object.get("marks") {
            None => Vec::new(),
            Some(Value::Array(marks)) => marks_of(marks, &at(&path, "marks"))?,
            Some(_) => return refuse(at(&path, "marks"), "marks is not an array"),
        };
        Ok(Inline::Text {
            text: text.clone(),
            marks,
        })
    }
}

/// An integer already checked to fit, narrowed. The ranges checked are within the target type.
fn narrow<T: TryFrom<u64> + Default>(value: u64) -> T {
    T::try_from(value).unwrap_or_default()
}

fn image(node: &Node<'_>) -> Result<Block, Invalid> {
    let id = node.string("course_file_id")?;
    let Ok(course_file_id) = Uuid::try_parse(id) else {
        return refuse(node.attr_path("course_file_id"), "not a UUID");
    };
    let alt = node.string("alt")?;
    if !attribute_is_plain(alt) {
        return refuse(node.attr_path("alt"), "a control character");
    }
    let decorative = match node.attr("decorative") {
        None => false,
        Some(Value::Bool(flag)) => *flag,
        Some(_) => return refuse(node.attr_path("decorative"), "not a boolean"),
    };
    if alt.trim().is_empty() && !decorative {
        return refuse(
            node.attr_path("alt"),
            "an image needs alt text unless decorative",
        );
    }
    Ok(Block::Image(Image {
        course_file_id,
        alt: alt.to_owned(),
        decorative,
    }))
}

fn video_embed(node: &Node<'_>) -> Result<Block, Invalid> {
    let provider = node.choice("provider", &VideoProvider::ALL, VideoProvider::as_str)?;
    let video_id = node.string("video_id")?;
    if !provider.id_is_valid(video_id) {
        return refuse(node.attr_path("video_id"), "not the provider's id format");
    }
    Ok(Block::VideoEmbed(VideoEmbed {
        provider,
        video_id: video_id.to_owned(),
        title: label(node, "title", MAX_EMBED_TITLE_CHARS)?,
    }))
}

/// A short label attribute: not blank, at most `max` characters, no control character.
fn label(node: &Node<'_>, name: &str, max: usize) -> Result<String, Invalid> {
    let value = node.string(name)?;
    if value.trim().is_empty() || value.chars().count() > max || !attribute_is_plain(value) {
        return refuse(node.attr_path(name), "not a label of the allowed length");
    }
    Ok(value.to_owned())
}

fn marks_of(values: &[Value], path: &Path) -> Result<Vec<Mark>, Invalid> {
    let mut marks: Vec<Mark> = Vec::new();
    for (i, value) in values.iter().enumerate() {
        let mark = mark(value, at(path, i))?;
        if marks.iter().any(|m| m.name() == mark.name()) {
            return refuse(at(path, i), "a mark given twice");
        }
        marks.push(mark);
    }
    Ok(marks)
}

fn mark(value: &Value, path: Path) -> Result<Mark, Invalid> {
    let Some(object) = value.as_object() else {
        return refuse(path, "a mark is not an object");
    };
    if let Some(key) = object
        .keys()
        .find(|key| !matches!(key.as_str(), "type" | "attrs"))
    {
        return refuse(at(&path, key), "a member a mark does not take");
    }
    let attrs = match object.get("attrs") {
        None => None,
        Some(Value::Object(attrs)) => Some(attrs),
        Some(_) => return refuse(at(&path, "attrs"), "attrs is not an object"),
    };
    let plain = match object.get("type").and_then(Value::as_str) {
        Some("bold") => Mark::Bold,
        Some("italic") => Mark::Italic,
        Some("underline") => Mark::Underline,
        Some("strike") => Mark::Strike,
        Some("code") => Mark::Code,
        Some("subscript") => Mark::Subscript,
        Some("superscript") => Mark::Superscript,
        Some("link") => return link(attrs, &path),
        _ => return refuse(at(&path, "type"), "an unknown mark"),
    };
    if let Some(key) = attrs.and_then(|attrs| attrs.keys().next()) {
        return refuse(
            at(&at(&path, "attrs"), key),
            "an attribute this mark does not take",
        );
    }
    Ok(plain)
}

fn link(attrs: Option<&Map<String, Value>>, path: &Path) -> Result<Mark, Invalid> {
    let attrs_path = at(path, "attrs");
    if let Some(key) = attrs.and_then(|a| a.keys().find(|key| key.as_str() != "href")) {
        return refuse(at(&attrs_path, key), "an attribute a link does not take");
    }
    match attrs.and_then(|a| a.get("href")) {
        Some(Value::String(href)) if href_is_valid(href) => Ok(Mark::Link { href: href.clone() }),
        _ => refuse(at(&attrs_path, "href"), "not an allowed link target"),
    }
}

/// Text content: no control character but tab and newline (B8).
fn text_is_plain(text: &str) -> bool {
    !text
        .chars()
        .any(|c| c.is_control() && c != '\t' && c != '\n')
}

/// An attribute's text: no control character at all.
fn attribute_is_plain(text: &str) -> bool {
    !text.chars().any(char::is_control)
}
