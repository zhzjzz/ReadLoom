mod backup;
mod core;
mod editing;
mod epub;
mod epub_edit;
mod reader;
mod settings;
mod text_codec;
mod txt_edit;

pub use backup::{BackupSummary, RestoreSummary};
pub use core::{
    CoreError, LibraryDocument, LibraryGroup, LibrarySnapshot, ReadloomCore, StoredBookmark,
};
pub use editing::{
    BlockId, ChapterKey, DocumentDraft, EditChange, EditCommand, EditError, EditSession,
    EditableBlock, ImageMediaType, InsertSide, JoinDirection, SaveOutcome, SaveState, SaveTicket,
    ValidatedImageAsset, ViewAnchor,
};
pub use epub::{EpubChapter, EpubDocument, EpubImageResource, EpubReadingLocator};
pub use epub_edit::EpubDraft;
pub use reader::{
    ParagraphKind, ReaderDocument, ReadingParagraph, SearchHit, TextReadingLocator, TxtChapter,
};
pub use settings::{
    AppSettings, AppTheme, BookSettings, ChapterTitleStyle, DEFAULT_TXT_CHAPTER_PATTERN,
    DataSettings, EpubSettings, ReadingSettings, ShortcutSettings, TextAlignment, TxtBlankLines,
    TxtLeadingIndent, TxtSettings, WindowCloseAction,
};
pub use text_codec::{LineEnding, SaveTextOptions, TextEncoding};
pub use txt_edit::TxtDraft;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reader_exposes_detected_txt_chapters_and_paragraphs() {
        let document = ReaderDocument::from_text(
            "示例.txt",
            "序章\n开场正文。\n\n第一章 风起\n这是第一章正文。".to_owned(),
        );

        assert_eq!(
            document
                .chapters()
                .iter()
                .map(|chapter| chapter.title.as_str())
                .collect::<Vec<_>>(),
            ["序章", "第一章 风起"]
        );
        assert_eq!(document.paragraphs()[1].text, "开场正文。");
    }

    #[test]
    fn reader_search_returns_a_navigable_paragraph_anchor() {
        let document = ReaderDocument::from_text(
            "搜索.txt",
            "第一章\n平静的开场。\n风声从窗外传来。\n尾声\n结束。".to_owned(),
        );

        let hits = document.search("风声", 20);

        assert_eq!(hits.len(), 1);
        assert_eq!(hits[0].paragraph_index, 2);
        assert_eq!(hits[0].chapter_index, 0);
        assert_eq!(hits[0].character_offset_in_paragraph, 0);
        assert!(hits[0].preview.contains("风声"));
    }

    #[test]
    fn txt_locator_keeps_legacy_fields_and_resolves_the_paragraph_anchor() {
        let document = ReaderDocument::from_text(
            "定位.txt",
            "第一章\n第一段。\n目标段落。\n第二章\n结尾。".to_owned(),
        );

        let locator = document.locator_for_paragraph(2, 2);
        let json = serde_json::to_value(&locator).expect("serialize locator");

        assert_eq!(json["version"], 1);
        assert!(json.get("characterOffset").is_some());
        assert!(json.get("lineNumber").is_some());
        assert_eq!(json["chapterIndex"], 0);
        assert_eq!(json["paragraphIndex"], 2);
        assert_eq!(json["characterOffsetInParagraph"], 2);
        assert_eq!(document.resolve_locator(&locator), 2);
    }

    #[test]
    fn opening_a_txt_makes_it_available_through_the_library_interface() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("readloom-state.sqlite3");
        let txt = directory.path().join("本地书.txt");
        std::fs::write(&txt, "第一章\r\n正文。\r\n").expect("write txt");
        let core = ReadloomCore::open(&database).expect("open core");

        let opened = core.open_txt(&txt).expect("open txt");
        let library = core.library_snapshot(50).expect("load library");

        assert_eq!(opened.title(), "本地书.txt");
        assert_eq!(opened.paragraphs()[1].text, "正文。");
        assert_eq!(library.documents.len(), 1);
        assert_eq!(library.documents[0].display_title, "本地书.txt");
        assert!(library.documents[0].available);
        assert_eq!(library.documents[0].document_kind, "txt");
    }

    #[test]
    fn removing_a_library_book_keeps_the_source_file() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("readloom-state.sqlite3");
        let txt = directory.path().join("只移除收藏.txt");
        std::fs::write(&txt, "第一章\n正文。\n").expect("write txt");
        let core = ReadloomCore::open(&database).expect("open core");
        let document = core.open_txt(&txt).expect("open txt");
        core.save_text_locator(&document, &document.locator_for_paragraph(1, 0))
            .expect("save progress");
        core.add_text_bookmark(&document, 1).expect("save bookmark");

        assert!(
            core.remove_from_library(&txt)
                .expect("remove library entry")
        );
        assert!(
            txt.is_file(),
            "removing a library entry must not delete the book"
        );
        assert!(
            core.library_snapshot(50)
                .expect("load library")
                .documents
                .is_empty()
        );
        assert!(
            !core
                .remove_from_library(&txt)
                .expect("remove missing entry")
        );
        drop(core);

        let connection = rusqlite::Connection::open(&database).expect("open state database");
        for table in [
            "library_entries",
            "reading_progress",
            "bookmarks",
            "recent_documents",
        ] {
            let count: i64 = connection
                .query_row(&format!("SELECT COUNT(*) FROM {table}"), [], |row| {
                    row.get(0)
                })
                .expect("count document state");
            assert_eq!(count, 0, "{table} should be cleared when removing a book");
        }
    }

    #[test]
    fn library_groups_reject_blank_and_duplicate_names() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("readloom-state.sqlite3");
        let core = ReadloomCore::open(&database).expect("open core");

        let group = core
            .create_library_group("  长篇小说  ")
            .expect("create library group");
        let _second_group = core
            .create_library_group("历史")
            .expect("create second group");
        let third_group = core
            .create_library_group("随笔")
            .expect("create third group");

        assert_eq!(group.name, "长篇小说");
        assert_eq!(
            core.library_snapshot(50)
                .expect("load library")
                .groups
                .iter()
                .map(|group| group.name.as_str())
                .collect::<Vec<_>>(),
            ["随笔", "历史", "长篇小说"]
        );
        assert!(core.create_library_group("长篇小说").is_err());
        assert!(core.create_library_group("   ").is_err());
        assert!(
            core.reorder_library_group(&third_group.group_id, 2)
                .expect("move newest group to the end")
        );
        assert_eq!(
            core.library_snapshot(50)
                .expect("load reordered library")
                .groups
                .iter()
                .map(|group| group.name.as_str())
                .collect::<Vec<_>>(),
            ["历史", "长篇小说", "随笔"]
        );
        assert!(
            !core
                .reorder_library_group(&third_group.group_id, 2)
                .expect("same position is unchanged")
        );
        assert!(
            core.reorder_library_group(&group.group_id, 0)
                .expect("move a group directly to the front")
        );

        let txt = directory.path().join("待分组.txt");
        std::fs::write(&txt, "第一章\n正文。\n").expect("write grouped txt");
        core.open_txt(&txt).expect("open grouped txt");
        assert!(
            core.move_library_book(&txt, Some(&group.group_id))
                .expect("move to group")
        );
        assert_eq!(
            core.library_snapshot(50)
                .expect("load grouped library")
                .documents[0]
                .group_id,
            Some(group.group_id.clone())
        );
        assert!(
            core.delete_library_group(&group.group_id)
                .expect("delete populated group")
        );
        let snapshot = core.library_snapshot(50).expect("load deleted group state");
        assert!(
            snapshot
                .groups
                .iter()
                .all(|candidate| candidate.group_id != group.group_id)
        );
        assert_eq!(snapshot.documents[0].group_id, None);
        assert!(txt.is_file(), "deleting a group must keep its source books");
        assert!(
            !core
                .delete_library_group(&group.group_id)
                .expect("deleting the same group twice is unchanged")
        );
    }

    #[test]
    fn cleaning_invalid_library_entries_keeps_valid_books_and_clears_stale_state() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("readloom-state.sqlite3");
        let valid_txt = directory.path().join("仍然存在.txt");
        let missing_txt = directory.path().join("已经移动.txt");
        std::fs::write(&valid_txt, "第一章\n有效正文。\n").expect("write valid txt");
        std::fs::write(&missing_txt, "第一章\n将被移动。\n").expect("write missing txt");
        let core = ReadloomCore::open(&database).expect("open core");
        core.open_txt(&valid_txt).expect("open valid txt");
        let missing_document = core.open_txt(&missing_txt).expect("open missing txt");
        core.save_text_locator(
            &missing_document,
            &missing_document.locator_for_paragraph(1, 0),
        )
        .expect("save stale progress");
        core.add_text_bookmark(&missing_document, 1)
            .expect("save stale bookmark");
        let before_cleaning = core.library_snapshot(50).expect("load unclean library");
        let valid_stored_path = before_cleaning
            .documents
            .iter()
            .find(|document| document.display_title == "仍然存在.txt")
            .expect("valid library document")
            .path
            .clone();
        let missing_stored_path = before_cleaning
            .documents
            .iter()
            .find(|document| document.display_title == "已经移动.txt")
            .expect("missing library document")
            .path
            .clone();
        std::fs::remove_file(&missing_txt).expect("simulate moved source book");

        assert_eq!(
            core.clean_invalid_library_entries().expect("clean invalid"),
            1
        );
        let snapshot = core.library_snapshot(50).expect("load cleaned library");
        assert_eq!(snapshot.documents.len(), 1);
        assert_eq!(snapshot.documents[0].path, valid_stored_path);
        drop(core);

        let connection = rusqlite::Connection::open(&database).expect("open state database");
        for (table, column) in [
            ("library_entries", "path"),
            ("reading_progress", "document_path"),
            ("bookmarks", "document_path"),
            ("recent_documents", "path"),
        ] {
            let count: i64 = connection
                .query_row(
                    &format!("SELECT COUNT(*) FROM {table} WHERE {column} = ?1"),
                    [&missing_stored_path],
                    |row| row.get(0),
                )
                .expect("count stale document state");
            assert_eq!(count, 0, "{table} should not retain stale document state");
        }
    }

    #[test]
    fn txt_reading_locator_round_trips_through_the_core_interface() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("readloom-state.sqlite3");
        let txt = directory.path().join("进度.txt");
        std::fs::write(&txt, "第一章\n第一段。\n目标段落。\n尾声\n结束。").expect("write txt");
        let core = ReadloomCore::open(&database).expect("open core");
        let document = core.open_txt(&txt).expect("open txt");
        let locator = document.locator_for_paragraph(2, 3);
        core.save_text_locator(&document, &locator)
            .expect("save locator");
        drop(core);

        let reopened_core = ReadloomCore::open(&database).expect("reopen core");
        let reopened_document = reopened_core.open_txt(&txt).expect("reopen txt");
        let restored = reopened_core
            .load_text_locator(&reopened_document)
            .expect("load locator")
            .expect("stored locator");

        assert_eq!(reopened_document.resolve_locator(&restored), 2);
        assert_eq!(restored.character_offset_in_paragraph, Some(3));
    }

    #[test]
    fn bookmarks_can_be_listed_navigated_and_deleted() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("readloom-state.sqlite3");
        let txt = directory.path().join("书签.txt");
        std::fs::write(&txt, "第一章\n第一段。\n第二段。\n").expect("write txt");
        let core = ReadloomCore::open(&database).expect("open core");
        let document = core.open_txt(&txt).expect("open txt");

        core.add_text_bookmark(&document, 2).expect("add bookmark");
        let bookmarks = core.bookmarks_for_path(&txt).expect("list bookmarks");

        assert_eq!(bookmarks.len(), 1);
        assert_eq!(bookmarks[0].chapter_title, "第一章");
        assert_eq!(bookmarks[0].paragraph_index, 2);
        assert!(
            core.delete_bookmark(&bookmarks[0].bookmark_id)
                .expect("delete bookmark")
        );
        assert!(
            core.bookmarks_for_path(&txt)
                .expect("list deleted bookmarks")
                .is_empty()
        );
    }

    #[test]
    fn core_opens_common_gbk_chinese_txt() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("readloom-state.sqlite3");
        let txt = directory.path().join("编码.txt");
        std::fs::write(
            &txt,
            [
                0xB5, 0xDA, 0xD2, 0xBB, 0xD5, 0xC2, 0x0A, 0xD6, 0xD0, 0xCE, 0xC4,
            ],
        )
        .expect("write GBK txt");
        let core = ReadloomCore::open(&database).expect("open core");

        let document = core.open_txt(&txt).expect("decode GBK txt");

        assert_eq!(document.paragraphs()[0].text, "第一章");
        assert_eq!(document.paragraphs()[1].text, "中文");
    }

    #[test]
    fn fifty_thousand_paragraph_txt_restores_a_deep_locator() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("readloom-state.sqlite3");
        let txt = directory.path().join("五万段.txt");
        let content = (0..50_000)
            .map(|index| format!("这是第 {index} 段用于原生长文本恢复验证。\n"))
            .collect::<String>();
        std::fs::write(&txt, content).expect("write long txt");
        let core = ReadloomCore::open(&database).expect("open core");
        let document = core.open_txt(&txt).expect("open long txt");
        assert_eq!(document.paragraphs().len(), 50_000);
        let locator = document.locator_for_paragraph(48_765, 5);
        core.save_text_locator(&document, &locator)
            .expect("save deep locator");
        drop(core);

        let reopened_core = ReadloomCore::open(&database).expect("reopen core");
        let reopened = reopened_core.open_txt(&txt).expect("reopen long txt");
        let restored = reopened_core
            .load_text_locator(&reopened)
            .expect("load locator")
            .expect("deep locator");

        assert_eq!(reopened.resolve_locator(&restored), 48_765);
        assert_eq!(restored.character_offset_in_paragraph, Some(5));
        assert_eq!(
            reopened.search("第 49999 段", 10)[0].paragraph_index,
            49_999
        );
    }

    #[test]
    fn text_before_the_first_heading_is_an_implicit_opening_chapter() {
        let document =
            ReaderDocument::from_text("开篇.txt", "作者的话。\n第一章 风起\n正文。".to_owned());

        assert_eq!(document.chapters()[0].title, "开篇");
        assert_eq!(document.chapters()[0].paragraph_index, 0);
        assert_eq!(document.chapters()[1].title, "第一章 风起");
        assert_eq!(document.paragraphs()[0].chapter_index, 0);
        assert_eq!(document.paragraphs()[1].chapter_index, 1);
    }

    #[test]
    fn txt_edit_preserves_utf8_bom_and_crlf() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("readloom-state.sqlite3");
        let txt = directory.path().join("编辑.txt");
        std::fs::write(&txt, b"\xEF\xBB\xBFfirst\r\nsecond\r\n").expect("write txt");
        let core = ReadloomCore::open(&database).expect("open core");
        let document = core.open_txt(&txt).expect("open txt");

        let saved = core
            .save_txt(
                &document,
                "first\nsecond changed\n",
                SaveTextOptions::PRESERVE,
            )
            .expect("save txt");
        let bytes = std::fs::read(&txt).expect("read saved txt");

        assert!(bytes.starts_with(b"\xEF\xBB\xBF"));
        assert!(bytes.ends_with(b"second changed\r\n"));
        assert_eq!(saved.content(), "first\nsecond changed\n");
        assert_eq!(saved.line_ending(), LineEnding::Crlf);
    }

    #[test]
    fn txt_edit_refuses_to_overwrite_external_changes() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("readloom-state.sqlite3");
        let txt = directory.path().join("冲突.txt");
        std::fs::write(&txt, "原内容").expect("write txt");
        let core = ReadloomCore::open(&database).expect("open core");
        let document = core.open_txt(&txt).expect("open txt");
        std::fs::write(&txt, "外部修改").expect("external change");

        let error = core
            .save_txt(&document, "编辑内容", SaveTextOptions::PRESERVE)
            .expect_err("conflict should fail");

        assert!(error.to_string().contains("其他程序修改"));
        assert_eq!(
            std::fs::read_to_string(&txt).expect("read current"),
            "外部修改"
        );
    }

    #[test]
    fn app_settings_round_trip_through_the_existing_preferences_table() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let database = directory.path().join("readloom-state.sqlite3");
        let core = ReadloomCore::open(&database).expect("open core");
        let mut settings = AppSettings {
            theme: AppTheme::Dark,
            library_columns: 5,
            ..AppSettings::default()
        };
        settings.reading.font_size = 23;
        settings.txt.leading_indent = TxtLeadingIndent::Preserve;
        core.save_settings(&settings).expect("save settings");
        drop(core);

        let reopened = ReadloomCore::open(&database).expect("reopen core");
        let loaded = reopened.load_settings().expect("load settings");

        assert_eq!(loaded.theme, AppTheme::Dark);
        assert_eq!(loaded.library_columns, 5);
        assert_eq!(loaded.reading.font_size, 23);
        assert_eq!(loaded.txt.leading_indent, TxtLeadingIndent::Preserve);
    }

    #[test]
    fn txt_layout_options_change_the_closed_reading_model_not_the_source_content() {
        let mut txt = TxtSettings {
            leading_indent: TxtLeadingIndent::Preserve,
            blank_lines: TxtBlankLines::Single,
            merge_wrapped_lines: true,
            ..TxtSettings::default()
        };
        let source = "自定义标题\n  第一行没有句号\n第二行继续。\n\n\n结尾。".to_owned();
        let document = ReaderDocument::from_text_with_settings(
            "排版.txt",
            source.clone(),
            &txt,
            "^自定义标题$",
        );

        assert_eq!(document.content(), source);
        assert_eq!(document.chapters()[0].title, "自定义标题");
        assert_eq!(
            document.paragraphs()[1].text,
            "  第一行没有句号第二行继续。"
        );
        assert_eq!(
            document
                .paragraphs()
                .iter()
                .filter(|paragraph| paragraph.kind == ParagraphKind::Blank)
                .count(),
            1
        );

        txt.blank_lines = TxtBlankLines::Remove;
        let hidden =
            ReaderDocument::from_text_with_settings("排版.txt", source, &txt, "^自定义标题$");
        assert!(
            hidden
                .paragraphs()
                .iter()
                .all(|paragraph| paragraph.kind != ParagraphKind::Blank)
        );
    }

    #[test]
    fn background_image_is_validated_copied_and_cleared_through_core() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3")).expect("open core");
        let source = directory.path().join("background.png");
        std::fs::write(&source, b"\x89PNG\r\n\x1a\nminimal-test-payload").expect("write image");

        let stored = core
            .set_background_image(&source)
            .expect("store background");
        assert!(stored.is_file());
        assert_eq!(core.background_image_path().unwrap(), Some(stored.clone()));
        core.clear_background_image().expect("clear background");
        assert_eq!(core.background_image_path().unwrap(), None);
        assert!(!stored.exists());

        let invalid = directory.path().join("invalid.png");
        std::fs::write(&invalid, b"not-an-image").unwrap();
        assert!(
            core.set_background_image(&invalid)
                .unwrap_err()
                .to_string()
                .contains("格式无效")
        );
    }

    #[test]
    fn epub_image_import_validates_content_and_dimensions_before_editing() {
        let directory = tempfile::tempdir().expect("temporary directory");
        let core = ReadloomCore::open(&directory.path().join("state.sqlite3")).expect("open core");
        let png = directory.path().join("illustration.not-really-png");
        image::RgbaImage::from_pixel(1, 1, image::Rgba([12, 34, 56, 255]))
            .save_with_format(&png, image::ImageFormat::Png)
            .expect("write valid PNG");

        let asset = core
            .validate_epub_image(&png)
            .expect("validate EPUB image by content");

        assert_eq!(asset.media_type, ImageMediaType::Png);
        assert_eq!((asset.width, asset.height), (1, 1));
        assert!(!asset.digest.is_empty());

        let invalid = directory.path().join("spoofed.png");
        std::fs::write(&invalid, b"not an image").expect("write spoofed image");
        assert!(
            core.validate_epub_image(&invalid)
                .expect_err("reject spoofed image")
                .to_string()
                .contains("图片")
        );
    }
}
