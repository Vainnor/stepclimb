// SPDX-License-Identifier: AGPL-3.0-or-later
//! The page editor's side of the page document model (docs/specs/m1/07-pages-and-the-editor.md
//! B7 to B9, B21). The JSON files in `fixtures/page_doc/` are shared with the editor's Vitest
//! tests in `packages/ui/src/editor/`: the editor loads `editor-document.json` and gives it back
//! unchanged, writes `editor-inserted.json` from every block its slash menu inserts, and checks
//! `link-targets.json` with its own link check. Here the server parses the same files, so a
//! change on either side that the other does not follow fails a test.

use serde_json::Value;
use stepclimb_core::page_doc::{PageDoc, link_target};

fn fixture(name: &str) -> Value {
    let path = format!(
        "{}/tests/fixtures/page_doc/{name}",
        env!("CARGO_MANIFEST_DIR")
    );
    let text = std::fs::read_to_string(&path).expect("the fixture exists");
    serde_json::from_str(&text).expect("the fixture is JSON")
}

/// The body saves, and its stored form is the body itself: the editor writes the canonical
/// form, every attribute spelled out and nothing the model does not hold.
fn assert_canonical(name: &str) {
    let body = fixture(name);
    let doc = PageDoc::parse_new(&body).unwrap_or_else(|invalid| panic!("{name}: {invalid}"));
    assert_eq!(doc.to_value(), body, "{name} is not in its stored form");
}

#[test]
fn the_editors_full_document_is_stored_as_written() {
    assert_canonical("editor-document.json");
}

#[test]
fn every_block_the_editor_inserts_is_stored_as_written() {
    assert_canonical("editor-inserted.json");
}

#[test]
fn the_editor_and_the_server_agree_on_link_targets() {
    let cases = fixture("link-targets.json");
    let refused = cases["refused"]
        .as_array()
        .expect("a list of refused targets");
    assert!(!refused.is_empty());
    for href in refused {
        let href = href.as_str().expect("a string");
        assert_eq!(link_target(href), None, "{href:?} should be refused");
    }
    let stored = cases["stored"]
        .as_array()
        .expect("a list of stored targets");
    assert!(!stored.is_empty());
    for pair in stored {
        let given = pair[0].as_str().expect("the given target");
        let expected = pair[1].as_str().expect("the stored target");
        assert_eq!(link_target(given).as_deref(), Some(expected), "{given:?}");
    }
}
