use std::io::Write;

use zip::{ZipWriter, write::SimpleFileOptions};

use super::*;

fn zip(entries: &[(&str, &[u8])]) -> Vec<u8> {
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    for (name, content) in entries {
        writer
            .start_file(*name, SimpleFileOptions::default())
            .unwrap();
        writer.write_all(content).unwrap();
    }
    writer.finish().unwrap().into_inner()
}

fn office(kind: BusinessFile, extra: &[(&str, &[u8])]) -> Vec<u8> {
    let (part, root) = kind.main_part().unwrap();
    let content_type = match kind {
        BusinessFile::Docx => "wordprocessingml.document",
        BusinessFile::Xlsx => "spreadsheetml.sheet",
        BusinessFile::Pptx => "presentationml.presentation",
        _ => unreachable!(),
    };
    let manifest = format!(
        "<Types><Override PartName=\"/{part}\" ContentType=\"application/vnd.openxmlformats-officedocument.{content_type}.main+xml\"/></Types>"
    );
    let document = format!("<{root}><body/></{root}>");
    let mut entries = vec![
        ("[Content_Types].xml", manifest.as_bytes()),
        (part, document.as_bytes()),
    ];
    entries.extend_from_slice(extra);
    zip(&entries)
}

fn accepts(bytes: &[u8], kind: BusinessFile) -> bool {
    validate_bytes(bytes, kind, &mut Budget::new()).is_ok()
}

#[test]
fn accepts_business_packages_text_and_mixed_archives() {
    for kind in [BusinessFile::Docx, BusinessFile::Xlsx, BusinessFile::Pptx] {
        let document = office(kind, &[]);
        assert!(accepts(&document, kind));
        let extension = match kind {
            BusinessFile::Docx => "docx",
            BusinessFile::Xlsx => "xlsx",
            _ => "pptx",
        };
        assert!(accepts(
            &zip(&[
                (&format!("folder/report.{extension}"), &document),
                ("notes.txt", "Примечания".as_bytes())
            ]),
            BusinessFile::Zip
        ));
    }
    for kind in [BusinessFile::Odt, BusinessFile::Ods, BusinessFile::Odp] {
        let document = zip(&[
            ("mimetype", kind.odf_mime().unwrap()),
            (
                "content.xml",
                b"<document-content><body/></document-content>",
            ),
            ("META-INF/manifest.xml", b"<manifest/>"),
        ]);
        assert!(accepts(&document, kind));
    }
    assert!(accepts(
        "Договор\nДата\tСтоимость".as_bytes(),
        BusinessFile::Text
    ));
    assert!(accepts(b"\xff\xfeT\0e\0x\0t\0", BusinessFile::Text));
    assert!(accepts(b"\xfe\xff\0T\0e\0x\0t", BusinessFile::Text));
    assert!(accepts(b"name,amount\norder,200\n", BusinessFile::Csv));
}

#[test]
fn rejects_renamed_binary_wrong_office_family_and_macro_documents() {
    assert!(!accepts(b"MZ\x00\x01", BusinessFile::Text));
    assert!(!accepts(b"\xff\xfeT", BusinessFile::Text));
    assert!(!accepts(b"not a DOCX", BusinessFile::Docx));
    assert!(!accepts(
        &office(BusinessFile::Xlsx, &[]),
        BusinessFile::Docx
    ));
    for part in [
        "word/vbaProject.bin",
        "word/activeX/control.xml",
        "word/embeddings/oleObject.bin",
        "customUI/customUI.xml",
        "word/payload.exe",
    ] {
        assert!(
            !accepts(
                &office(BusinessFile::Docx, &[(part, b"payload")]),
                BusinessFile::Docx
            ),
            "{part}"
        );
    }
    let macro_types = b"<Types><Override PartName=\"/word/document.xml\" ContentType=\"application/vnd.ms-word.document.macroEnabled.main+xml\"/></Types>";
    assert!(!accepts(
        &zip(&[
            ("[Content_Types].xml", macro_types),
            ("word/document.xml", b"<document/>")
        ]),
        BusinessFile::Docx
    ));
}

#[test]
fn blocks_active_xml_dde_and_external_resources_but_keeps_web_hyperlinks() {
    for xml in [
        "<!DOCTYPE document [<!ENTITY xxe SYSTEM 'file:///etc/passwd'>]><document>&xxe;</document>",
        "<document><instrText>D&#68;E</instrText><instrText>AUTO cmd</instrText></document>",
        "<document><fldSimple instr='DDEAUTO cmd'/></document>",
        "<worksheet><f>cmd|' /C calc'!A0</f></worksheet>",
        "<document-content><table-cell formula='of:=DDE(&quot;cmd&quot;)'/></document-content>",
        "<Relationships><Relationship Type='http://schemas/attachedTemplate' TargetMode='External' Target='https://example.test/template.dotm'/></Relationships>",
        "<Relationships><Relationship Type='http://schemas/hyperlink' TargetMode='External' Target='file:///example.txt'/></Relationships>",
        "<document><script/></document>",
        "<document/><document/>",
        "<document><unclosed/>",
    ] {
        assert!(
            inspect_xml(xml.as_bytes(), &Budget::new()).is_err(),
            "{xml}"
        );
    }
    assert!(inspect_xml(b"<Relationships><Relationship Type='http://schemas/hyperlink' TargetMode='External' Target='https://example.test'/></Relationships>", &Budget::new()).is_ok());
    assert!(inspect_xml(b"<document><instrText>HYPERLINK &quot;https://example.test&quot;</instrText><text>A &amp; B</text></document>", &Budget::new()).is_ok());
}

#[test]
fn rejects_zip_slip_executables_nested_archives_symlinks_and_encryption() {
    for name in [
        "../notes.txt",
        "/notes.txt",
        "folder/../../notes.txt",
        "folder\\notes.txt",
        "notes.txt:payload.exe",
        "setup.exe",
        "script.js",
        "page.html",
        "macro.docm",
    ] {
        assert!(
            !accepts(&zip(&[(name, b"payload")]), BusinessFile::Zip),
            "{name}"
        );
    }
    assert!(!accepts(
        &zip(&[("archive.zip", &zip(&[("notes.txt", b"notes")]))]),
        BusinessFile::Zip
    ));
    assert!(!accepts(
        &zip(&[("notes.txt", b"a"), ("NOTES.TXT", b"b")]),
        BusinessFile::Zip
    ));
    let mut writer = ZipWriter::new(Cursor::new(Vec::new()));
    writer
        .add_symlink("notes.txt", "../secret", SimpleFileOptions::default())
        .unwrap();
    assert!(!accepts(
        &writer.finish().unwrap().into_inner(),
        BusinessFile::Zip
    ));
    let mut encrypted = zip(&[("notes.txt", b"notes")]);
    // Mark both local and central directory headers encrypted. No password path is permitted.
    encrypted[6] |= 1;
    let central = encrypted
        .windows(4)
        .position(|bytes| bytes == b"PK\x01\x02")
        .unwrap();
    encrypted[central + 8] |= 1;
    assert!(!accepts(&encrypted, BusinessFile::Zip));
}

#[test]
fn enforces_shared_expansion_entry_and_time_budgets() {
    let document = office(BusinessFile::Docx, &[]);
    let archive = zip(&[("report.docx", &document), ("notes.txt", b"notes")]);
    let mut budget = Budget::new();
    budget.bytes = document.len() as u64;
    assert!(validate_bytes(&archive, BusinessFile::Zip, &mut budget).is_err());
    let mut budget = Budget::new();
    budget.entries = 2;
    assert!(validate_bytes(&archive, BusinessFile::Zip, &mut budget).is_err());
    let mut budget = Budget::new();
    budget.deadline = Instant::now().checked_sub(Duration::from_secs(1)).unwrap();
    assert!(validate_bytes(&archive, BusinessFile::Zip, &mut budget).is_err());
    let deep = format!("{}{}", "<x>".repeat(129), "</x>".repeat(129));
    assert!(inspect_xml(deep.as_bytes(), &Budget::new()).is_err());
}
