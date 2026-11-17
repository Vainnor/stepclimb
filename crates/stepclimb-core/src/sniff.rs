// SPDX-License-Identifier: AGPL-3.0-or-later
//! The type of an uploaded file, read from its first bytes (docs/specs/m1/06-files.md B6, Q3).
//!
//! [`sniff`] looks at the first [`HEAD_BYTES`] of a file and matches a fixed list of signatures:
//! PDF, PNG, JPEG, GIF, WebP, MP4, WebM, MP3, Ogg, the ZIP-based office formats (Office Open
//! XML and OpenDocument) and plain UTF-8 text. Anything else is [`OCTET_STREAM`]. The client's
//! `Content-Type` and the file's name are never read: the stored type decides the headers a
//! file is served with, so it must not be one the uploader chose.
//!
//! Text whose first character other than white space or a byte order mark is `<` is not plain
//! text here: HTML, SVG and XML are served as downloads, never as anything a browser renders.

/// How many bytes of a file [`sniff`] reads.
pub const HEAD_BYTES: usize = 8 * 1024;

/// The type of a file no signature matches.
pub const OCTET_STREAM: &str = "application/octet-stream";

/// Plain UTF-8 text.
pub const TEXT: &str = "text/plain; charset=utf-8";

/// The type of the file whose first bytes are `head` (at most [`HEAD_BYTES`] are read).
/// `complete` says `head` is the whole file, so a UTF-8 sequence cut at its end is an error
/// rather than the window's edge.
#[must_use]
pub fn sniff(head: &[u8], complete: bool) -> &'static str {
    let head = &head[..head.len().min(HEAD_BYTES)];
    binary(head)
        .or_else(|| text(head, complete))
        .unwrap_or(OCTET_STREAM)
}

/// The fixed signatures: magic numbers at the start of the file.
fn binary(head: &[u8]) -> Option<&'static str> {
    const SIGNATURES: [(&[u8], &str); 6] = [
        (b"%PDF-", "application/pdf"),
        (b"\x89PNG\r\n\x1a\n", "image/png"),
        (b"\xff\xd8\xff", "image/jpeg"),
        (b"GIF87a", "image/gif"),
        (b"GIF89a", "image/gif"),
        (b"ID3", "audio/mpeg"),
    ];
    if let Some((_, mime)) = SIGNATURES.iter().find(|(magic, _)| head.starts_with(magic)) {
        return Some(mime);
    }
    if head.len() >= 12 && head.starts_with(b"RIFF") && &head[8..12] == b"WEBP" {
        return Some("image/webp");
    }
    mp3_frame(head)
        .or_else(|| mp4(head))
        .or_else(|| webm(head))
        .or_else(|| ogg(head))
        .or_else(|| office(head))
}

/// An MPEG audio Layer III frame header without an ID3 tag: an 11-bit frame sync, then a layer
/// of `01`.
fn mp3_frame(head: &[u8]) -> Option<&'static str> {
    match head {
        [0xff, second, ..] if second & 0xe6 == 0xe2 => Some("audio/mpeg"),
        _ => None,
    }
}

/// An ISO base media file whose `ftyp` box names an MP4 major brand. Other brands (HEIF images,
/// QuickTime, 3GP) are not MP4 and stay unmatched.
fn mp4(head: &[u8]) -> Option<&'static str> {
    const BRANDS: [&[u8; 4]; 10] = [
        b"isom", b"iso2", b"iso4", b"iso5", b"iso6", b"mp41", b"mp42", b"avc1", b"M4V ", b"dash",
    ];
    let brand = head.get(8..12)?;
    (head.get(4..8)? == b"ftyp" && BRANDS.iter().any(|b| &b[..] == brand)).then_some("video/mp4")
}

/// An EBML document whose `DocType` element (`42 82`, a one-byte size of 4) is `webm`, in the
/// header at the start of the file. Matroska files name `matroska` and stay unmatched.
fn webm(head: &[u8]) -> Option<&'static str> {
    const DOC_TYPE: &[u8] = b"\x42\x82\x84webm";
    let header = &head[..head.len().min(64)];
    (head.starts_with(b"\x1a\x45\xdf\xa3") && header.windows(DOC_TYPE.len()).any(|w| w == DOC_TYPE))
        .then_some("video/webm")
}

/// An Ogg stream: `audio/ogg` when its first page starts a Vorbis or Opus stream, otherwise
/// the container's own `application/ogg` (RFC 5334).
fn ogg(head: &[u8]) -> Option<&'static str> {
    if !head.starts_with(b"OggS") {
        return None;
    }
    let page = &head[..head.len().min(64)];
    let audio = [&b"\x01vorbis"[..], b"OpusHead"]
        .iter()
        .any(|codec| page.windows(codec.len()).any(|w| w == *codec));
    Some(if audio {
        "audio/ogg"
    } else {
        "application/ogg"
    })
}

/// A ZIP archive that is an Office Open XML or OpenDocument file. A plain ZIP archive stays
/// unmatched.
fn office(head: &[u8]) -> Option<&'static str> {
    if !head.starts_with(b"PK\x03\x04") {
        return None;
    }
    opendocument(head).or_else(|| office_open_xml(head))
}

/// OpenDocument stores its type, uncompressed, as the first entry `mimetype`: the name at byte
/// 30, the contents right after it.
fn opendocument(head: &[u8]) -> Option<&'static str> {
    const TYPES: [&str; 3] = [
        "application/vnd.oasis.opendocument.text",
        "application/vnd.oasis.opendocument.spreadsheet",
        "application/vnd.oasis.opendocument.presentation",
    ];
    let rest = head.get(30..)?.strip_prefix(b"mimetype")?;
    TYPES.into_iter().find(|t| {
        rest.starts_with(t.as_bytes())
            && !rest
                .get(t.len())
                .is_some_and(|next| next.is_ascii_alphanumeric() || *next == b'-')
    })
}

/// Office Open XML: a `[Content_Types].xml` entry and an entry under `word/`, `xl/` or `ppt/`,
/// each found as the name of a local file header in the window.
fn office_open_xml(head: &[u8]) -> Option<&'static str> {
    const PARTS: [(&[u8], &str); 3] = [
        (
            b"word/",
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document",
        ),
        (
            b"xl/",
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet",
        ),
        (
            b"ppt/",
            "application/vnd.openxmlformats-officedocument.presentationml.presentation",
        ),
    ];
    let names = entry_names(head);
    if !names.iter().any(|name| *name == b"[Content_Types].xml") {
        return None;
    }
    PARTS
        .iter()
        .find(|(dir, _)| names.iter().any(|name| name.starts_with(dir)))
        .map(|(_, mime)| *mime)
}

/// The names of the ZIP local file headers that start in `head` and whose name fits in it.
fn entry_names(head: &[u8]) -> Vec<&[u8]> {
    let mut names = Vec::new();
    for at in 0..head.len().saturating_sub(30) {
        if !head[at..].starts_with(b"PK\x03\x04") {
            continue;
        }
        let len = usize::from(u16::from_le_bytes([head[at + 26], head[at + 27]]));
        if let Some(name) = head.get(at + 30..at + 30 + len) {
            names.push(name);
        }
    }
    names
}

/// Plain UTF-8 text: valid UTF-8 (a sequence cut by the window's edge allowed unless
/// `complete`), no control character but tab, line feed, form feed and carriage return, and
/// not starting with `<`. An empty file is not text.
fn text(head: &[u8], complete: bool) -> Option<&'static str> {
    let valid = match std::str::from_utf8(head) {
        Ok(text) => text,
        Err(error) if !complete && error.error_len().is_none() => {
            // The window ends inside a character the rest of the file completes.
            std::str::from_utf8(&head[..error.valid_up_to()]).ok()?
        }
        Err(_) => return None,
    };
    let controls = valid
        .chars()
        .any(|c| c.is_control() && !matches!(c, '\t' | '\n' | '\x0c' | '\r'));
    let first = valid.trim_start_matches(|c: char| c.is_whitespace() || c == '\u{feff}');
    (!head.is_empty() && !controls && !first.starts_with('<')).then_some(TEXT)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn zip_entry(name: &str, contents: &[u8]) -> Vec<u8> {
        let mut entry = b"PK\x03\x04".to_vec();
        entry.extend_from_slice(&[0; 22]);
        let len = u16::try_from(name.len()).unwrap();
        entry.extend_from_slice(&len.to_le_bytes());
        entry.extend_from_slice(&[0, 0]);
        entry.extend_from_slice(name.as_bytes());
        entry.extend_from_slice(contents);
        entry
    }

    #[test]
    fn each_signature_names_its_type() {
        let cases: [(&[u8], &str); 12] = [
            (b"%PDF-1.7\n%\xe2\xe3", "application/pdf"),
            (b"\x89PNG\r\n\x1a\n\0\0\0\rIHDR", "image/png"),
            (b"\xff\xd8\xff\xe0\0\x10JFIF", "image/jpeg"),
            (b"GIF89a\x01\0\x01\0", "image/gif"),
            (b"GIF87a\x01\0\x01\0", "image/gif"),
            (b"RIFF\x24\0\0\0WEBPVP8 ", "image/webp"),
            (b"\0\0\0\x18ftypmp42\0\0\0\0", "video/mp4"),
            (
                b"\x1a\x45\xdf\xa3\x9f\x42\x86\x81\x01\x42\x82\x84webm",
                "video/webm",
            ),
            (b"ID3\x04\0\0\0\0\0\0", "audio/mpeg"),
            (b"\xff\xfb\x90\x64\0", "audio/mpeg"),
            (b"OggS\0\x02\0\0\0\0\0\0\0\0\x01vorbis", "audio/ogg"),
            (b"OggS\0\x02\0\0\0\0\0\0\0\0\x80theora", "application/ogg"),
        ];
        for (head, mime) in cases {
            assert_eq!(sniff(head, true), mime, "{head:?}");
        }
    }

    #[test]
    fn office_files_are_told_apart_from_a_plain_zip() {
        let mut docx = zip_entry("[Content_Types].xml", b"<Types/>");
        docx.extend(zip_entry("word/document.xml", b"<w/>"));
        assert_eq!(
            sniff(&docx, true),
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
        );
        let mut xlsx = zip_entry("[Content_Types].xml", b"");
        xlsx.extend(zip_entry("xl/workbook.xml", b""));
        assert!(sniff(&xlsx, true).ends_with("spreadsheetml.sheet"));
        let odt = zip_entry("mimetype", b"application/vnd.oasis.opendocument.text");
        assert_eq!(sniff(&odt, true), "application/vnd.oasis.opendocument.text");
        let template = zip_entry(
            "mimetype",
            b"application/vnd.oasis.opendocument.text-template",
        );
        assert_eq!(sniff(&template, true), OCTET_STREAM);
        let word_without_types = zip_entry("word/document.xml", b"");
        assert_eq!(sniff(&word_without_types, true), OCTET_STREAM);
        assert_eq!(sniff(&zip_entry("notes.txt", b"hi"), true), OCTET_STREAM);
    }

    #[test]
    fn other_containers_and_brands_are_not_matched() {
        for head in [
            &b"\0\0\0\x18ftypheic\0\0\0\0"[..],
            b"\0\0\0\x14ftypqt  \0\0\0\0",
            b"\x1a\x45\xdf\xa3\x9f\x42\x86\x81\x01\x42\x82\x88matroska",
            b"RIFF\x24\0\0\0WAVEfmt ",
            b"\x7fELF\x02\x01\x01",
            b"MZ\x90\0\x03",
        ] {
            assert_eq!(sniff(head, true), OCTET_STREAM, "{head:?}");
        }
    }

    #[test]
    fn plain_utf8_text_is_text_and_markup_is_not() {
        assert_eq!(
            sniff("Notes for week 1\r\n\tÅngström\n".as_bytes(), true),
            TEXT
        );
        assert_eq!(sniff("\u{feff}Hello".as_bytes(), true), TEXT);
        for markup in [
            "<!DOCTYPE html><html><script>alert(1)</script>",
            "  \n<svg xmlns=\"http://www.w3.org/2000/svg\"/>",
            "\u{feff}<?xml version=\"1.0\"?>",
            "<p>hello</p>",
        ] {
            assert_eq!(sniff(markup.as_bytes(), true), OCTET_STREAM, "{markup}");
        }
        assert_eq!(sniff(b"bell \x07", true), OCTET_STREAM);
        assert_eq!(sniff(b"nul \0", true), OCTET_STREAM);
        assert_eq!(sniff(b"\xc3\x28 latin", true), OCTET_STREAM);
        assert_eq!(sniff(b"", true), OCTET_STREAM);
    }

    #[test]
    fn a_character_cut_by_the_window_is_text_only_when_more_follows() {
        let mut head = vec![b'a'; HEAD_BYTES - 1];
        head.push(0xc3);
        assert_eq!(sniff(&head, false), TEXT);
        assert_eq!(sniff(&head, true), OCTET_STREAM);
    }

    #[test]
    fn only_the_window_is_read() {
        let mut head = vec![b'a'; HEAD_BYTES];
        head.push(0);
        assert_eq!(sniff(&head, false), TEXT);
    }
}
