//! Temporary, bounded material readers for AI-assisted skill authoring.

use std::{
    cell::RefCell,
    collections::HashMap,
    io::{Cursor, Read},
    time::{Duration, Instant},
};

use base64::{Engine as _, engine::general_purpose::STANDARD};
use quick_xml::{
    Reader, XmlVersion,
    escape::resolve_predefined_entity,
    events::{BytesStart, Event},
};
use tokio::sync::Semaphore;
use zip::ZipArchive;

use crate::{avatars::image_metadata, error::AppError};

pub(crate) const MAX_FILE_BYTES: usize = 20 * 1_024 * 1_024;
pub(crate) const MAX_IMAGE_BYTES: usize = 10 * 1_024 * 1_024;
pub(crate) const MAX_TEXT_CHARS: usize = 1_000_000;
const MAX_ARCHIVE_BYTES: u64 = 50 * 1_024 * 1_024;
const MAX_ARCHIVE_ENTRIES: usize = 2_048;
const MAX_PDF_STREAM_BYTES: usize = 5 * 1_024 * 1_024;
const PARSE_TIMEOUT: Duration = Duration::from_secs(20);
static PARSERS: Semaphore = Semaphore::const_new(2);
thread_local! {
    // lopdf has default features disabled, including its parallel Rayon reader.
    static PDF_LOAD_BUDGET: RefCell<Option<PdfLoadBudget>> = const { RefCell::new(None) };
}

struct PdfLoadBudget {
    bytes: usize,
    objects: usize,
    failed: bool,
}

impl PdfLoadBudget {
    fn bytes(&mut self, count: usize) -> bool {
        let Some(remaining) = self.bytes.checked_sub(count) else {
            return false;
        };
        self.bytes = remaining;
        true
    }

    fn object(&mut self, object: &lopdf::Object, depth: usize) -> bool {
        if self.objects == 0 || depth > 64 {
            return false;
        }
        self.objects -= 1;
        match object {
            lopdf::Object::Array(items) => items.iter().all(|item| self.object(item, depth + 1)),
            lopdf::Object::Dictionary(dictionary) => self.dictionary(dictionary, depth),
            lopdf::Object::Stream(stream) => {
                self.bytes(stream.content.len()) && self.dictionary(&stream.dict, depth)
            }
            lopdf::Object::Name(bytes) | lopdf::Object::String(bytes, _) => self.bytes(bytes.len()),
            _ => true,
        }
    }

    fn dictionary(&mut self, dictionary: &lopdf::Dictionary, depth: usize) -> bool {
        dictionary
            .iter()
            .all(|(key, value)| self.bytes(key.len()) && self.object(value, depth + 1))
    }
}

pub(crate) enum SkillSource {
    Text {
        name: String,
        text: String,
    },
    Image {
        name: String,
        content_type: String,
        data_url: String,
    },
}

/// Files stay in memory and are discarded after the draft generation request.
pub(crate) async fn parse_material(
    file_name: &str,
    _content_type: &str,
    bytes: Vec<u8>,
) -> Result<SkillSource, AppError> {
    let name = file_name
        .rsplit(['/', '\\'])
        .next()
        .unwrap_or_default()
        .trim()
        .to_owned();
    if name.is_empty() || name.chars().count() > 200 || name.chars().any(char::is_control) {
        return Err(bad(
            "material filename must contain between 1 and 200 characters",
        ));
    }
    if bytes.is_empty() || bytes.len() > MAX_FILE_BYTES {
        return Err(AppError::PayloadTooLarge(
            "each material must contain between 1 byte and 20 MiB".to_owned(),
        ));
    }
    let extension = name
        .rsplit('.')
        .next()
        .unwrap_or_default()
        .to_ascii_lowercase();
    if matches!(extension.as_str(), "png" | "jpg" | "jpeg" | "webp") {
        return image_source(name, &bytes);
    }
    if !matches!(extension.as_str(), "txt" | "md" | "pdf" | "docx" | "epub") {
        return Err(AppError::UnsupportedMediaType(
            "supported materials: TXT, MD, PDF, DOCX, EPUB, JPEG, PNG, WebP".to_owned(),
        ));
    }
    let permit = PARSERS
        .try_acquire()
        .map_err(|_| AppError::TooManyRequests)?;
    // The permit remains in the blocking worker even when its caller times out.
    // A timeout cannot forcibly interrupt a third-party parser mid-operation.
    let worker = tokio::task::spawn_blocking(move || {
        let _permit = permit;
        let deadline = Instant::now() + PARSE_TIMEOUT;
        let text = match extension.as_str() {
            "pdf" => pdf_text(&bytes, deadline)?,
            "docx" | "epub" => archive_text(&bytes, &extension, deadline)?,
            _ => std::str::from_utf8(&bytes)
                .map_err(|_| bad("text materials must use UTF-8 encoding"))?
                .trim_start_matches('\u{feff}')
                .to_owned(),
        };
        let text = text.trim().to_owned();
        check_text(&text)?;
        if text.is_empty() {
            return Err(bad(
                "material has no readable text; use a text PDF, TXT, DOCX, EPUB, or upload page images",
            ));
        }
        Ok(SkillSource::Text { name, text })
    });
    tokio::time::timeout(PARSE_TIMEOUT, worker)
        .await
        .map_err(|_| bad("material took too long to read; upload a smaller excerpt"))?
        .map_err(|_| bad("material could not be read; check the file and try again"))?
}

fn image_source(name: String, bytes: &[u8]) -> Result<SkillSource, AppError> {
    if bytes.len() > MAX_IMAGE_BYTES {
        return Err(AppError::PayloadTooLarge(
            "each image must be at most 10 MiB".to_owned(),
        ));
    }
    let (content_type, width, height) = image_metadata(bytes)
        .ok_or_else(|| bad("material must be a valid JPEG, PNG, or WebP image"))?;
    if width == 0
        || height == 0
        || width > 8_192
        || height > 8_192
        || u64::from(width) * u64::from(height) > 40_000_000
    {
        return Err(bad(
            "image dimensions must be at most 8192 pixels and 40 megapixels",
        ));
    }
    Ok(SkillSource::Image {
        name,
        content_type: content_type.to_owned(),
        data_url: format!("data:{content_type};base64,{}", STANDARD.encode(bytes)),
    })
}

fn pdf_text(bytes: &[u8], deadline: Instant) -> Result<String, AppError> {
    if !bytes.starts_with(b"%PDF-") {
        return Err(bad("material is not a PDF file"));
    }
    validate_pdf_structure(bytes)?;
    PDF_LOAD_BUDGET.with_borrow_mut(|budget| {
        *budget = Some(PdfLoadBudget {
            bytes: 50 * 1_024 * 1_024,
            objects: 50_000,
            failed: false,
        });
    });
    let document = lopdf::Document::load_mem_with_options(
        bytes,
        lopdf::LoadOptions {
            filter: Some(limit_pdf_object),
            ..lopdf::LoadOptions::with_max_decompressed_size(MAX_PDF_STREAM_BYTES)
        },
    );
    if PDF_LOAD_BUDGET
        .with_borrow_mut(Option::take)
        .is_none_or(|budget| budget.failed)
    {
        return Err(bad(
            "PDF is too complex or exceeds the 50 MiB parsing limit; upload a smaller excerpt",
        ));
    }
    let document =
        document.map_err(|_| bad("PDF could not be read; damaged PDFs are not supported"))?;
    if document.is_encrypted() || document.encryption_state.is_some() {
        return Err(bad("encrypted PDFs are not supported"));
    }
    let pages = document.get_pages();
    if pages.len() > 1_000 {
        return Err(bad(
            "PDF materials must contain at most 1000 pages; upload a smaller excerpt",
        ));
    }
    let mut text = String::new();
    for (page, page_id) in pages {
        check_deadline(deadline)?;
        let page_text = document
            .extract_text_with_limit(&[page], MAX_PDF_STREAM_BYTES)
            .map_err(|_| {
                bad("PDF text could not be read or a page is too complex; upload a text export")
            })?;
        if page_text.trim().is_empty() {
            let content = document
                .get_page_content_with_limit(page_id, MAX_PDF_STREAM_BYTES)
                .map_err(|_| bad("PDF page could not be read"))?;
            let operations = lopdf::content::Content::decode_strict(&content)
                .map_err(|_| bad("PDF page content could not be read; use a text export"))?;
            // Check actual drawing operations; inherited image resources can also
            // appear on genuinely blank pages and must not reject those pages.
            if operations
                .operations
                .iter()
                .any(|operation| matches!(operation.operator.as_str(), "Do" | "BI"))
            {
                return Err(bad(
                    "PDF contains scanned or visual-only pages; use OCR or upload those pages as images",
                ));
            }
        }
        text.push_str(&page_text);
        text.push('\n');
        check_text(&text)?;
    }
    Ok(text)
}

/// `lopdf` eagerly expands object and cross-reference streams during loading,
/// before our page loop can enforce aggregate limits or its deadline. Accept
/// classic cross-reference tables only; page/font compression remains supported.
/// Scan raw bytes conservatively (including strings and streams), decoding PDF
/// name escapes, so obfuscated names cannot enter an eager decompression path.
fn validate_pdf_structure(bytes: &[u8]) -> Result<(), AppError> {
    for (index, byte) in bytes.iter().enumerate() {
        if *byte == b'/' && unsupported_pdf_name(&bytes[index + 1..]) {
            return Err(unsupported_pdf_structure());
        }
    }
    // Match lopdf's selection of the final EOF and its preceding startxref.
    // Requiring the selected target to begin with a table also rejects an xref
    // stream whose dictionary omits /Type /XRef entirely.
    let tail_start = bytes.len().saturating_sub(512);
    let eof = bytes[tail_start..]
        .windows(5)
        .rposition(|part| part == b"%%EOF")
        .map(|index| tail_start + index)
        .ok_or_else(unsupported_pdf_structure)?;
    let start = eof.saturating_sub(25);
    let marker = bytes[start..eof]
        .windows(9)
        .rposition(|part| part == b"startxref")
        .map(|index| start + index + 9)
        .ok_or_else(unsupported_pdf_structure)?;
    let offset: usize = std::str::from_utf8(&bytes[marker..eof])
        .ok()
        .and_then(|value| value.trim().parse().ok())
        .ok_or_else(unsupported_pdf_structure)?;
    let target = bytes.get(offset..).ok_or_else(unsupported_pdf_structure)?;
    if !target.starts_with(b"xref") || !target.get(4).is_some_and(|byte| pdf_whitespace(*byte)) {
        return Err(unsupported_pdf_structure());
    }
    Ok(())
}

fn unsupported_pdf_name(bytes: &[u8]) -> bool {
    // Every blocked name is at most seven bytes; no allocation or unbounded
    // token scan is needed even for a hostile multi-megabyte name.
    let mut name = [0_u8; 8];
    let mut length = 0;
    let mut cursor = 0;
    while let Some(&byte) = bytes.get(cursor) {
        if pdf_whitespace(byte) || b"()<>[]{}/%".contains(&byte) {
            break;
        }
        let decoded = if byte == b'#' {
            let Some(value) = bytes
                .get(cursor + 1..cursor + 3)
                .and_then(|pair| Some(hex_digit(pair[0])? * 16 + hex_digit(pair[1])?))
            else {
                break;
            };
            cursor += 3;
            value
        } else {
            cursor += 1;
            byte
        };
        if length == name.len() {
            return false;
        }
        name[length] = decoded;
        length += 1;
    }
    matches!(
        &name[..length],
        b"ObjStm" | b"Encrypt" | b"Prev" | b"XRefStm"
    )
}

fn hex_digit(byte: u8) -> Option<u8> {
    match byte {
        b'0'..=b'9' => Some(byte - b'0'),
        b'a'..=b'f' => Some(byte - b'a' + 10),
        b'A'..=b'F' => Some(byte - b'A' + 10),
        _ => None,
    }
}

fn pdf_whitespace(byte: u8) -> bool {
    b" \t\n\r\0\x0c".contains(&byte)
}

fn unsupported_pdf_structure() -> AppError {
    bad(
        "Optimized, incremental, or encrypted PDFs are not supported. Re-export a standard PDF without object compression, or upload TXT, DOCX, or EPUB.",
    )
}

fn limit_pdf_object(
    id: lopdf::ObjectId,
    object: &mut lopdf::Object,
) -> Option<(lopdf::ObjectId, lopdf::Object)> {
    PDF_LOAD_BUDGET.with_borrow_mut(|budget| {
        let budget = budget.as_mut()?;
        if budget.failed || !budget.object(object, 0) {
            budget.failed = true;
            return None;
        }
        Some((id, object.clone()))
    })
}

fn archive_text(bytes: &[u8], extension: &str, deadline: Instant) -> Result<String, AppError> {
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|_| bad("DOCX or EPUB archive is invalid"))?;
    if archive.len() > MAX_ARCHIVE_ENTRIES {
        return Err(bad("document contains too many archived files"));
    }
    let mut total = 0_u64;
    for index in 0..archive.len() {
        let file = archive
            .by_index(index)
            .map_err(|_| bad("encrypted or damaged document archive"))?;
        if file.enclosed_name().is_none() || file.name().contains('\\') || file.is_symlink() {
            return Err(bad("document contains an unsafe archive path"));
        }
        total = total.saturating_add(file.size());
        if total > MAX_ARCHIVE_BYTES {
            return Err(bad(
                "document expands beyond 50 MiB; upload a smaller excerpt",
            ));
        }
    }
    let mut budget = MAX_ARCHIVE_BYTES;
    if extension == "docx" {
        let xml = read_entry(&mut archive, "word/document.xml", &mut budget)?;
        return xml_text(&xml, true, deadline);
    }
    if read_entry(&mut archive, "mimetype", &mut budget)?.trim() != "application/epub+zip" {
        return Err(bad("material is not an EPUB book"));
    }
    let container = read_entry(&mut archive, "META-INF/container.xml", &mut budget)?;
    let package = xml_nodes(&container, &[b"rootfile"])?
        .into_iter()
        .find_map(|(_, mut attrs)| attrs.remove("full-path"))
        .ok_or_else(|| bad("EPUB package was not found"))?;
    let package = archive_path("", &package)?;
    let xml = read_entry(&mut archive, &package, &mut budget)?;
    let nodes = xml_nodes(&xml, &[b"item", b"itemref"])?;
    let mut manifest = HashMap::new();
    let mut spine = Vec::new();
    for (tag, mut attrs) in nodes {
        if tag == "item" {
            if let (Some(id), Some(href), Some(media_type)) = (
                attrs.remove("id"),
                attrs.remove("href"),
                attrs.remove("media-type"),
            ) {
                manifest.insert(id, (href, media_type));
            }
        } else if let Some(idref) = attrs.remove("idref") {
            spine.push(idref);
        }
    }
    let base = package.rsplit_once('/').map_or("", |(base, _)| base);
    let mut text = String::new();
    for id in spine {
        check_deadline(deadline)?;
        let (href, media_type) = manifest
            .get(&id)
            .ok_or_else(|| bad("EPUB chapter was not found"))?;
        if !matches!(media_type.as_str(), "application/xhtml+xml" | "text/html") {
            return Err(bad("EPUB chapters must contain readable text"));
        }
        let path = archive_path(base, href)?;
        text.push_str(&xml_text(
            &read_entry(&mut archive, &path, &mut budget)?,
            false,
            deadline,
        )?);
        text.push('\n');
        check_text(&text)?;
    }
    Ok(text)
}

fn read_entry(
    archive: &mut ZipArchive<Cursor<&[u8]>>,
    path: &str,
    budget: &mut u64,
) -> Result<String, AppError> {
    let file = archive
        .by_name(path)
        .map_err(|_| bad("document is missing required content"))?;
    let mut bytes = Vec::new();
    file.take(*budget + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| bad("document content could not be decompressed"))?;
    let size = u64::try_from(bytes.len()).map_err(AppError::internal)?;
    if size > *budget {
        return Err(bad(
            "document expands beyond 50 MiB; upload a smaller excerpt",
        ));
    }
    *budget -= size;
    String::from_utf8(bytes).map_err(|_| bad("document XML must use UTF-8 encoding"))
}

fn archive_path(base: &str, href: &str) -> Result<String, AppError> {
    let href = percent_encoding::percent_decode_str(href.split('#').next().unwrap_or_default())
        .decode_utf8()
        .map_err(|_| bad("EPUB chapter path is invalid"))?;
    if href.starts_with('/') || href.contains([':', '\\', '\0']) {
        return Err(bad("external EPUB chapters are not supported"));
    }
    let mut parts: Vec<_> = base.split('/').filter(|part| !part.is_empty()).collect();
    for part in href.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts
                    .pop()
                    .ok_or_else(|| bad("EPUB chapter path leaves the archive"))?;
            }
            _ => parts.push(part),
        }
    }
    Ok(parts.join("/"))
}

type XmlNode = (String, HashMap<String, String>);

fn xml_nodes(xml: &str, tags: &[&[u8]]) -> Result<Vec<XmlNode>, AppError> {
    let mut reader = Reader::from_str(xml);
    let mut nodes = Vec::new();
    loop {
        match reader
            .read_event()
            .map_err(|_| bad("document XML is invalid"))?
        {
            Event::Start(tag) | Event::Empty(tag) if tags.contains(&tag.local_name().as_ref()) => {
                nodes.push((
                    String::from_utf8_lossy(tag.local_name().as_ref()).into_owned(),
                    attributes(&tag)?,
                ));
            }
            Event::Eof => return Ok(nodes),
            _ => {}
        }
    }
}

fn attributes(tag: &BytesStart<'_>) -> Result<HashMap<String, String>, AppError> {
    tag.attributes()
        .map(|attribute| {
            let attribute = attribute.map_err(|_| bad("document XML attribute is invalid"))?;
            Ok((
                String::from_utf8_lossy(attribute.key.local_name().as_ref()).into_owned(),
                attribute
                    .normalized_value(XmlVersion::Implicit1_0)
                    .map_err(|_| bad("document XML attribute is invalid"))?
                    .into_owned(),
            ))
        })
        .collect()
}

fn xml_text(xml: &str, docx: bool, deadline: Instant) -> Result<String, AppError> {
    let mut reader = Reader::from_str(xml);
    let mut text = String::new();
    let mut in_text = false;
    let mut hidden_depth = 0_usize;
    let text_element: &[u8] = if docx { b"t" } else { b"body" };
    loop {
        check_deadline(deadline)?;
        let event = reader
            .read_event()
            .map_err(|_| bad("document XML is invalid"))?;
        match event {
            Event::Start(tag) => {
                let local = tag.local_name();
                if hidden_depth > 0
                    || matches!(
                        local.as_ref(),
                        b"script" | b"style" | b"head" | b"svg" | b"del"
                    )
                {
                    hidden_depth += 1;
                }
                if local.as_ref() == text_element {
                    in_text = true;
                }
            }
            Event::End(tag) => {
                if hidden_depth > 0 {
                    hidden_depth -= 1;
                    continue;
                }
                if tag.local_name().as_ref() == text_element {
                    in_text = false;
                }
                if matches!(
                    tag.local_name().as_ref(),
                    b"p" | b"div" | b"h1" | b"h2" | b"h3" | b"li" | b"tr" | b"section"
                ) {
                    text.push('\n');
                }
                if matches!(tag.local_name().as_ref(), b"tc" | b"td" | b"th") {
                    text.push('\t');
                }
            }
            Event::Empty(tag)
                if hidden_depth == 0
                    && matches!(tag.local_name().as_ref(), b"br" | b"cr" | b"tab") =>
            {
                text.push('\n');
            }
            Event::Text(value) if in_text && hidden_depth == 0 => text.push_str(
                &value
                    .decode()
                    .map_err(|_| bad("document text is invalid"))?,
            ),
            Event::CData(value) if in_text && hidden_depth == 0 => text.push_str(
                &value
                    .decode()
                    .map_err(|_| bad("document text is invalid"))?,
            ),
            Event::GeneralRef(value) if in_text && hidden_depth == 0 => {
                if let Some(character) = value
                    .resolve_char_ref()
                    .map_err(|_| bad("document character reference is invalid"))?
                {
                    text.push(character);
                } else {
                    let entity = value
                        .decode()
                        .map_err(|_| bad("document entity is invalid"))?;
                    text.push_str(
                        resolve_predefined_entity(&entity)
                            .ok_or_else(|| bad("custom XML entities are not supported"))?,
                    );
                }
            }
            Event::Eof => {
                check_text(&text)?;
                return Ok(text);
            }
            _ => {}
        }
        // UTF-8 uses at most four bytes per character; avoid rescanning every event.
        if text.len() > MAX_TEXT_CHARS * 4 {
            return Err(text_too_large());
        }
    }
}

fn check_text(text: &str) -> Result<(), AppError> {
    if text.contains('\0') {
        return Err(bad("material text must not contain null bytes"));
    }
    if text.chars().count() > MAX_TEXT_CHARS {
        return Err(text_too_large());
    }
    Ok(())
}

fn check_deadline(deadline: Instant) -> Result<(), AppError> {
    if Instant::now() >= deadline {
        return Err(bad(
            "material took too long to read; upload a smaller excerpt",
        ));
    }
    Ok(())
}

fn text_too_large() -> AppError {
    bad("material contains more than 1000000 characters; upload a smaller excerpt")
}

fn bad(message: &str) -> AppError {
    AppError::BadRequest(message.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;
    use lopdf::{Document, Object, Stream, dictionary};
    use std::io::Write;
    use zip::{ZipWriter, write::SimpleFileOptions};
    static PARSER_TEST: tokio::sync::Mutex<()> = tokio::sync::Mutex::const_new(());

    fn archive(entries: &[(&str, &[u8])]) -> Vec<u8> {
        let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
        for (name, data) in entries {
            writer
                .start_file(
                    *name,
                    SimpleFileOptions::default()
                        .compression_method(zip::CompressionMethod::Deflated),
                )
                .unwrap();
            writer.write_all(data).unwrap();
        }
        writer.finish().unwrap().into_inner()
    }

    fn pdf(content: &[u8]) -> Vec<u8> {
        let mut document = Document::with_version("1.5");
        document.reference_table.cross_reference_type = lopdf::xref::XrefType::CrossReferenceTable;
        let pages = document.new_object_id();
        let font = document
            .add_object(dictionary! {"Type"=>"Font", "Subtype"=>"Type1", "BaseFont"=>"Helvetica"});
        let resources = document.add_object(dictionary! {"Font"=>dictionary! {"F1"=>font}});
        let stream = document.add_object(Stream::new(dictionary! {}, content.to_vec()));
        let page = document.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages, "Contents"=>stream, "Resources"=>resources});
        document.objects.insert(pages, Object::Dictionary(dictionary! {"Type"=>"Pages", "Kids"=>vec![Object::Reference(page)], "Count"=>1, "MediaBox"=>vec![0.into(),0.into(),600.into(),800.into()]}));
        let catalog = document.add_object(dictionary! {"Type"=>"Catalog", "Pages"=>pages});
        document.trailer.set("Root", catalog);
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        bytes
    }

    fn text(source: SkillSource) -> String {
        match source {
            SkillSource::Text { text, .. } => text,
            SkillSource::Image { .. } => panic!("expected text"),
        }
    }

    #[tokio::test]
    async fn reads_utf8_and_rejects_empty_invalid_and_excessive_text() {
        let _guard = PARSER_TEST.lock().await;
        assert_eq!(
            text(
                parse_material(
                    "note.MD",
                    "text/plain",
                    "\u{feff}Привет мир".as_bytes().to_vec()
                )
                .await
                .unwrap()
            ),
            "Привет мир"
        );
        for bytes in [
            b"  \n".to_vec(),
            vec![0xff],
            b"a\0b".to_vec(),
            vec![b'x'; MAX_TEXT_CHARS + 1],
        ] {
            assert!(
                parse_material("note.txt", "text/plain", bytes)
                    .await
                    .is_err()
            );
        }
        assert!(
            parse_material("file.exe", "text/plain", b"abc".to_vec())
                .await
                .is_err()
        );
    }

    #[tokio::test]
    async fn reads_pdf_text_and_rejects_scanned_and_invalid_pdf() {
        let _guard = PARSER_TEST.lock().await;
        assert!(
            text(
                parse_material(
                    "book.pdf",
                    "application/pdf",
                    pdf(b"BT /F1 12 Tf 10 20 Td (Hello book) Tj ET")
                )
                .await
                .unwrap()
            )
            .contains("Hello book")
        );
        assert!(
            parse_material("scan.pdf", "application/pdf", pdf(b"q Q"))
                .await
                .is_err()
        );
        assert!(
            parse_material(
                "broken.pdf",
                "application/pdf",
                b"%PDF-1.5 damaged".to_vec()
            )
            .await
            .is_err()
        );
    }

    #[test]
    fn rejects_pdf_decompression_bomb() {
        let bytes = pdf(b"BT /F1 12 Tf (Hello) Tj ET");
        let mut document = Document::load_mem(&bytes).unwrap();
        for object in document.objects.values_mut() {
            if let Object::Stream(stream) = object {
                stream.set_content(vec![b' '; MAX_PDF_STREAM_BYTES + 1]);
                stream.compress().unwrap();
            }
        }
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        assert!(pdf_text(&bytes, Instant::now() + PARSE_TIMEOUT).is_err());
    }

    #[test]
    fn preflight_rejects_aggregate_object_stream_bomb_without_loading() {
        let mut document = Document::load_mem(&pdf(b"BT /F1 12 Tf (Hello) Tj ET")).unwrap();
        // About 1 MiB on input, but eagerly loading these streams would retain
        // more than 1 GiB even though every individual stream meets its limit.
        let mut stream = Stream::new(
            // The writer strips existing ObjStm objects. Rename the type after
            // serialization, preserving its byte length and every xref offset.
            dictionary! {"Type"=>"RawStm", "N"=>0, "First"=>0},
            vec![b' '; MAX_PDF_STREAM_BYTES],
        );
        stream.compress().unwrap();
        for _ in 0..240 {
            document.add_object(stream.clone());
        }
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        let positions: Vec<_> = bytes
            .windows(6)
            .enumerate()
            .filter_map(|(index, value)| (value == b"RawStm").then_some(index))
            .collect();
        assert_eq!(positions.len(), 240);
        for index in positions {
            bytes[index..index + 6].copy_from_slice(b"ObjStm");
        }
        assert!(bytes.len() < 2 * 1_024 * 1_024);
        let error = pdf_text(&bytes, Instant::now() + PARSE_TIMEOUT).unwrap_err();
        assert!(error.to_string().contains("without object compression"));
    }

    #[test]
    fn fails_entire_pdf_when_aggregate_object_budget_is_exceeded() {
        let mut document = Document::load_mem(&pdf(b"BT /F1 12 Tf (Hello) Tj ET")).unwrap();
        document.add_object(lopdf::Object::Array(vec![lopdf::Object::Null; 50_001]));
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        assert!(
            pdf_text(&bytes, Instant::now() + PARSE_TIMEOUT)
                .unwrap_err()
                .to_string()
                .contains("parsing limit")
        );
        // Failure state must not leak to the next upload on the same worker.
        assert!(
            pdf_text(
                &pdf(b"BT /F1 12 Tf (Next) Tj ET"),
                Instant::now() + PARSE_TIMEOUT
            )
            .unwrap()
            .contains("Next")
        );
    }

    #[test]
    fn preflight_handles_escaped_names_and_untyped_xref_streams() {
        for name in ["ObjStm", "#4Fb#6AStm", "Encr#79pt", "Pr#65v", "XR#65fStm"] {
            let bytes = pdf(format!("BT /F1 12 Tf (/{name}) Tj ET").as_bytes());
            assert!(validate_pdf_structure(&bytes).is_err(), "accepted {name}");
        }
        assert!(!unsupported_pdf_name(b"ObjStmExtra "));
        let bytes = b"%PDF-1.5\n1 0 obj <</Size 1 /W [999999999 1 1] /Length 0>>stream\nendstream\nendobj\nstartxref\n9\n%%EOF\n";
        assert!(validate_pdf_structure(bytes).is_err());
    }

    #[test]
    fn accepts_classic_pdf_with_compressed_page_content() {
        let content = b"BT /F1 12 Tf (Hello compressed page) Tj ET\n".repeat(100);
        let mut document = Document::load_mem(&pdf(&content)).unwrap();
        for object in document.objects.values_mut() {
            if let Object::Stream(stream) = object {
                stream.compress().unwrap();
                assert!(stream.dict.has(b"Filter"));
            }
        }
        let mut bytes = Vec::new();
        document.save_to(&mut bytes).unwrap();
        assert!(
            pdf_text(&bytes, Instant::now() + PARSE_TIMEOUT)
                .unwrap()
                .contains("Hello compressed page")
        );
    }

    #[test]
    fn rejects_mixed_pdf_scanned_pages_but_accepts_blank_pages() {
        for scanned in [false, true] {
            let mut document = Document::load_mem(&pdf(b"BT /F1 12 Tf (Hello) Tj ET")).unwrap();
            let first_page = *document.get_pages().get(&1).unwrap();
            let pages = document
                .get_dictionary(first_page)
                .unwrap()
                .get(b"Parent")
                .unwrap()
                .as_reference()
                .unwrap();
            let image = document.add_object(Stream::new(dictionary! {"Type"=>"XObject", "Subtype"=>"Image", "Width"=>1, "Height"=>1, "ColorSpace"=>"DeviceRGB", "BitsPerComponent"=>8}, vec![255; 3]));
            let resources =
                document.add_object(dictionary! {"XObject"=>dictionary! {"Im1"=>image}});
            let content = if scanned {
                b"q /Im1 Do Q".as_slice()
            } else {
                b"q Q".as_slice()
            };
            let stream = document.add_object(Stream::new(dictionary! {}, content.to_vec()));
            let page = document.add_object(dictionary! {"Type"=>"Page", "Parent"=>pages, "Contents"=>stream, "Resources"=>resources});
            let pages_dict = document
                .get_object_mut(pages)
                .unwrap()
                .as_dict_mut()
                .unwrap();
            pages_dict.set(
                "Kids",
                vec![Object::Reference(first_page), Object::Reference(page)],
            );
            pages_dict.set("Count", 2);
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).unwrap();
            let result = pdf_text(&bytes, Instant::now() + PARSE_TIMEOUT);
            if scanned {
                assert!(result.unwrap_err().to_string().contains("scanned"));
            } else {
                assert!(result.unwrap().contains("Hello"));
            }
        }
    }

    #[test]
    fn rejects_encrypted_pdf_even_with_empty_password() {
        for password in ["", "secret"] {
            let mut document = Document::load_mem(&pdf(b"BT /F1 12 Tf (Hello) Tj ET")).unwrap();
            document.trailer.set(
                "ID",
                vec![
                    Object::string_literal("test-id"),
                    Object::string_literal("test-id"),
                ],
            );
            let state = lopdf::EncryptionState::try_from(lopdf::EncryptionVersion::V1 {
                document: &document,
                owner_password: "owner",
                user_password: password,
                permissions: lopdf::Permissions::COPYABLE,
            })
            .unwrap();
            document.encrypt(&state).unwrap();
            let mut bytes = Vec::new();
            document.save_to(&mut bytes).unwrap();
            assert!(pdf_text(&bytes, Instant::now() + PARSE_TIMEOUT).is_err());
        }
    }

    #[tokio::test]
    async fn reads_docx_runs_entities_and_paragraphs_without_external_resources() {
        let _guard = PARSER_TEST.lock().await;
        let bytes = archive(&[
            ("word/document.xml", br#"<w:document xmlns:w="urn:word"><w:body><w:p><w:r><w:t>Hello</w:t></w:r><w:r><w:t> &amp; world</w:t></w:r></w:p><w:p><w:r><w:t>Second</w:t></w:r></w:p></w:body></w:document>"#),
            ("word/_rels/document.xml.rels", br#"<Relationships><Relationship Target="https://example.invalid/private" TargetMode="External"/></Relationships>"#),
        ]);
        assert_eq!(
            text(
                parse_material("book.docx", "application/octet-stream", bytes)
                    .await
                    .unwrap()
            ),
            "Hello & world\nSecond"
        );
    }

    fn epub(chapter_href: &str) -> Vec<u8> {
        let package = format!(
            r#"<package><manifest><item id="one" href="{chapter_href}" media-type="application/xhtml+xml"/><item id="two" href="two.xhtml" media-type="application/xhtml+xml"/></manifest><spine><itemref idref="two"/><itemref idref="one"/></spine></package>"#
        );
        archive(&[
            ("mimetype", b"application/epub+zip"),
            ("META-INF/container.xml", br#"<container><rootfiles><rootfile full-path="OEBPS/content.opf"/></rootfiles></container>"#),
            ("OEBPS/content.opf", package.as_bytes()),
            ("OEBPS/one.xhtml", br#"<!DOCTYPE html SYSTEM "https://example.invalid/dtd"><html><head><title>Hidden title</title></head><body><p>First &amp; text</p><script>Hidden script</script><img src="https://example.invalid/private"/></body></html>"#),
            ("OEBPS/two.xhtml", br#"<html><body><p>Second</p></body></html>"#),
        ])
    }

    #[tokio::test]
    async fn reads_epub_in_spine_order_without_scripts_or_external_images() {
        let _guard = PARSER_TEST.lock().await;
        let output = text(
            parse_material("book.epub", "application/epub+zip", epub("one.xhtml"))
                .await
                .unwrap(),
        );
        assert_eq!(output, "Second\n\nFirst & text");
        assert!(
            parse_material(
                "book.epub",
                "application/epub+zip",
                epub("https://example.invalid/chapter")
            )
            .await
            .is_err()
        );
    }

    #[test]
    fn rejects_archive_traversal_expansion_and_custom_entities() {
        let deadline = Instant::now() + PARSE_TIMEOUT;
        assert!(
            archive_text(
                &archive(&[("../word/document.xml", b"hello")]),
                "docx",
                deadline
            )
            .is_err()
        );
        let huge = vec![b' '; usize::try_from(MAX_ARCHIVE_BYTES).unwrap() + 1];
        assert!(archive_text(&archive(&[("word/document.xml", &huge)]), "docx", deadline).is_err());
        assert!(xml_text(r#"<!DOCTYPE x [<!ENTITY secret SYSTEM "file:///etc/passwd">]><body>&secret;</body>"#, false, deadline).is_err());
        assert!(archive_path("OEBPS", "../../private").is_err());
        assert!(archive_path("OEBPS", "%2e%2e/%2e%2e/private").is_err());
        assert_eq!(
            archive_path("OEBPS", "first%20chapter.xhtml").unwrap(),
            "OEBPS/first chapter.xhtml"
        );
    }

    #[test]
    fn rejects_archive_with_too_many_entries() {
        let names: Vec<_> = (0..=MAX_ARCHIVE_ENTRIES)
            .map(|index| format!("item{index}.xml"))
            .collect();
        let entries: Vec<_> = names
            .iter()
            .map(|name| (name.as_str(), b"x".as_slice()))
            .collect();
        assert!(archive_text(&archive(&entries), "docx", Instant::now() + PARSE_TIMEOUT).is_err());
    }

    #[test]
    fn validates_image_type_and_dimensions_from_content() {
        let png = STANDARD.decode("iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aXioAAAAASUVORK5CYII=").unwrap();
        let SkillSource::Image {
            content_type,
            data_url,
            ..
        } = image_source("one.png".to_owned(), &png).unwrap()
        else {
            panic!("expected image")
        };
        assert_eq!(content_type, "image/png");
        assert!(data_url.starts_with("data:image/png;base64,"));
        assert!(image_source("fake.png".to_owned(), b"<svg/>").is_err());
        let mut oversized = png;
        oversized[16..20].copy_from_slice(&9_000_u32.to_be_bytes());
        assert!(image_source("huge.png".to_owned(), &oversized).is_err());
    }
}
