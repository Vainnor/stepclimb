// SPDX-License-Identifier: AGPL-3.0-or-later
//! The page document schema (docs/specs/m1/07-pages-and-the-editor.md T4, T5 for validation).

use serde_json::{Value, json};

use super::*;

/// A body with every node, mark and attribute M1 allows, written as `to_value` writes it.
fn everything() -> Value {
    let text = |t: &str| json!({ "type": "text", "text": t });
    let para = |t: &str| json!({ "type": "paragraph", "content": [text(t)] });
    let marked = |mark: Value| json!({ "type": "text", "text": "m", "marks": [mark] });
    json!({ "type": "doc", "content": [
        { "type": "paragraph" },
        { "type": "paragraph", "content": [
            text("plain\ttab\nnewline"),
            { "type": "hardBreak" },
            marked(json!({ "type": "bold" })),
            marked(json!({ "type": "italic" })),
            marked(json!({ "type": "underline" })),
            marked(json!({ "type": "strike" })),
            marked(json!({ "type": "code" })),
            marked(json!({ "type": "subscript" })),
            marked(json!({ "type": "superscript" })),
            marked(json!({ "type": "link", "attrs": { "href": "https://example.com/" } })),
            { "type": "text", "text": "two", "marks": [{ "type": "bold" }, { "type": "italic" }] },
        ] },
        { "type": "heading", "attrs": { "level": 2 }, "content": [text("Two")] },
        { "type": "heading", "attrs": { "level": 3 }, "content": [text("Three")] },
        { "type": "heading", "attrs": { "level": 4 } },
        { "type": "bulletList", "content": [
            { "type": "listItem", "content": [
                para("item"),
                { "type": "orderedList", "attrs": { "start": 1 }, "content": [
                    { "type": "listItem", "content": [para("inner")] },
                ] },
                { "type": "codeBlock", "attrs": { "language": null }, "content": [text("x")] },
            ] },
        ] },
        { "type": "orderedList", "attrs": { "start": 3 }, "content": [
            { "type": "listItem", "content": [para("third")] },
        ] },
        { "type": "blockquote", "content": [para("quoted")] },
        { "type": "horizontalRule" },
        { "type": "codeBlock", "attrs": { "language": "rust" }, "content": [text("fn main() {}\n")] },
        { "type": "codeBlock", "attrs": { "language": null } },
        { "type": "table", "content": [
            { "type": "tableRow", "content": [
                { "type": "tableHeader", "attrs": { "colspan": 2, "rowspan": 1, "colwidth": null }, "content": [para("h")] },
            ] },
            { "type": "tableRow", "content": [
                { "type": "tableCell", "attrs": { "colspan": 1, "rowspan": 20, "colwidth": null }, "content": [para("a")] },
                { "type": "tableCell", "attrs": { "colspan": 1, "rowspan": 1, "colwidth": null }, "content": [
                    { "type": "bulletList", "content": [{ "type": "listItem", "content": [para("b")] }] },
                ] },
            ] },
        ] },
        { "type": "image", "attrs": {
            "course_file_id": "0190f000-0000-7000-8000-000000000001", "alt": "A runway diagram",
            "decorative": false,
        } },
        { "type": "image", "attrs": {
            "course_file_id": "0190f000-0000-7000-8000-000000000002", "alt": "", "decorative": true,
        } },
        { "type": "callout", "attrs": { "kind": "info" }, "content": [para("i")] },
        { "type": "callout", "attrs": { "kind": "tip" }, "content": [para("t")] },
        { "type": "callout", "attrs": { "kind": "warning" }, "content": [para("w")] },
        { "type": "callout", "attrs": { "kind": "danger" }, "content": [
            para("d"),
            { "type": "codeBlock", "attrs": { "language": "bash" }, "content": [text("ls")] },
        ] },
        { "type": "videoEmbed", "attrs": { "provider": "youtube", "video_id": "dQw4w9WgXcQ", "title": "Intro" } },
        { "type": "videoEmbed", "attrs": { "provider": "vimeo", "video_id": "76979871", "title": "Taxi" } },
        { "type": "transcript", "content": [
            { "type": "transcriptLine", "attrs": { "speaker": "Speaker A" }, "content": [text("Hello")] },
            { "type": "transcriptLine", "attrs": { "speaker": "B" }, "content": [
                marked(json!({ "type": "bold" })), { "type": "hardBreak" },
            ] },
            { "type": "transcriptLine", "attrs": { "speaker": "C" } },
        ] },
    ] })
}

#[test]
fn every_allowed_node_and_mark_parses_and_writes_back_unchanged() {
    let body = everything();
    let doc = PageDoc::parse(&body).expect("the full schema parses");
    assert_eq!(doc.to_value(), body);
    assert_eq!(PageDoc::parse(&doc.to_value()), Ok(doc));
}

#[test]
fn omitted_optional_attributes_take_their_defaults() {
    let body = json!({ "type": "doc", "content": [
        { "type": "orderedList", "content": [
            { "type": "listItem", "content": [{ "type": "paragraph", "attrs": {} }] },
        ] },
        { "type": "codeBlock", "content": [
            { "type": "text", "text": "a" }, { "type": "text", "text": "b", "marks": [] },
        ] },
        { "type": "table", "content": [{ "type": "tableRow", "content": [
            { "type": "tableCell", "content": [{ "type": "paragraph" }] },
        ] }] },
        { "type": "image", "attrs": { "course_file_id": "0190f000-0000-7000-8000-000000000001", "alt": "x" } },
    ] });
    let doc = PageDoc::parse(&body).expect("defaults apply");
    assert_eq!(
        doc.to_value(),
        json!({ "type": "doc", "content": [
            { "type": "orderedList", "attrs": { "start": 1 }, "content": [
                { "type": "listItem", "content": [{ "type": "paragraph" }] },
            ] },
            { "type": "codeBlock", "attrs": { "language": null }, "content": [
                { "type": "text", "text": "ab" },
            ] },
            { "type": "table", "content": [{ "type": "tableRow", "content": [
                { "type": "tableCell", "attrs": { "colspan": 1, "rowspan": 1, "colwidth": null },
                  "content": [{ "type": "paragraph" }] },
            ] }] },
            { "type": "image", "attrs": {
                "course_file_id": "0190f000-0000-7000-8000-000000000001", "alt": "x",
                "decorative": false,
            } },
        ] })
    );
}

#[test]
fn the_empty_document_is_one_empty_paragraph() {
    let empty = PageDoc::empty();
    assert_eq!(
        empty.to_value(),
        json!({ "type": "doc", "content": [{ "type": "paragraph" }] })
    );
    assert_eq!(PageDoc::parse(&empty.to_value()), Ok(empty));
}

/// The pointer `body` is refused at.
fn refused_at(body: &Value) -> String {
    match PageDoc::parse(body) {
        Ok(doc) => panic!("accepted {body}: {doc:?}"),
        Err(invalid) => invalid.pointer(),
    }
}

/// A document of these top-level blocks.
fn doc(content: Value) -> Value {
    json!({ "type": "doc", "content": content })
}

fn para(content: Value) -> Value {
    json!({ "type": "paragraph", "content": content })
}

fn text(t: &str) -> Value {
    json!({ "type": "text", "text": t })
}

#[test]
fn the_specs_refusals_name_the_first_offense() {
    let cases = [
        // T4
        (doc(json!([{ "type": "script" }])), "/content/0/type"),
        (
            doc(json!([{ "type": "paragraph", "attrs": { "onclick": "alert(1)" } }])),
            "/content/0/attrs/onclick",
        ),
        (
            doc(json!([{ "type": "heading", "attrs": { "level": 1 } }])),
            "/content/0/attrs/level",
        ),
        (
            doc(
                json!([{ "type": "table", "content": [{ "type": "tableRow", "content": [
                { "type": "tableCell", "attrs": { "colspan": 21 }, "content": [para(json!([]))] },
            ] }] }]),
            ),
            "/content/0/content/0/content/0/attrs/colspan",
        ),
        (
            doc(json!([para(json!([text("ok"), text("nul\u{0}")]))])),
            "/content/0/content/1/text",
        ),
        // B13: no question node in M1.
        (doc(json!([{ "type": "question" }])), "/content/0/type"),
    ];
    for (body, pointer) in cases {
        assert_eq!(refused_at(&body), pointer, "{body}");
    }
}

#[test]
fn a_body_whose_stored_form_is_over_one_mebibyte_is_refused_at_its_root() {
    let with = |n: usize| doc(json!([para(json!([text(&"x".repeat(n))]))]));
    let overhead = serde_json::to_vec(&with(1)).unwrap().len() - 1;
    let largest = with(MAX_BODY_BYTES - overhead);
    assert_eq!(serde_json::to_vec(&largest).unwrap().len(), MAX_BODY_BYTES);
    let parsed = PageDoc::parse_new(&largest).expect("1 MiB stored is the limit");
    assert_eq!(parsed.stored_size(), MAX_BODY_BYTES);
    let over = with(MAX_BODY_BYTES - overhead + 1);
    assert_eq!(PageDoc::parse_new(&over).unwrap_err().pointer(), "");
    // A stored body is not measured again.
    assert!(PageDoc::parse(&over).is_ok());
}

/// The review's case: 9,900 table cells without attributes and a paragraph of padding, under
/// 1 MiB as submitted, grow past it once every cell's attributes are written out.
fn cells_that_grow_past_the_limit() -> Value {
    let cell = json!({ "type": "tableCell", "content": [{ "type": "paragraph" }] });
    let row = json!({ "type": "tableRow", "content": vec![cell; 9_900] });
    let table = json!({ "type": "table", "content": [row] });
    let bare = doc(json!([table.clone(), para(json!([text("x")]))]));
    let pad = MAX_BODY_BYTES - serde_json::to_vec(&bare).unwrap().len() - 16;
    doc(json!([table, para(json!([text(&"x".repeat(pad + 1))]))]))
}

#[test]
fn the_limit_is_on_the_stored_form_not_the_submitted_one() {
    let body = cells_that_grow_past_the_limit();
    let submitted = serde_json::to_vec(&body).unwrap().len();
    assert!(submitted < MAX_BODY_BYTES, "{submitted}");
    let parsed = PageDoc::parse(&body).expect("the structure is allowed");
    assert!(
        parsed.stored_size() > MAX_BODY_BYTES,
        "{}",
        parsed.stored_size()
    );
    assert_eq!(PageDoc::parse_new(&body).unwrap_err().pointer(), "");
}

#[test]
fn deep_nesting_parses_without_overflowing() {
    // 200 nested bullet lists, built in code: deeper than serde_json lets a request carry.
    let mut list = json!({ "type": "bulletList", "content": [
        { "type": "listItem", "content": [para(json!([text("bottom")]))] },
    ] });
    for _ in 0..199 {
        list = json!({ "type": "bulletList", "content": [
            { "type": "listItem", "content": [para(json!([text("level")])), list] },
        ] });
    }
    let body = doc(json!([list]));
    let parsed = PageDoc::parse(&body).expect("nesting depth is not a rule of its own");
    assert_eq!(parsed.to_value(), body);
}

#[test]
fn a_body_over_20000_nodes_is_refused_at_the_first_node_past_it() {
    // The document, the paragraph, then text nodes: 20,000 nodes in all.
    let texts = |n: usize| Value::Array((0..n).map(|_| text("x")).collect());
    assert!(PageDoc::parse(&doc(json!([para(texts(MAX_NODES - 2))]))).is_ok());
    let over = doc(json!([para(texts(MAX_NODES - 1))]));
    assert_eq!(
        refused_at(&over),
        format!("/content/0/content/{}", MAX_NODES - 2)
    );
}

#[test]
fn members_and_shapes_the_schema_does_not_list_are_refused() {
    let image = |attrs: Value| doc(json!([{ "type": "image", "attrs": attrs }]));
    let id = "0190f000-0000-7000-8000-000000000001";
    let cases = [
        (json!([]), ""),
        (json!({ "type": "paragraph" }), "/type"),
        (json!({ "type": "doc" }), "/content"),
        (json!({ "type": "doc", "content": [] }), "/content"),
        (json!({ "type": "doc", "content": {} }), "/content"),
        (
            json!({ "type": "doc", "content": [{}], "extra": 1 }),
            "/extra",
        ),
        (doc(json!([{}])), "/content/0/type"),
        (
            doc(json!([{ "type": "script", "attrs": { "src": "https://evil.example/x.js" } }])),
            "/content/0/type",
        ),
        (doc(json!(["text"])), "/content/0"),
        (doc(json!([text("loose")])), "/content/0/type"),
        (
            doc(json!([{ "type": "paragraph", "attrs": [] }])),
            "/content/0/attrs",
        ),
        (
            doc(json!([{ "type": "paragraph", "marks": [] }])),
            "/content/0/marks",
        ),
        (
            doc(json!([para(json!([{ "type": "text", "text": "" }]))])),
            "/content/0/content/0/text",
        ),
        (
            doc(json!([para(json!([{ "type": "text", "text": 1 }]))])),
            "/content/0/content/0/text",
        ),
        (
            doc(json!([para(json!([{ "type": "text", "text": "a\r" }]))])),
            "/content/0/content/0/text",
        ),
        (
            doc(json!([para(
                json!([{ "type": "text", "text": "a\u{85}" }])
            )])),
            "/content/0/content/0/text",
        ),
        (
            doc(json!([para(json!([{ "type": "image" }]))])),
            "/content/0/content/0/type",
        ),
        (
            doc(json!([{ "type": "heading", "content": [] }])),
            "/content/0/attrs/level",
        ),
        (
            doc(json!([{ "type": "heading", "attrs": { "level": 5 } }])),
            "/content/0/attrs/level",
        ),
        (
            doc(json!([{ "type": "heading", "attrs": { "level": 2.0 } }])),
            "/content/0/attrs/level",
        ),
        (
            doc(json!([{ "type": "heading", "attrs": { "level": "2" } }])),
            "/content/0/attrs/level",
        ),
        (
            doc(json!([{ "type": "horizontalRule", "content": [] }])),
            "/content/0/content",
        ),
        (
            doc(json!([{ "type": "bulletList", "content": [para(json!([]))] }])),
            "/content/0/content/0/type",
        ),
        (
            doc(json!([{ "type": "bulletList", "content": [] }])),
            "/content/0/content",
        ),
        (
            doc(
                json!([{ "type": "bulletList", "content": [{ "type": "listItem", "content": [
                { "type": "bulletList", "content": [{ "type": "listItem", "content": [para(json!([]))] }] },
            ] }] }]),
            ),
            "/content/0/content/0/content/0/type",
        ),
        (
            doc(json!([{ "type": "orderedList", "attrs": { "start": -1 }, "content": [] }])),
            "/content/0/attrs/start",
        ),
        (
            doc(json!([{ "type": "codeBlock", "attrs": { "language": "brainfuck" } }])),
            "/content/0/attrs/language",
        ),
        (
            doc(
                json!([{ "type": "codeBlock", "content": [{ "type": "text", "text": "a", "marks": [{ "type": "bold" }] }] }]),
            ),
            "/content/0/content/0/marks",
        ),
        (
            doc(json!([{ "type": "codeBlock", "content": [{ "type": "hardBreak" }] }])),
            "/content/0/content/0/type",
        ),
        (
            doc(
                json!([{ "type": "callout", "attrs": { "kind": "note" }, "content": [para(json!([]))] }]),
            ),
            "/content/0/attrs/kind",
        ),
        (
            doc(
                json!([{ "type": "callout", "attrs": { "kind": "info" }, "content": [
                { "type": "callout", "attrs": { "kind": "info" }, "content": [para(json!([]))] },
            ] }]),
            ),
            "/content/0/content/0/type",
        ),
        (
            doc(json!([{ "type": "blockquote", "content": [
                { "type": "image", "attrs": { "course_file_id": id, "alt": "x" } },
            ] }])),
            "/content/0/content/0/type",
        ),
        (
            doc(
                json!([{ "type": "table", "content": [{ "type": "tableRow", "content": [
                { "type": "tableCell", "attrs": { "colwidth": [100] }, "content": [para(json!([]))] },
            ] }] }]),
            ),
            "/content/0/content/0/content/0/attrs/colwidth",
        ),
        (
            doc(
                json!([{ "type": "table", "content": [{ "type": "tableRow", "content": [
                { "type": "tableCell", "attrs": { "rowspan": 0 }, "content": [para(json!([]))] },
            ] }] }]),
            ),
            "/content/0/content/0/content/0/attrs/rowspan",
        ),
        (
            doc(
                json!([{ "type": "table", "content": [{ "type": "tableRow", "content": [para(json!([]))] }] }]),
            ),
            "/content/0/content/0/content/0/type",
        ),
        (
            image(json!({ "alt": "x" })),
            "/content/0/attrs/course_file_id",
        ),
        (
            image(json!({ "course_file_id": "nope", "alt": "x" })),
            "/content/0/attrs/course_file_id",
        ),
        (
            image(json!({ "course_file_id": id })),
            "/content/0/attrs/alt",
        ),
        (
            image(json!({ "course_file_id": id, "alt": "" })),
            "/content/0/attrs/alt",
        ),
        (
            image(json!({ "course_file_id": id, "alt": "  " })),
            "/content/0/attrs/alt",
        ),
        (
            image(json!({ "course_file_id": id, "alt": "a\nb" })),
            "/content/0/attrs/alt",
        ),
        (
            image(json!({ "course_file_id": id, "alt": "x", "decorative": "yes" })),
            "/content/0/attrs/decorative",
        ),
        (
            image(json!({ "course_file_id": id, "alt": "x", "src": "https://evil.example/x.png" })),
            "/content/0/attrs/src",
        ),
        (
            doc(json!([{ "type": "transcript", "content": [para(json!([]))] }])),
            "/content/0/content/0/type",
        ),
        (
            doc(json!([{ "type": "transcript", "content": [
                { "type": "transcriptLine", "attrs": { "speaker": "x".repeat(41) } },
            ] }])),
            "/content/0/content/0/attrs/speaker",
        ),
        (
            doc(json!([{ "type": "transcript", "content": [
                { "type": "transcriptLine", "attrs": { "speaker": " " } },
            ] }])),
            "/content/0/content/0/attrs/speaker",
        ),
        (
            doc(json!([para(
                json!([{ "type": "text", "text": "a", "marks": [{ "type": "bold" }, { "type": "bold" }] }])
            )])),
            "/content/0/content/0/marks/1",
        ),
        (
            doc(json!([para(
                json!([{ "type": "text", "text": "a", "marks": [{ "type": "highlight" }] }])
            )])),
            "/content/0/content/0/marks/0/type",
        ),
        (
            doc(json!([para(
                json!([{ "type": "text", "text": "a", "marks": [{ "type": "bold", "attrs": { "x": 1 } }] }])
            )])),
            "/content/0/content/0/marks/0/attrs/x",
        ),
        (
            doc(json!([para(
                json!([{ "type": "text", "text": "a", "marks": [{ "type": "link", "attrs": { "href": "/c/x", "target": "_self" } }] }])
            )])),
            "/content/0/content/0/marks/0/attrs/target",
        ),
    ];
    for (body, pointer) in cases {
        assert_eq!(refused_at(&body), pointer, "{body}");
    }
}

#[test]
fn a_video_id_must_match_its_providers_format() {
    let embed = |provider: &str, id: &str| {
        doc(
            json!([{ "type": "videoEmbed", "attrs": { "provider": provider, "video_id": id, "title": "V" } }]),
        )
    };
    // T8: twelve characters is not a YouTube id.
    for (provider, id) in [
        ("youtube", "dQw4w9WgXcQx"),
        ("youtube", "dQw4w9WgXc"),
        ("youtube", "dQw4w9WgX/Q"),
        ("vimeo", "12a"),
        ("vimeo", ""),
        ("vimeo", "123456789012345678901"),
    ] {
        assert_eq!(
            refused_at(&embed(provider, id)),
            "/content/0/attrs/video_id",
            "{provider} {id}"
        );
    }
    assert_eq!(
        refused_at(&embed("dailymotion", "x")),
        "/content/0/attrs/provider"
    );
    let untitled = doc(
        json!([{ "type": "videoEmbed", "attrs": { "provider": "vimeo", "video_id": "1", "title": "" } }]),
    );
    assert_eq!(refused_at(&untitled), "/content/0/attrs/title");
    let with_url = doc(json!([{ "type": "videoEmbed", "attrs": {
        "provider": "youtube", "video_id": "dQw4w9WgXcQ", "title": "V", "src": "https://youtube.com",
    } }]));
    assert_eq!(refused_at(&with_url), "/content/0/attrs/src");
}

#[test]
fn links_take_only_the_allowed_targets() {
    // T5
    for refused in [
        "javascript:alert(1)",
        "JavaScript:alert(1)",
        "data:text/html,<script>alert(1)</script>",
        "//evil.example",
        "/\\evil.example",
        "/a/..//evil.example",
        "/\t/evil.example",
        "vbscript:x",
        "ftp://example.com/x",
        "https://user:pass@example.com",
        "https://",
        "mailto:",
        "mailto:a@example.com\nbcc:b@example.com",
        "relative/path",
        "",
        "/\"><svg/onload=alert(1)>",
        "/a'b",
        "/a<b",
        "/a>b",
        "/a`b",
    ] {
        assert_eq!(link_target(refused), None, "{refused:?}");
        let body = doc(json!([para(
            json!([{ "type": "text", "text": "a", "marks": [
            { "type": "link", "attrs": { "href": refused } },
        ] }])
        )]));
        assert_eq!(refused_at(&body), "/content/0/content/0/marks/0/attrs/href");
    }
    for (given, stored) in [
        ("https://example.com", "https://example.com/"),
        ("http://example.com/a?b=c#d", "http://example.com/a?b=c#d"),
        ("HTTPS://EXAMPLE.COM", "https://example.com/"),
        (
            "https://a.example/\"onmouseover=alert(1)//",
            "https://a.example/%22onmouseover=alert(1)//",
        ),
        ("https://a.example/<b>", "https://a.example/%3Cb%3E"),
        ("mailto:a@example.com", "mailto:a@example.com"),
        ("MAILTO:a@example.com", "mailto:a@example.com"),
        ("/c/x", "/c/x"),
        ("/", "/"),
    ] {
        assert_eq!(link_target(given).as_deref(), Some(stored), "{given:?}");
        let body = doc(json!([para(
            json!([{ "type": "text", "text": "a", "marks": [
            { "type": "link", "attrs": { "href": given } },
        ] }])
        )]));
        let saved = PageDoc::parse(&body).expect("an allowed link").to_value();
        assert_eq!(
            saved["content"][0]["content"][0]["marks"][0]["attrs"]["href"],
            stored
        );
        // The stored form parses back to itself.
        assert_eq!(PageDoc::parse(&saved).expect("stored").to_value(), saved);
    }
    let missing = doc(json!([para(
        json!([{ "type": "text", "text": "a", "marks": [{ "type": "link" }] }])
    )]));
    assert_eq!(
        refused_at(&missing),
        "/content/0/content/0/marks/0/attrs/href"
    );
}

#[test]
fn images_are_listed_with_the_path_of_their_file_id() {
    let doc = PageDoc::parse(&everything()).unwrap();
    let images = doc.images();
    let pointers: Vec<(String, String)> = images
        .iter()
        .map(|(id, path)| {
            let pointer = Invalid {
                path: path.clone(),
                reason: "",
            }
            .pointer();
            (id.to_string(), pointer)
        })
        .collect();
    assert_eq!(
        pointers,
        [
            (
                "0190f000-0000-7000-8000-000000000001".to_owned(),
                "/content/12/attrs/course_file_id".to_owned()
            ),
            (
                "0190f000-0000-7000-8000-000000000002".to_owned(),
                "/content/13/attrs/course_file_id".to_owned()
            ),
        ]
    );
    let body = everything();
    for (_, path) in &images {
        let pointer: String = path.iter().map(|s| format!("/{s}")).collect();
        assert!(
            body.pointer(&pointer).is_some_and(Value::is_string),
            "{pointer}"
        );
    }
}

#[test]
fn a_pointer_escapes_tilde_and_slash() {
    let invalid = Invalid {
        path: vec!["content".into(), "a/b~c".into()],
        reason: "",
    };
    assert_eq!(invalid.pointer(), "/content/a~1b~0c");
}
