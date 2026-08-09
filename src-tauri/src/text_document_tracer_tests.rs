use std::fs;

use tempfile::tempdir_in;

use crate::{
    application::text_document_service::{
        OpenTextDocument, SaveTextDocument, SaveTextDocumentAs, TextDocumentService,
    },
    config::TextDocumentLimits,
    domain::text_document::{LineEnding, SaveLineEnding, TextEncoding},
};

fn test_directory() -> tempfile::TempDir {
    tempdir_in(concat!(env!("CARGO_MANIFEST_DIR"), "/target")).expect("temporary test directory")
}

#[test]
fn user_can_open_utf8_txt_with_detected_crlf() {
    let directory = test_directory();
    let path = directory.path().join("中文测试.txt");
    fs::write(&path, "第一行\r\nSecond line\r\n").expect("write fixture");

    let service = TextDocumentService::new(TextDocumentLimits::default());
    let opened = service
        .open(OpenTextDocument {
            path,
            encoding_override: None,
            allow_large: false,
        })
        .expect("open valid UTF-8 document");

    assert_eq!(opened.content, "第一行\nSecond line\n");
    assert_eq!(opened.encoding, TextEncoding::Utf8);
    assert!(!opened.has_bom);
    assert_eq!(opened.line_ending, LineEnding::Crlf);
    assert_eq!(opened.revision, 0);
}

#[test]
fn user_can_edit_save_and_reopen_utf8_txt() {
    let directory = test_directory();
    let path = directory.path().join("round-trip.txt");
    fs::write(&path, "before\n").expect("write fixture");
    let service = TextDocumentService::new(TextDocumentLimits::default());
    let opened = service
        .open(OpenTextDocument {
            path: path.clone(),
            encoding_override: None,
            allow_large: false,
        })
        .expect("open fixture");

    let saved = service
        .save(SaveTextDocument {
            document_id: opened.document_id,
            content: "after 中文 😀\n".to_owned(),
            encoding: TextEncoding::Utf8,
            has_bom: false,
            line_ending: SaveLineEnding::Preserve,
            expected_revision: opened.revision,
        })
        .expect("save through the application service");

    assert_eq!(saved.revision, 1);
    assert_eq!(
        fs::read_to_string(path).expect("read saved file"),
        "after 中文 😀\n"
    );
}

#[test]
fn external_modification_blocks_save_without_overwriting_disk() {
    let directory = test_directory();
    let path = directory.path().join("externally-edited.txt");
    fs::write(&path, "opened\n").expect("write fixture");
    let service = TextDocumentService::new(TextDocumentLimits::default());
    let opened = service
        .open(OpenTextDocument {
            path: path.clone(),
            encoding_override: None,
            allow_large: false,
        })
        .expect("open fixture");
    fs::write(&path, "external change\n").expect("simulate another editor");

    let error = service
        .save(SaveTextDocument {
            document_id: opened.document_id,
            content: "readloom change\n".to_owned(),
            encoding: TextEncoding::Utf8,
            has_bom: false,
            line_ending: SaveLineEnding::Preserve,
            expected_revision: opened.revision,
        })
        .expect_err("external change must block overwrite");

    assert_eq!(error.to_dto().code, "EXTERNAL_MODIFICATION");
    assert_eq!(
        fs::read_to_string(path).expect("read disk"),
        "external change\n"
    );
}

#[test]
fn unrepresentable_gbk_save_leaves_original_file_intact() {
    let directory = test_directory();
    let path = directory.path().join("gbk-protection.txt");
    fs::write(&path, "original\n").expect("write fixture");
    let service = TextDocumentService::new(TextDocumentLimits::default());
    let opened = service
        .open(OpenTextDocument {
            path: path.clone(),
            encoding_override: None,
            allow_large: false,
        })
        .expect("open fixture");

    let error = service
        .save(SaveTextDocument {
            document_id: opened.document_id,
            content: "中文和 emoji 😀\n".to_owned(),
            encoding: TextEncoding::Gbk,
            has_bom: false,
            line_ending: SaveLineEnding::Lf,
            expected_revision: opened.revision,
        })
        .expect_err("GBK must not silently replace emoji");

    assert_eq!(error.to_dto().code, "UNREPRESENTABLE_CHARACTERS");
    assert_eq!(
        fs::read_to_string(path).expect("read original"),
        "original\n"
    );
}

#[test]
fn save_as_creates_new_file_and_does_not_modify_original() {
    let directory = test_directory();
    let original = directory.path().join("original.txt");
    let copy = directory.path().join("copy.txt");
    fs::write(&original, "original\r\n").expect("write fixture");
    let service = TextDocumentService::new(TextDocumentLimits::default());
    let opened = service
        .open(OpenTextDocument {
            path: original.clone(),
            encoding_override: None,
            allow_large: false,
        })
        .expect("open fixture");

    let saved = service
        .save_as(SaveTextDocumentAs {
            document_id: opened.document_id,
            target_path: copy.clone(),
            content: "copy 中文\n".to_owned(),
            encoding: TextEncoding::Utf8,
            has_bom: true,
            line_ending: SaveLineEnding::Crlf,
            expected_revision: opened.revision,
            allow_overwrite: false,
        })
        .expect("save as a new file");

    assert_eq!(saved.revision, 1);
    assert_eq!(
        fs::read_to_string(original).expect("original"),
        "original\r\n"
    );
    assert_eq!(
        fs::read(copy).expect("copy"),
        [b"\xEF\xBB\xBF".as_slice(), "copy 中文\r\n".as_bytes()].concat()
    );
}

#[test]
fn configured_size_boundaries_are_reported_before_reading_content() {
    let directory = test_directory();
    let confirmation_path = directory.path().join("confirm.txt");
    let rejected_path = directory.path().join("reject.txt");
    fs::write(&confirmation_path, b"12345").expect("confirmation fixture");
    fs::write(&rejected_path, b"123456789").expect("rejection fixture");
    let service = TextDocumentService::new(TextDocumentLimits {
        confirmation_threshold_bytes: 4,
        maximum_editable_bytes: 8,
    });

    let confirmation = service
        .open(OpenTextDocument {
            path: confirmation_path,
            encoding_override: None,
            allow_large: false,
        })
        .expect_err("file above confirmation threshold");
    let rejected = service
        .open(OpenTextDocument {
            path: rejected_path,
            encoding_override: None,
            allow_large: true,
        })
        .expect_err("file above maximum threshold");

    assert_eq!(
        confirmation.to_dto().code,
        "LARGE_FILE_CONFIRMATION_REQUIRED"
    );
    assert_eq!(rejected.to_dto().code, "FILE_TOO_LARGE");
    assert_eq!(
        TextDocumentLimits::default().confirmation_threshold_bytes,
        40 * 1024 * 1024
    );
    assert_eq!(
        TextDocumentLimits::default().maximum_editable_bytes,
        160 * 1024 * 1024
    );
}

#[test]
fn unknown_extension_is_opened_as_text() {
    let directory = test_directory();
    let path = directory.path().join("not-text.md");
    fs::write(&path, "text").expect("fixture");
    let service = TextDocumentService::new(TextDocumentLimits::default());

    let opened = service
        .open(OpenTextDocument {
            path,
            encoding_override: None,
            allow_large: false,
        })
        .expect("unknown extensions should use the text reader");

    assert!(opened.document_id.0.starts_with("txt-"));
    assert_eq!(opened.content, "text");
    assert_eq!(opened.file_name, "not-text.md");
}
