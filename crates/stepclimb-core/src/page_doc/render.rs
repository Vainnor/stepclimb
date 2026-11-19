// SPDX-License-Identifier: AGPL-3.0-or-later
//! A page body as HTML (docs/specs/m1/07-pages-and-the-editor.md B14 to B17).
//!
//! [`render`] writes each node of a validated [`PageDoc`] as fixed elements, with `data-slot`
//! hooks for the reader's styles; text and every attribute value are escaped, and attributes
//! come only from the typed model. The result then passes through `ammonia` with an allowlist
//! of exactly those elements and attributes ([`sanitizer`]), so a bug in the writer cannot let
//! other markup through. Escaping follows the HTML serializer ammonia writes with, so for any
//! valid document the sanitizer changes nothing (T6).
//!
//! An image is written without its URL: `data-sc-image="<course_file_id>"` stands in for the
//! signed URL, which is issued per request and put in by [`fill_images`] (B15), so the HTML a
//! cache keeps never holds a credential. Video embeds point at the provider's privacy-enhanced
//! player, built from the id alone (B16). A code block carries its language as a class; the
//! browser highlights it (B17).

use std::collections::{HashMap, HashSet};
use std::fmt::Write as _;

use uuid::Uuid;

use super::{
    Block, Cell, Image, Inline, ListItem, Mark, PageDoc, TranscriptLine, VideoEmbed, VideoProvider,
};

/// The version of the HTML [`render`] writes. A cached render of an older version is written
/// again; raise it with every change to the output.
pub const RENDERER_VERSION: i32 = 1;

/// The attribute that stands in for an image's URL until [`fill_images`] puts it in.
pub const IMAGE_PLACEHOLDER: &str = "data-sc-image";

/// The `sandbox` of an embedded player (B16).
const EMBED_SANDBOX: &str = "allow-scripts allow-same-origin allow-presentation";

/// `doc` as sanitized HTML, images as placeholders.
#[must_use]
pub fn render(doc: &PageDoc) -> String {
    sanitizer().clean(&write_unsanitized(doc)).to_string()
}

/// [`render`]'s HTML before the sanitizer: what the writer alone produces.
#[must_use]
pub fn write_unsanitized(doc: &PageDoc) -> String {
    let mut out = String::new();
    for block in &doc.content {
        write_block(&mut out, block);
    }
    out
}

/// `html` from [`render`] with each image's placeholder replaced by `src` with its URL in
/// `urls`, by course file id. An image whose id has no URL keeps no `src`. The placeholder can
/// only be the renderer's: text and attribute values are escaped, so `"` never appears in them.
#[must_use]
pub fn fill_images(html: &str, urls: &HashMap<Uuid, String>) -> String {
    let mut out = String::with_capacity(html.len());
    let mut rest = html;
    let marker = format!(" {IMAGE_PLACEHOLDER}=\"");
    while let Some(at) = rest.find(&marker) {
        out.push_str(&rest[..at]);
        let after = &rest[at + marker.len()..];
        let Some(end) = after.find('"') else {
            out.push_str(&rest[at..]);
            return out;
        };
        if let Some(url) = after[..end]
            .parse::<Uuid>()
            .ok()
            .and_then(|id| urls.get(&id))
        {
            out.push_str(" src=\"");
            out.push_str(&escape_attribute(url));
            out.push('"');
        }
        rest = &after[end + 1..];
    }
    out.push_str(rest);
    out
}

/// The sanitizer every rendered page passes through: the writer's elements and attributes and
/// nothing else, `https`, `http` and `mailto` URLs or same-origin paths, no comments.
#[must_use]
pub fn sanitizer() -> ammonia::Builder<'static> {
    let mut builder = ammonia::Builder::empty();
    builder
        .tags(ALLOWED_TAGS.iter().copied().collect())
        .tag_attributes(
            ALLOWED_ATTRIBUTES
                .iter()
                .map(|(tag, attributes)| (*tag, attributes.iter().copied().collect()))
                .collect(),
        )
        .allowed_classes(HashMap::from([(
            "code",
            LANGUAGE_CLASSES.iter().copied().collect::<HashSet<_>>(),
        )]))
        .url_schemes(HashSet::from(["https", "http", "mailto"]))
        .url_relative(ammonia::UrlRelative::PassThrough)
        .link_rel(None)
        .strip_comments(true)
        .attribute_filter(keep_attribute);
    builder
}

/// The sanitizer's check of an attribute's value, for the attributes whose values the writer
/// fixes: an embed's player URL and settings, and a callout's kind. Any other value is dropped.
fn keep_attribute<'a>(
    element: &str,
    attribute: &str,
    value: &'a str,
) -> Option<std::borrow::Cow<'a, str>> {
    let allowed = match (element, attribute) {
        ("iframe", "src") => EMBED_PREFIXES.iter().any(|prefix| {
            value.strip_prefix(prefix).is_some_and(|id| {
                !id.is_empty()
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b == b'_' || b == b'-')
            })
        }),
        ("iframe", "sandbox") => value == EMBED_SANDBOX,
        ("iframe", "allow") => value == "fullscreen",
        ("iframe", "referrerpolicy") => value == "strict-origin-when-cross-origin",
        ("aside", "data-kind") => ["info", "tip", "warning", "danger"].contains(&value),
        _ => true,
    };
    allowed.then_some(std::borrow::Cow::Borrowed(value))
}

/// The start of every player URL [`embed_url`] builds; the id follows.
const EMBED_PREFIXES: [&str; 2] = [
    "https://www.youtube-nocookie.com/embed/",
    "https://player.vimeo.com/video/",
];

/// The class of a code block in each of [`super::CODE_LANGUAGES`], in the same order (a test checks
/// they match): the only classes the sanitizer keeps.
pub const LANGUAGE_CLASSES: &[&str] = &[
    "language-bash",
    "language-c",
    "language-cpp",
    "language-csharp",
    "language-css",
    "language-go",
    "language-html",
    "language-java",
    "language-javascript",
    "language-json",
    "language-kotlin",
    "language-markdown",
    "language-php",
    "language-python",
    "language-ruby",
    "language-rust",
    "language-sql",
    "language-swift",
    "language-typescript",
    "language-xml",
    "language-yaml",
];

/// Every element [`render`] writes.
pub const ALLOWED_TAGS: &[&str] = &[
    "p",
    "h2",
    "h3",
    "h4",
    "ul",
    "ol",
    "li",
    "blockquote",
    "hr",
    "pre",
    "code",
    "table",
    "tbody",
    "tr",
    "th",
    "td",
    "img",
    "aside",
    "iframe",
    "dl",
    "div",
    "dt",
    "dd",
    "br",
    "strong",
    "em",
    "u",
    "s",
    "a",
    "sub",
    "sup",
];

/// Every attribute [`render`] writes, by element. `class` on `code` is allowed through
/// [`sanitizer`]'s class list, [`LANGUAGE_CLASSES`], instead.
pub const ALLOWED_ATTRIBUTES: &[(&str, &[&str])] = &[
    ("ol", &["start"]),
    ("pre", &["data-slot"]),
    ("table", &["data-slot"]),
    ("th", &["colspan", "rowspan"]),
    ("td", &["colspan", "rowspan"]),
    ("img", &["data-slot", IMAGE_PLACEHOLDER, "alt"]),
    ("aside", &["data-slot", "data-kind"]),
    (
        "iframe",
        &[
            "data-slot",
            "src",
            "title",
            "sandbox",
            "allow",
            "referrerpolicy",
        ],
    ),
    ("dl", &["data-slot"]),
    ("div", &["data-slot"]),
    ("a", &["href", "rel", "target"]),
];

fn write_block(out: &mut String, block: &Block) {
    match block {
        Block::Paragraph(content) => wrap(out, "<p>", "</p>", |out| write_inlines(out, content)),
        Block::Heading { level, content } => {
            let _ = write!(out, "<h{level}>");
            write_inlines(out, content);
            let _ = write!(out, "</h{level}>");
        }
        Block::BulletList(items) => wrap(out, "<ul>", "</ul>", |out| write_items(out, items)),
        Block::OrderedList { start, items } => {
            if *start == 1 {
                out.push_str("<ol>");
            } else {
                let _ = write!(out, "<ol start=\"{start}\">");
            }
            write_items(out, items);
            out.push_str("</ol>");
        }
        Block::Blockquote(content) => {
            wrap(out, "<blockquote>", "</blockquote>", |out| {
                write_blocks(out, content)
            });
        }
        Block::HorizontalRule => out.push_str("<hr>"),
        Block::CodeBlock { language, text } => write_code(out, language.as_deref(), text),
        Block::Table(rows) => write_table(out, rows),
        Block::Image(image) => write_image(out, image),
        Block::Callout { kind, content } => {
            let _ = write!(
                out,
                "<aside data-slot=\"callout\" data-kind=\"{}\">",
                kind.as_str()
            );
            write_blocks(out, content);
            out.push_str("</aside>");
        }
        Block::VideoEmbed(embed) => write_embed(out, embed),
        Block::Transcript(lines) => write_transcript(out, lines),
    }
}

fn wrap(out: &mut String, open: &str, close: &str, inner: impl FnOnce(&mut String)) {
    out.push_str(open);
    inner(out);
    out.push_str(close);
}

fn write_blocks(out: &mut String, blocks: &[Block]) {
    for block in blocks {
        write_block(out, block);
    }
}

fn write_items(out: &mut String, items: &[ListItem]) {
    for item in items {
        wrap(out, "<li>", "</li>", |out| write_blocks(out, &item.0));
    }
}

fn write_code(out: &mut String, language: Option<&str>, text: &str) {
    out.push_str("<pre data-slot=\"code-block\">");
    match language {
        Some(language) => {
            let _ = write!(
                out,
                "<code class=\"language-{}\">",
                escape_attribute(language)
            );
        }
        None => out.push_str("<code>"),
    }
    out.push_str(&escape_text(text));
    out.push_str("</code></pre>");
}

fn write_table(out: &mut String, rows: &[Vec<Cell>]) {
    out.push_str("<table data-slot=\"table\"><tbody>");
    for cells in rows {
        out.push_str("<tr>");
        for cell in cells {
            let tag = if cell.header { "th" } else { "td" };
            let _ = write!(out, "<{tag}");
            if cell.colspan != 1 {
                let _ = write!(out, " colspan=\"{}\"", cell.colspan);
            }
            if cell.rowspan != 1 {
                let _ = write!(out, " rowspan=\"{}\"", cell.rowspan);
            }
            out.push('>');
            write_blocks(out, &cell.content);
            let _ = write!(out, "</{tag}>");
        }
        out.push_str("</tr>");
    }
    out.push_str("</tbody></table>");
}

fn write_image(out: &mut String, image: &Image) {
    let _ = write!(
        out,
        "<img data-slot=\"page-image\" {IMAGE_PLACEHOLDER}=\"{}\" alt=\"{}\">",
        image.course_file_id,
        escape_attribute(&image.alt),
    );
}

/// The player URL of `embed`, built from its id alone (B16).
#[must_use]
pub fn embed_url(embed: &VideoEmbed) -> String {
    match embed.provider {
        VideoProvider::Youtube => format!("{}{}", EMBED_PREFIXES[0], embed.video_id),
        VideoProvider::Vimeo => format!("{}{}", EMBED_PREFIXES[1], embed.video_id),
    }
}

fn write_embed(out: &mut String, embed: &VideoEmbed) {
    let _ = write!(
        out,
        "<iframe data-slot=\"video-embed\" src=\"{}\" title=\"{}\" sandbox=\"{EMBED_SANDBOX}\" \
         allow=\"fullscreen\" referrerpolicy=\"strict-origin-when-cross-origin\"></iframe>",
        escape_attribute(&embed_url(embed)),
        escape_attribute(&embed.title),
    );
}

fn write_transcript(out: &mut String, lines: &[TranscriptLine]) {
    out.push_str("<dl data-slot=\"transcript\">");
    for line in lines {
        let _ = write!(
            out,
            "<div data-slot=\"transcript-line\"><dt>{}</dt><dd>",
            escape_text(&line.speaker)
        );
        write_inlines(out, &line.content);
        out.push_str("</dd></div>");
    }
    out.push_str("</dl>");
}

fn write_inlines(out: &mut String, content: &[Inline]) {
    for inline in content {
        match inline {
            Inline::HardBreak => out.push_str("<br>"),
            Inline::Text { text, marks } => {
                for mark in marks {
                    open_mark(out, mark);
                }
                out.push_str(&escape_text(text));
                for mark in marks.iter().rev() {
                    out.push_str(close_mark(mark));
                }
            }
        }
    }
}

fn open_mark(out: &mut String, mark: &Mark) {
    match mark {
        Mark::Link { href } => {
            let _ = write!(out, "<a href=\"{}\"", escape_attribute(href));
            // Same-origin paths and `mailto:` stay in the page; other origins open apart (B9).
            if href.starts_with("https:") || href.starts_with("http:") {
                out.push_str(" rel=\"noopener noreferrer\" target=\"_blank\"");
            }
            out.push('>');
        }
        other => {
            let _ = write!(out, "<{}>", mark_tag(other));
        }
    }
}

fn close_mark(mark: &Mark) -> &'static str {
    match mark {
        Mark::Bold => "</strong>",
        Mark::Italic => "</em>",
        Mark::Underline => "</u>",
        Mark::Strike => "</s>",
        Mark::Code => "</code>",
        Mark::Link { .. } => "</a>",
        Mark::Subscript => "</sub>",
        Mark::Superscript => "</sup>",
    }
}

fn mark_tag(mark: &Mark) -> &'static str {
    match mark {
        Mark::Bold => "strong",
        Mark::Italic => "em",
        Mark::Underline => "u",
        Mark::Strike => "s",
        Mark::Code | Mark::Link { .. } => "code",
        Mark::Subscript => "sub",
        Mark::Superscript => "sup",
    }
}

/// `text` for an element's content, escaped as the HTML serializer ammonia uses escapes it.
fn escape_text(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            other => out.push(other),
        }
    }
    out
}

/// `value` for a double-quoted attribute, escaped as that serializer escapes it.
fn escape_attribute(value: &str) -> String {
    let mut out = String::with_capacity(value.len());
    for c in value.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '\u{a0}' => out.push_str("&nbsp;"),
            other => out.push(other),
        }
    }
    out
}

#[cfg(test)]
mod tests;
