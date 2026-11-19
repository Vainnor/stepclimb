// SPDX-License-Identifier: AGPL-3.0-or-later
//! The renderer (docs/specs/m1/07-pages-and-the-editor.md T4's HTML, T6, T8; B14 to B17).

use std::path::PathBuf;

use proptest::prelude::*;
use serde_json::Value;

use super::*;
use crate::page_doc::{CODE_LANGUAGES, CalloutKind, link_target};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures/page_doc")
}

fn read_json(name: &str) -> Value {
    let text = std::fs::read_to_string(fixtures().join(name)).expect("a fixture");
    serde_json::from_str(&text).expect("JSON")
}

/// `html` equals the committed file `name`, or with `UPDATE_GOLDEN=1` becomes it.
fn golden(name: &str, html: &str) {
    let path = fixtures().join(name);
    if std::env::var_os("UPDATE_GOLDEN").is_some() {
        std::fs::write(&path, html).expect("write the golden file");
    }
    let expected =
        std::fs::read_to_string(&path).expect("the golden file; UPDATE_GOLDEN=1 writes it");
    assert_eq!(html, expected, "{name}");
}

/// The editor's full document (every node, mark and attribute) as a paragraph per stored link
/// target of `link-targets.json`.
fn links_document() -> PageDoc {
    let targets = read_json("link-targets.json");
    let content = targets["stored"]
        .as_array()
        .expect("stored targets")
        .iter()
        .map(|pair| {
            let href = link_target(pair[0].as_str().expect("a target")).expect("a stored target");
            Block::Paragraph(vec![Inline::Text {
                text: "link".into(),
                marks: vec![Mark::Link { href }],
            }])
        })
        .collect();
    PageDoc { content }
}

/// T4: every node and mark renders to the committed HTML, which the sanitizer leaves as it is.
#[test]
fn t4_every_node_and_mark_renders_to_the_golden_html() {
    let doc = PageDoc::parse_stored(&read_json("editor-document.json")).expect("the full document");
    let html = render(&doc);
    assert_eq!(html, write_unsanitized(&doc));
    golden("editor-document.html", &html);
}

/// T4 with the decision on link targets: every stored target renders escaped, and the HTML
/// holds no attribute it did not write.
#[test]
fn t4_every_stored_link_target_renders_escaped() {
    let doc = links_document();
    let html = render(&doc);
    assert_eq!(html, write_unsanitized(&doc));
    assert!(!html.contains("onmouseover=\""), "{html}");
    golden("link-targets.html", &html);
}

/// B9: a link to another origin opens apart, a same-origin path and `mailto:` do not.
#[test]
fn links_to_other_origins_open_apart() {
    let link = |href: &str| PageDoc {
        content: vec![Block::Paragraph(vec![Inline::Text {
            text: "x".into(),
            marks: vec![Mark::Link {
                href: link_target(href).expect("a target"),
            }],
        }])],
    };
    assert_eq!(
        render(&link("https://example.com")),
        "<p><a href=\"https://example.com/\" rel=\"noopener noreferrer\" \
         target=\"_blank\">x</a></p>"
    );
    assert_eq!(render(&link("/c/x")), "<p><a href=\"/c/x\">x</a></p>");
    assert_eq!(
        render(&link("mailto:a@example.com")),
        "<p><a href=\"mailto:a@example.com\">x</a></p>"
    );
}

/// T8 (B12, B16): an embed is the provider's player built from the id, sandboxed, with nothing
/// else from the author but its title.
#[test]
fn t8_an_embed_is_the_providers_player_built_from_the_id() {
    let embed = |provider, id: &str| PageDoc {
        content: vec![Block::VideoEmbed(VideoEmbed {
            provider,
            video_id: id.into(),
            title: "Taxi \"basics\" <1>".into(),
        })],
    };
    let youtube = render(&embed(VideoProvider::Youtube, "dQw4w9WgXcQ"));
    assert_eq!(
        youtube,
        "<iframe data-slot=\"video-embed\" src=\"https://www.youtube-nocookie.com/embed/dQw4w9WgXcQ\" \
         title=\"Taxi &quot;basics&quot; &lt;1&gt;\" \
         sandbox=\"allow-scripts allow-same-origin allow-presentation\" allow=\"fullscreen\" \
         referrerpolicy=\"strict-origin-when-cross-origin\"></iframe>"
    );
    let vimeo = render(&embed(VideoProvider::Vimeo, "76979871"));
    assert!(
        vimeo.contains("src=\"https://player.vimeo.com/video/76979871\""),
        "{vimeo}"
    );
}

/// B14: the allowlist holds no element or attribute that runs code or loads other markup, and
/// none of MathML or SVG (RUSTSEC-2026-0533 needs both).
#[test]
fn the_allowlist_holds_nothing_that_runs_code() {
    for refused in [
        "script", "style", "math", "svg", "object", "embed", "form", "input", "button", "link",
        "meta", "base", "frame", "template", "noscript",
    ] {
        assert!(!ALLOWED_TAGS.contains(&refused), "{refused}");
    }
    for (tag, attributes) in ALLOWED_ATTRIBUTES {
        assert!(ALLOWED_TAGS.contains(tag), "{tag}");
        for attribute in *attributes {
            assert!(!attribute.starts_with("on"), "{tag} {attribute}");
            assert!(
                !["style", "srcdoc", "class"].contains(attribute),
                "{tag} {attribute}"
            );
        }
    }
    let classes: Vec<String> = CODE_LANGUAGES
        .iter()
        .map(|l| format!("language-{l}"))
        .collect();
    assert_eq!(
        LANGUAGE_CLASSES,
        classes.iter().map(String::as_str).collect::<Vec<_>>()
    );
}

/// B14: markup the writer never makes does not survive the sanitizer.
#[test]
fn the_sanitizer_drops_what_the_writer_never_writes() {
    let hostile = "<p onclick=\"x()\" style=\"color:red\" class=\"a\">a</p>\
                   <script>alert(1)</script><svg><g/></svg><math><mi>x</mi></math>\
                   <a href=\"javascript:alert(1)\">j</a><img src=\"https://evil.example/x\" alt=\"i\">\
                   <iframe src=\"https://evil.example/\" srcdoc=\"<b>\"></iframe><!-- c -->\
                   <code class=\"language-rust evil\">c</code>";
    assert_eq!(
        sanitizer().clean(hostile).to_string(),
        "<p>a</p><a>j</a><img alt=\"i\"><iframe></iframe>\
         <code class=\"language-rust\">c</code>"
    );
}

/// B15: each image's placeholder becomes `src` with its URL, escaped; an image without a URL
/// keeps none; nothing else changes.
#[test]
fn fill_images_puts_each_url_in_place_of_its_placeholder() {
    let first = Uuid::from_u128(1);
    let second = Uuid::from_u128(2);
    let doc = PageDoc {
        content: vec![
            Block::Image(Image {
                course_file_id: first,
                alt: "One".into(),
                decorative: false,
            }),
            Block::Paragraph(vec![Inline::Text {
                text: format!("data-sc-image=\"{second}\""),
                marks: Vec::new(),
            }]),
            Block::Image(Image {
                course_file_id: second,
                alt: String::new(),
                decorative: true,
            }),
        ],
    };
    let html = render(&doc);
    assert!(!html.contains("src="), "{html}");
    let urls = HashMap::from([(first, "https://lms.example/files/a&b".to_owned())]);
    assert_eq!(
        fill_images(&html, &urls),
        format!(
            "<img data-slot=\"page-image\" src=\"https://lms.example/files/a&amp;b\" alt=\"One\">\
             <p>data-sc-image=\"{second}\"</p>\
             <img data-slot=\"page-image\" alt=\"\">"
        )
    );
}

// T6: random valid documents.

fn text_strategy() -> impl Strategy<Value = String> {
    "[a-zA-Z0-9 &<>\"'\u{a0}\u{e9}\u{1f6eb}\t\n=/;:-]{1,12}"
}

fn mark_strategy() -> impl Strategy<Value = Vec<Mark>> {
    let targets: Vec<String> = read_json("link-targets.json")["stored"]
        .as_array()
        .expect("stored targets")
        .iter()
        .filter_map(|pair| link_target(pair[0].as_str()?))
        .collect();
    let link = proptest::sample::select(targets).prop_map(|href| Mark::Link { href });
    let one = prop_oneof![
        Just(Mark::Bold),
        Just(Mark::Italic),
        Just(Mark::Underline),
        Just(Mark::Strike),
        Just(Mark::Code),
        Just(Mark::Subscript),
        Just(Mark::Superscript),
        link,
    ];
    proptest::collection::vec(one, 0..3).prop_map(|marks| {
        let mut seen = Vec::new();
        marks
            .into_iter()
            .filter(|mark| {
                let fresh = !seen.contains(&mark.name());
                seen.push(mark.name());
                fresh
            })
            .collect()
    })
}

fn inlines_strategy() -> impl Strategy<Value = Vec<Inline>> {
    let text =
        (text_strategy(), mark_strategy()).prop_map(|(text, marks)| Inline::Text { text, marks });
    proptest::collection::vec(prop_oneof![4 => text, 1 => Just(Inline::HardBreak)], 0..4)
}

fn nested_strategy() -> impl Strategy<Value = Block> {
    let leaf = prop_oneof![
        inlines_strategy().prop_map(Block::Paragraph),
        (
            proptest::option::of(proptest::sample::select(CODE_LANGUAGES)),
            text_strategy()
        )
            .prop_map(|(language, text)| Block::CodeBlock {
                language: language.map(str::to_owned),
                text,
            }),
    ];
    leaf.prop_recursive(2, 12, 3, |inner| {
        let item = (inlines_strategy(), proptest::collection::vec(inner, 0..2)).prop_map(
            |(first, rest)| {
                let mut blocks = vec![Block::Paragraph(first)];
                blocks.extend(rest);
                ListItem(blocks)
            },
        );
        let items = proptest::collection::vec(item, 1..3).boxed();
        prop_oneof![
            items.clone().prop_map(Block::BulletList),
            (0u32..5, items).prop_map(|(start, items)| Block::OrderedList { start, items }),
        ]
    })
}

fn top_strategy() -> impl Strategy<Value = Block> {
    let nested = || proptest::collection::vec(nested_strategy(), 1..3);
    let cell = (any::<bool>(), 1u8..=20, 1u8..=20, nested()).prop_map(
        |(header, colspan, rowspan, content)| Cell {
            header,
            colspan,
            rowspan,
            content,
        },
    );
    let kind = proptest::sample::select(vec![
        CalloutKind::Info,
        CalloutKind::Tip,
        CalloutKind::Warning,
        CalloutKind::Danger,
    ]);
    let embed = prop_oneof![
        "[A-Za-z0-9_-]{11}".prop_map(|id| (VideoProvider::Youtube, id)),
        "[0-9]{1,20}".prop_map(|id| (VideoProvider::Vimeo, id)),
    ];
    prop_oneof![
        nested_strategy(),
        (2u8..=4, inlines_strategy())
            .prop_map(|(level, content)| Block::Heading { level, content }),
        nested().prop_map(Block::Blockquote),
        Just(Block::HorizontalRule),
        proptest::collection::vec(proptest::collection::vec(cell, 1..3), 1..3)
            .prop_map(Block::Table),
        (any::<u128>(), text_strategy(), any::<bool>()).prop_map(|(id, alt, decorative)| {
            Block::Image(Image {
                course_file_id: Uuid::from_u128(id),
                alt: if decorative { String::new() } else { alt },
                decorative,
            })
        }),
        (kind, nested()).prop_map(|(kind, content)| Block::Callout { kind, content }),
        (embed, text_strategy()).prop_map(|((provider, video_id), title)| {
            Block::VideoEmbed(VideoEmbed {
                provider,
                video_id,
                title,
            })
        }),
        proptest::collection::vec(
            ("[A-Za-z <>&\"]{1,10}", inlines_strategy())
                .prop_map(|(speaker, content)| TranscriptLine { speaker, content }),
            1..3,
        )
        .prop_map(Block::Transcript),
    ]
}

proptest! {
    #![proptest_config(ProptestConfig::with_cases(10_000))]

    /// T6 (B14): for any valid document the sanitizer changes nothing in the writer's HTML, so
    /// the writer emits only allowlisted markup, escaped as the serializer escapes it.
    #[test]
    fn t6_the_sanitizer_changes_nothing_the_writer_emits(
        content in proptest::collection::vec(top_strategy(), 1..5)
    ) {
        let doc = PageDoc { content };
        let written = write_unsanitized(&doc);
        prop_assert_eq!(render(&doc), written);
    }
}
