// SPDX-License-Identifier: AGPL-3.0-or-later
//! No `Deserialize` type of the server uses a serde attribute that buffers its content.
//!
//! `ApiJson` refuses an array for a struct by wrapping the deserializer (`src/extract/
//! maps_only.rs`). serde reads `flatten` fields and untagged, internally tagged and adjacently
//! tagged enums into a buffer first and replays it through its own deserializer, which the
//! wrapper cannot reach, so a struct in them would take an array again. This test reads every
//! source file of the crate and fails, naming the type, on any such attribute on a type that
//! derives `Deserialize`. A type that needs one has to bring its own check of the array case.

use std::{fs, path::Path};

/// The serde attribute arguments that buffer content.
const BUFFERED: [&str; 4] = ["flatten", "untagged", "tag", "content"];

#[test]
fn no_deserialize_type_of_the_server_buffers_its_content() {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut found = Vec::new();
    for file in rust_files(&src) {
        let source = fs::read_to_string(&file).expect("a readable source file");
        for name in buffered_deserialize_types(&source) {
            found.push(format!("{}: {name}", file.display()));
        }
    }
    assert!(found.is_empty(), "{found:#?}");
}

/// Every `.rs` file under `dir`.
fn rust_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    for entry in fs::read_dir(dir).expect("a readable directory") {
        let path = entry.expect("a directory entry").path();
        if path.is_dir() {
            files.extend(rust_files(&path));
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            files.push(path);
        }
    }
    files
}

/// The names of the structs and enums in `source` that derive `Deserialize` and carry a
/// [`BUFFERED`] serde argument on the type, a field or a variant.
fn buffered_deserialize_types(source: &str) -> Vec<String> {
    let lines: Vec<&str> = source
        .lines()
        .filter(|line| !line.trim_start().starts_with("//"))
        .collect();
    let mut found = Vec::new();
    for (start, line) in lines.iter().enumerate() {
        let Some(name) = item_name(line) else {
            continue;
        };
        let item = attributes_above(&lines, start) + &body_from(&lines, start);
        if derives_deserialize(&item) && serde_arguments(&item).iter().any(|a| buffers(a)) {
            found.push(name.to_owned());
        }
    }
    found
}

/// The name of the struct or enum `line` declares.
fn item_name(line: &str) -> Option<&str> {
    let mut words = line.split_whitespace().skip_while(|w| w.starts_with("pub"));
    match words.next() {
        Some("struct" | "enum") => words
            .next()
            .map(|name| name.split(['<', '(', '{', ';']).next().unwrap_or(name)),
        _ => None,
    }
}

/// The attribute lines right above the item at `start`, joined: back to a blank line or the
/// end of the item before.
fn attributes_above(lines: &[&str], start: usize) -> String {
    let ends_an_item = |line: &str| {
        let line = line.trim();
        line.is_empty() || line.ends_with(['{', '}', ';'])
    };
    let first = lines[..start]
        .iter()
        .rposition(|line| ends_an_item(line))
        .map_or(0, |i| i + 1);
    lines[first..start].join("\n")
}

/// The item at `start`, from its declaration to its closing brace or semicolon.
fn body_from(lines: &[&str], start: usize) -> String {
    let mut depth = 0i32;
    let mut end = start;
    for (i, line) in lines.iter().enumerate().skip(start) {
        depth += line.matches('{').count() as i32 - line.matches('}').count() as i32;
        end = i;
        if depth <= 0 && (line.contains('}') || line.trim_end().ends_with(';')) {
            break;
        }
    }
    lines[start..=end].join("\n")
}

fn derives_deserialize(item: &str) -> bool {
    item.split("#[derive(").skip(1).any(|rest| {
        rest.split(")]")
            .next()
            .is_some_and(|d| d.contains("Deserialize"))
    })
}

/// The arguments of every `#[serde(...)]` in `item`.
fn serde_arguments(item: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    for rest in item.split("#[serde(").skip(1) {
        let mut depth = 1;
        let inside: String = rest
            .chars()
            .take_while(|c| {
                depth += match c {
                    '(' => 1,
                    ')' => -1,
                    _ => 0,
                };
                depth > 0
            })
            .collect();
        arguments.extend(inside.split(',').map(|a| a.trim().to_owned()));
    }
    arguments
}

/// `argument` (`flatten`, `tag = "type"`) is one of [`BUFFERED`].
fn buffers(argument: &str) -> bool {
    let key = argument.split('=').next().unwrap_or(argument).trim();
    BUFFERED.contains(&key)
}

#[test]
fn the_scan_finds_buffering_attributes_on_deserialize_types_only() {
    let source = r#"
/// Flattened.
#[derive(Deserialize)]
pub struct Flat {
    #[serde(flatten)]
    rest: Inner,
}

#[derive(Serialize, ToSchema)]
pub struct Shown {
    #[serde(flatten)]
    problem: Problem,
}

#[derive(Debug, Deserialize)]
#[serde(
    rename_all = "lowercase",
    tag = "type"
)]
enum Internal {
    A,
}

#[derive(Deserialize)]
#[serde(tag = "t", content = "c")]
enum Adjacent<T> {
    A(T),
}

#[derive(Deserialize)]
enum Variant {
    #[serde(untagged)]
    Other(String),
}

#[derive(Clone, Deserialize)]
#[serde(deny_unknown_fields, rename_all = "kebab-case")]
pub struct Plain {
    #[serde(default, rename = "type")]
    kind: Option<String>,
}

#[derive(Deserialize)]
pub struct Tuple(#[serde(flatten)] Inner);
"#;
    assert_eq!(
        buffered_deserialize_types(source),
        ["Flat", "Internal", "Adjacent", "Variant", "Tuple"],
    );
}
