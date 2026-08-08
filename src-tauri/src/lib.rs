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
mod stage1_tracer_tests;

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
    tauri::Builder::default()
        .manage(StartupState::new())
        .manage(
            application::text_document_service::TextDocumentService::new(
                config::TextDocumentLimits::stage1_default(),
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
        .manage(
            application::epub_document_service::EpubDocumentService::new(
                infrastructure::archive::archive_limits::ArchiveLimits::stage3_default(),
            ),
        )
        .register_asynchronous_uri_scheme_protocol(
            "readloom-epub",
            |context, request, responder| {
                let app = context.app_handle().clone();
                let webview_label = context.webview_label().to_owned();
                std::thread::spawn(move || {
                    let state =
                        app.state::<application::epub_document_service::EpubDocumentService>();
                    responder.respond(security::epub_protocol::handle_epub_protocol(
                        state.inner(),
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
            commands::epub_commands::open_epub_document,
            commands::epub_commands::close_epub_document,
            commands::epub_commands::save_epub_progress,
            commands::epub_commands::save_epub_bookmark,
            commands::epub_commands::delete_epub_bookmark,
            commands::epub_commands::search_epub_document,
            commands::epub_commands::cancel_epub_search,
            commands::epub_commands::list_recent_documents
        ])
        .run(tauri::generate_context!())
}
