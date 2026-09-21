//! Task-only business documents. Nothing is extracted to disk or executed.

#[cfg(test)]
mod tests;

use std::{
    collections::HashSet,
    io::{Cursor, Read},
    path::Path,
    sync::{Arc, LazyLock},
    time::{Duration, Instant},
};

use quick_xml::{Reader, events::Event};
use tokio::sync::Semaphore;
use zip::ZipArchive;

use crate::error::AppError;

use super::{ExpectedMedia, MAX_FILE_BYTES};

const MAX_ENTRIES: usize = 2_048;
const MAX_EXPANDED_BYTES: u64 = 50 * 1_024 * 1_024;
const INSPECTION_TIMEOUT: Duration = Duration::from_secs(10);
static INSPECTORS: LazyLock<Arc<Semaphore>> = LazyLock::new(|| Arc::new(Semaphore::new(2)));

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub(super) enum BusinessFile {
    Docx,
    Xlsx,
    Pptx,
    Odt,
    Ods,
    Odp,
    Text,
    Csv,
    Tsv,
    Markdown,
    Zip,
}

impl BusinessFile {
    pub(super) fn from_extension(extension: &str) -> Option<Self> {
        Some(match extension {
            "docx" => Self::Docx,
            "xlsx" => Self::Xlsx,
            "pptx" => Self::Pptx,
            "odt" => Self::Odt,
            "ods" => Self::Ods,
            "odp" => Self::Odp,
            "txt" => Self::Text,
            "csv" => Self::Csv,
            "tsv" => Self::Tsv,
            "md" => Self::Markdown,
            "zip" => Self::Zip,
            _ => return None,
        })
    }

    pub(super) fn accepts_mime(self, mime: &str) -> bool {
        match self {
            Self::Docx => {
                mime == "application/vnd.openxmlformats-officedocument.wordprocessingml.document"
            }
            Self::Xlsx => {
                mime == "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet"
            }
            Self::Pptx => {
                mime == "application/vnd.openxmlformats-officedocument.presentationml.presentation"
            }
            Self::Odt => mime == "application/vnd.oasis.opendocument.text",
            Self::Ods => mime == "application/vnd.oasis.opendocument.spreadsheet",
            Self::Odp => mime == "application/vnd.oasis.opendocument.presentation",
            Self::Text => mime == "text/plain",
            Self::Csv => matches!(mime, "text/csv" | "text/plain" | "application/vnd.ms-excel"),
            Self::Tsv => matches!(mime, "text/tab-separated-values" | "text/plain"),
            Self::Markdown => matches!(mime, "text/markdown" | "text/x-markdown" | "text/plain"),
            Self::Zip => matches!(mime, "application/zip" | "application/x-zip-compressed"),
        }
    }

    const fn main_part(self) -> Option<(&'static str, &'static str)> {
        match self {
            Self::Docx => Some(("word/document.xml", "document")),
            Self::Xlsx => Some(("xl/workbook.xml", "workbook")),
            Self::Pptx => Some(("ppt/presentation.xml", "presentation")),
            Self::Odt | Self::Ods | Self::Odp => Some(("content.xml", "document-content")),
            _ => None,
        }
    }

    const fn odf_mime(self) -> Option<&'static [u8]> {
        match self {
            Self::Odt => Some(b"application/vnd.oasis.opendocument.text"),
            Self::Ods => Some(b"application/vnd.oasis.opendocument.spreadsheet"),
            Self::Odp => Some(b"application/vnd.oasis.opendocument.presentation"),
            _ => None,
        }
    }
}

pub(super) async fn validate(path: &Path, kind: BusinessFile) -> Result<(), AppError> {
    let permit = Arc::clone(&INSPECTORS)
        .try_acquire_owned()
        .map_err(|_| AppError::TooManyRequests)?;
    let bytes = tokio::fs::read(path).await.map_err(AppError::internal)?;
    // Keep the permit inside the blocking job, including if the upload is cancelled.
    tokio::task::spawn_blocking(move || {
        let _permit = permit;
        validate_bytes(&bytes, kind, &mut Budget::new())
    })
    .await
    .map_err(AppError::internal)?
}

struct Budget {
    bytes: u64,
    entries: usize,
    deadline: Instant,
}

impl Budget {
    fn new() -> Self {
        Self {
            bytes: MAX_EXPANDED_BYTES,
            entries: MAX_ENTRIES,
            deadline: Instant::now() + INSPECTION_TIMEOUT,
        }
    }

    fn check(&self) -> Result<(), AppError> {
        if Instant::now() > self.deadline {
            return Err(rejected("file inspection exceeded its time limit"));
        }
        Ok(())
    }
}

fn rejected(message: &str) -> AppError {
    AppError::UnsupportedMediaType(message.to_owned())
}

fn validate_bytes(bytes: &[u8], kind: BusinessFile, budget: &mut Budget) -> Result<(), AppError> {
    budget.check()?;
    if bytes.is_empty() || bytes.len() > MAX_FILE_BYTES {
        return Err(rejected(
            "business documents must be non-empty and at most 20 MiB",
        ));
    }
    if kind.main_part().is_some() || kind == BusinessFile::Zip {
        inspect_archive(bytes, kind, budget)
    } else {
        validate_text(bytes)
    }
}

fn validate_text(bytes: &[u8]) -> Result<(), AppError> {
    let valid_character = |ch: char| !ch.is_control() || matches!(ch, '\t' | '\n' | '\r');
    let valid = if bytes.starts_with(b"\xff\xfe") || bytes.starts_with(b"\xfe\xff") {
        let little_endian = bytes[0] == 0xff;
        let units = bytes[2..].as_chunks::<2>().0.iter().map(|chunk| {
            if little_endian {
                u16::from_le_bytes([chunk[0], chunk[1]])
            } else {
                u16::from_be_bytes([chunk[0], chunk[1]])
            }
        });
        bytes.len().is_multiple_of(2)
            && char::decode_utf16(units).all(|ch| ch.is_ok_and(valid_character))
    } else {
        std::str::from_utf8(bytes).is_ok_and(|text| text.chars().all(valid_character))
    };
    if valid {
        Ok(())
    } else {
        Err(rejected(
            "text attachments must contain UTF-8 or UTF-16 text, not binary data",
        ))
    }
}

fn inspect_archive(bytes: &[u8], kind: BusinessFile, budget: &mut Budget) -> Result<(), AppError> {
    if !bytes.starts_with(b"PK\x03\x04") {
        return Err(rejected("document is not a supported ZIP package"));
    }
    let mut archive =
        ZipArchive::new(Cursor::new(bytes)).map_err(|_| rejected("invalid ZIP package"))?;
    if archive.is_empty() || archive.len() > budget.entries || archive.offset() != 0 {
        return Err(rejected(
            "archive contains too many entries or an unsupported structure",
        ));
    }
    budget.entries -= archive.len();
    if archive
        .has_overlapping_files()
        .map_err(|_| rejected("invalid ZIP structure"))?
    {
        return Err(rejected("overlapping archive entries are not allowed"));
    }
    let mut names = HashSet::new();
    let mut has_main = false;
    let mut has_manifest = false;
    let mut has_mime = kind.odf_mime().is_none();
    for index in 0..archive.len() {
        budget.check()?;
        let mut entry = archive
            .by_index(index)
            .map_err(|_| rejected("encrypted or damaged archive entries are not allowed"))?;
        let name = entry.name().to_owned();
        let lower = name.to_ascii_lowercase();
        if entry.enclosed_name().is_none()
            || name.starts_with('/')
            || name.len() > 512
            || name.trim() != name
            || name
                .chars()
                .any(|ch| super::is_dangerous_filename_character(ch) && ch != '/')
            || name
                .trim_end_matches('/')
                .split('/')
                .any(|part| matches!(part, "" | "." | ".."))
            || entry.is_symlink()
            || entry
                .unix_mode()
                .is_some_and(|mode| !matches!(mode & 0o170_000, 0 | 0o100_000 | 0o040_000))
            || !names.insert(lower.clone())
        {
            return Err(rejected("archive contains unsafe or duplicate paths"));
        }
        if entry.is_dir() {
            if entry.size() != 0 {
                return Err(rejected("archive directories must not contain data"));
            }
            continue;
        }
        let size = entry.size();
        if size > budget.bytes || size > MAX_EXPANDED_BYTES {
            return Err(rejected("expanded archive exceeds the 50 MiB limit"));
        }
        let mut content = Vec::new();
        (&mut entry)
            .take(size + 1)
            .read_to_end(&mut content)
            .map_err(|_| rejected("archive entry could not be read"))?;
        if content.len() as u64 != size {
            return Err(rejected("archive entry size is invalid"));
        }
        budget.bytes -= size;
        budget.check()?;
        if kind == BusinessFile::Zip {
            inspect_zip_member(&name, &content, budget)?;
            continue;
        }
        if unsafe_office_part(&lower) || content.starts_with(b"\xd0\xcf\x11\xe0\xa1\xb1\x1a\xe1") {
            return Err(rejected(
                "macros, embedded objects and executable document parts are not allowed",
            ));
        }
        let root = if matches!(lower.rsplit('.').next(), Some("xml" | "rels" | "vml")) {
            Some(inspect_xml(&content, budget)?)
        } else {
            None
        };
        if let Some((main, expected_root)) = kind.main_part()
            && name == main
        {
            has_main = root.as_deref() == Some(expected_root);
        }
        if name == "[Content_Types].xml"
            && root.as_deref() == Some("types")
            && kind.odf_mime().is_none()
        {
            has_manifest = office_content_type_matches(&content, kind)?;
        }
        if name == "META-INF/manifest.xml"
            && root.as_deref() == Some("manifest")
            && kind.odf_mime().is_some()
        {
            has_manifest = true;
        }
        if name == "mimetype"
            && let Some(mime) = kind.odf_mime()
        {
            has_mime = content == mime;
        }
    }
    if kind != BusinessFile::Zip && !(has_main && has_manifest && has_mime) {
        return Err(rejected("document package does not match its extension"));
    }
    Ok(())
}

fn inspect_zip_member(name: &str, content: &[u8], budget: &mut Budget) -> Result<(), AppError> {
    let file_name = name.rsplit('/').next().unwrap_or_default();
    let (_, media) = super::validate_task_file_metadata(file_name, "")?;
    if content.is_empty() || content.len() > media.max_bytes() {
        return Err(rejected("archive member exceeds the allowed file size"));
    }
    let valid = match media {
        ExpectedMedia::Business(BusinessFile::Zip) => {
            return Err(rejected("nested ZIP archives are not allowed"));
        }
        ExpectedMedia::Business(kind) => return validate_bytes(content, kind, budget),
        ExpectedMedia::Jpeg | ExpectedMedia::Png | ExpectedMedia::WebP => {
            super::validate_image(content, media)
        }
        ExpectedMedia::Pdf => super::validate_pdf(content, content),
        ExpectedMedia::Mp4 => super::validate_mp4(content, content.len()),
        ExpectedMedia::WebM => super::validate_webm(content),
    };
    if valid {
        Ok(())
    } else {
        Err(rejected(
            "archive member contents do not match its extension",
        ))
    }
}

fn unsafe_office_part(name: &str) -> bool {
    let extension = name.rsplit('.').next().unwrap_or_default();
    let printer_settings = name.contains("/printersettings/printersettings") && extension == "bin";
    // Office packages need only data parts. ActiveX, macros and embedded OLE
    // objects remain forbidden even when a macro-enabled file is renamed .docx.
    !matches!(
        extension,
        "xml"
            | "rels"
            | "vml"
            | "png"
            | "jpg"
            | "jpeg"
            | "gif"
            | "bmp"
            | "tif"
            | "tiff"
            | "emf"
            | "wmf"
            | "webp"
            | "odttf"
            | "ttf"
            | "fnt"
    ) && name != "mimetype"
        && !printer_settings
        || [
            "vbaproject",
            "vbadata",
            "macros/",
            "basic/",
            "scripts/",
            "activex/",
            "embeddings/",
            "externallinks/",
            "macrosheets/",
            "dialogsheets/",
            "webextensions/",
            "customui/",
        ]
        .iter()
        .any(|part| name.contains(part))
}

fn office_content_type_matches(bytes: &[u8], kind: BusinessFile) -> Result<bool, AppError> {
    let expected_type = match kind {
        BusinessFile::Docx => {
            "application/vnd.openxmlformats-officedocument.wordprocessingml.document.main+xml"
        }
        BusinessFile::Xlsx => {
            "application/vnd.openxmlformats-officedocument.spreadsheetml.sheet.main+xml"
        }
        BusinessFile::Pptx => {
            "application/vnd.openxmlformats-officedocument.presentationml.presentation.main+xml"
        }
        _ => return Ok(false),
    };
    let Some((part, _)) = kind.main_part() else {
        return Ok(false);
    };
    let expected_part = format!("/{part}");
    let mut reader = Reader::from_reader(bytes);
    loop {
        match reader
            .read_event()
            .map_err(|_| rejected("invalid document XML"))?
        {
            Event::Start(tag) | Event::Empty(tag) if tag.local_name().as_ref() == b"Override" => {
                let mut matches_part = false;
                let mut matches_type = false;
                for attribute in tag.attributes() {
                    let attribute =
                        attribute.map_err(|_| rejected("invalid document XML attribute"))?;
                    let value = attribute
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            reader.decoder(),
                        )
                        .map_err(|_| rejected("invalid document XML attribute"))?;
                    matches_part |= attribute.key.as_ref() == b"PartName" && value == expected_part;
                    matches_type |=
                        attribute.key.as_ref() == b"ContentType" && value == expected_type;
                }
                if matches_part && matches_type {
                    return Ok(true);
                }
            }
            Event::Eof => return Ok(false),
            _ => {}
        }
    }
}

fn inspect_xml(bytes: &[u8], budget: &Budget) -> Result<String, AppError> {
    let mut reader = Reader::from_reader(bytes);
    reader.config_mut().expand_empty_elements = true;
    let mut root = None;
    let mut elements = Vec::new();
    let mut instructions = String::new();
    let mut formulas = String::new();
    let mut events = 0_usize;
    loop {
        events += 1;
        if events.is_multiple_of(1_024) {
            budget.check()?;
        }
        if events > 1_000_000 {
            return Err(rejected("document XML is too complex"));
        }
        match reader
            .read_event()
            .map_err(|_| rejected("invalid document XML"))?
        {
            Event::Start(tag) => {
                let local = String::from_utf8_lossy(tag.local_name().as_ref()).to_ascii_lowercase();
                if matches!(
                    local.as_str(),
                    "script"
                        | "scripts"
                        | "event-listener"
                        | "event-listeners"
                        | "encryption-data"
                        | "oleobject"
                        | "object-ole"
                        | "object"
                        | "control"
                        | "altchunk"
                        | "ddelink"
                        | "ddelinks"
                        | "dde-source"
                        | "externalreference"
                ) {
                    return Err(rejected(
                        "active or encrypted document content is not allowed",
                    ));
                }
                if root.is_none() {
                    root = Some(local.clone());
                } else if elements.is_empty() {
                    return Err(rejected("document XML must have one root"));
                }
                let mut relationship_type = String::new();
                let mut target = String::new();
                let mut external = false;
                for attribute in tag.attributes() {
                    let attribute =
                        attribute.map_err(|_| rejected("invalid document XML attribute"))?;
                    let value = attribute
                        .decoded_and_normalized_value(
                            quick_xml::XmlVersion::Implicit1_0,
                            reader.decoder(),
                        )
                        .map_err(|_| rejected("invalid document XML attribute"))?;
                    let value_lower = value.to_ascii_lowercase();
                    match attribute.key.local_name().as_ref() {
                        b"ContentType" | b"Type" => {
                            if [
                                "macroenabled",
                                "vbaproject",
                                "vbadata",
                                "activex",
                                "oleobject",
                                "attachedtemplate",
                                "externallink",
                            ]
                            .iter()
                            .any(|needle| value_lower.contains(needle))
                                || value_lower.ends_with("/package")
                            {
                                return Err(rejected(
                                    "active document relationships are not allowed",
                                ));
                            }
                            if attribute.key.as_ref() == b"Type" {
                                relationship_type = value_lower;
                            }
                        }
                        b"TargetMode" => external = value_lower == "external",
                        b"Target" => target = value.to_string(),
                        b"instr" => {
                            instructions.push_str(&value);
                            instructions.push(' ');
                        }
                        b"formula" => {
                            formulas.push_str(&value);
                            formulas.push(' ');
                        }
                        b"href" => {
                            if value.contains(':') && (local != "a" || !is_web_link(&value)) {
                                return Err(rejected(
                                    "external document resources are not allowed",
                                ));
                            }
                            if value.starts_with('/') || value.split('/').any(|part| part == "..") {
                                return Err(rejected(
                                    "external document resources are not allowed",
                                ));
                            }
                        }
                        _ => {}
                    }
                }
                if external && (!relationship_type.ends_with("/hyperlink") || !is_web_link(&target))
                {
                    return Err(rejected("external document resources are not allowed"));
                }
                // Reader expands empty tags so depth and one-root checks stay exact.
                elements.push(local);
                if elements.len() > 128 {
                    return Err(rejected("document XML is too deeply nested"));
                }
            }
            Event::End(_) => {
                elements
                    .pop()
                    .ok_or_else(|| rejected("invalid XML nesting"))?;
            }
            Event::DocType(_) => {
                return Err(rejected(
                    "XML document types and external entities are not allowed",
                ));
            }
            Event::Text(text) => {
                let value = text
                    .decode()
                    .map_err(|_| rejected("invalid document text"))?;
                collect_field_text(elements.last(), &value, &mut instructions, &mut formulas);
            }
            Event::CData(text) => {
                let value = text
                    .decode()
                    .map_err(|_| rejected("invalid document text"))?;
                collect_field_text(elements.last(), &value, &mut instructions, &mut formulas);
            }
            Event::GeneralRef(reference) => {
                let reference = reference
                    .decode()
                    .map_err(|_| rejected("invalid XML reference"))?;
                let encoded = format!("&{reference};");
                let value = quick_xml::escape::unescape(&encoded)
                    .map_err(|_| rejected("custom XML entities are not allowed"))?;
                collect_field_text(elements.last(), &value, &mut instructions, &mut formulas);
            }
            Event::Eof => break,
            _ => {}
        }
    }
    if !elements.is_empty() {
        return Err(rejected("invalid XML nesting"));
    }
    if instructions
        .to_ascii_uppercase()
        .split_whitespace()
        .any(|word| matches!(word, "DDE" | "DDEAUTO"))
        || formulas.contains('|')
        || formulas.to_ascii_uppercase().contains("DDE(")
        || formulas.to_ascii_uppercase().contains("WEBSERVICE(")
        || formulas.to_ascii_uppercase().contains("RTD(")
    {
        return Err(rejected("active document fields are not allowed"));
    }
    root.ok_or_else(|| rejected("document XML is empty"))
}

fn collect_field_text(
    element: Option<&String>,
    value: &str,
    instructions: &mut String,
    formulas: &mut String,
) {
    match element.map(String::as_str) {
        Some("instrtext") => instructions.push_str(value),
        Some("f") => formulas.push_str(value),
        _ => {}
    }
}

fn is_web_link(value: &str) -> bool {
    let lower = value.to_ascii_lowercase();
    lower.starts_with("https://") || lower.starts_with("http://") || lower.starts_with("mailto:")
}
