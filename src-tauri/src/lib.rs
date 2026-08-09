mod application;
mod commands;
mod config;
mod domain;
mod error;
mod formats;
mod infrastructure;
mod security;

#[cfg(test)]
mod epub_test_fixtures;
#[cfg(test)]
mod text_document_tracer_tests;

use std::time::Instant;

use tauri::Manager;

pub(crate) struct StartupState {
    started_at: Instant,
}

impl StartupState {
    fn new() -> Self {
        Self {
            started_at: Instant::now(),
        }
    }

    pub(crate) fn elapsed_ms(&self) -> u64 {
        self.started_at
            .elapsed()
            .as_millis()
            .try_into()
            .unwrap_or(u64::MAX)
    }
}

pub fn run() -> Result<(), tauri::Error> {
    let epub_limits = infrastructure::archive::archive_limits::ArchiveLimits::default();
    let epub_documents = application::epub_document_service::EpubDocumentService::new(epub_limits);
    let epub_edits =
        application::epub_edit_service::EpubEditService::new(epub_limits, epub_documents.clone());
    tauri::Builder::default()
        .manage(StartupState::new())
        .manage(
            application::text_document_service::TextDocumentService::new(
                config::TextDocumentLimits::default(),
            ),
        )
        .setup(|app| {
            let app_data = app.path().app_data_dir()?;
            std::fs::create_dir_all(&app_data)?;
            app.manage(infrastructure::storage::local_state::LocalStateStore::open(
                &app_data.join("readloom-state.sqlite3"),
            )?);
            Ok(())
        })
        .manage(epub_documents)
        .manage(epub_edits)
        .register_asynchronous_uri_scheme_protocol(
            "readloom-epub",
            |context, request, responder| {
                let app = context.app_handle().clone();
                let webview_label = context.webview_label().to_owned();
                std::thread::spawn(move || {
                    let state =
                        app.state::<application::epub_document_service::EpubDocumentService>();
                    let edits = app.state::<application::epub_edit_service::EpubEditService>();
                    responder.respond(security::epub_protocol::handle_epub_protocol(
                        state.inner(),
                        edits.inner(),
                        &webview_label,
                        request,
                    ));
                });
            },
        )
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::system::system_probe,
            commands::system::frontend_ready,
            commands::text_document::open_text_document,
            commands::text_document::reopen_text_document,
            commands::text_document::save_text_document,
            commands::text_document::save_text_document_as,
            commands::text_document::close_text_document,
            commands::text_document::save_text_bookmark,
            commands::text_document::delete_text_bookmark,
            commands::epub_commands::open_epub_document,
            commands::epub_commands::close_epub_document,
            commands::epub_commands::save_epub_progress,
            commands::epub_commands::save_epub_bookmark,
            commands::epub_commands::delete_epub_bookmark,
            commands::epub_commands::search_epub_document,
            commands::epub_commands::cancel_epub_search,
            commands::epub_commands::list_recent_documents,
            commands::epub_commands::delete_recent_document,
            commands::epub_edit_commands::begin_epub_edit,
            commands::epub_edit_commands::get_epub_edit_draft,
            commands::epub_edit_commands::update_epub_metadata,
            commands::epub_edit_commands::replace_epub_cover,
            commands::epub_edit_commands::remove_epub_cover_change,
            commands::epub_edit_commands::analyze_epub_chapter_editability,
            commands::epub_edit_commands::begin_epub_chapter_edit,
            commands::epub_edit_commands::update_epub_chapter_draft,
            commands::epub_edit_commands::flush_epub_chapter_draft,
            commands::epub_edit_commands::validate_epub_chapter_draft,
            commands::epub_edit_commands::revert_epub_chapter_draft,
            commands::epub_edit_commands::close_epub_chapter_edit,
            commands::epub_edit_commands::import_epub_chapter_image,
            commands::epub_edit_commands::validate_epub_draft,
            commands::epub_edit_commands::prepare_epub_overwrite_confirmation,
            commands::epub_edit_commands::save_epub_as,
            commands::epub_edit_commands::cancel_epub_save,
            commands::epub_edit_commands::discard_epub_draft,
            commands::epub_edit_commands::close_epub_edit_session
        ])
        .run(tauri::generate_context!())
}
