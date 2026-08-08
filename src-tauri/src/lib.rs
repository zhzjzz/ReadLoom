mod application;
mod commands;
mod config;
mod domain;
mod error;
mod formats;
mod infrastructure;

#[cfg(test)]
mod stage1_tracer_tests;

use std::time::Instant;

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
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::system::system_probe,
            commands::system::frontend_ready,
            commands::text_document::open_text_document,
            commands::text_document::reopen_text_document,
            commands::text_document::save_text_document,
            commands::text_document::save_text_document_as,
            commands::text_document::close_text_document
        ])
        .run(tauri::generate_context!())
}
