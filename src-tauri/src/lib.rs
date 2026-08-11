#[cfg(all(not(debug_assertions), not(feature = "custom-protocol")))]
compile_error!(
    "Readloom release builds require the `custom-protocol` feature; use `npm run build:exe`."
);

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

use tauri::{
    Emitter, Manager, WindowEvent,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
};

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
        .manage(commands::system::WindowBehaviorState::default())
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
            let show = MenuItem::with_id(app, "tray-show", "显示 Readloom", true, None::<&str>)?;
            let quit = MenuItem::with_id(app, "tray-quit", "退出", true, None::<&str>)?;
            let menu = Menu::with_items(app, &[&show, &quit])?;
            let mut tray = TrayIconBuilder::with_id(commands::system::TRAY_ID)
                .menu(&menu)
                .show_menu_on_left_click(false)
                .tooltip("Readloom 阅织")
                .on_menu_event(|app, event| match event.id().as_ref() {
                    "tray-show" => show_main_window(app),
                    "tray-quit" => {
                        let _ = app.emit("readloom-request-exit", ());
                    }
                    _ => {}
                })
                .on_tray_icon_event(|tray, event| {
                    if matches!(
                        event,
                        TrayIconEvent::Click {
                            button: MouseButton::Left,
                            button_state: MouseButtonState::Up,
                            ..
                        }
                    ) {
                        show_main_window(tray.app_handle());
                    }
                });
            if let Some(icon) = app.default_window_icon().cloned() {
                tray = tray.icon(icon);
            }
            let tray = tray.build(app)?;
            tray.set_visible(false)?;
            Ok(())
        })
        .on_window_event(|window, event| {
            if matches!(event, WindowEvent::Resized(_))
                && window
                    .state::<commands::system::WindowBehaviorState>()
                    .minimize_to_tray()
                && window.is_minimized().unwrap_or(false)
            {
                let _ = window.hide();
            }
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
        .register_asynchronous_uri_scheme_protocol(
            "readloom-background",
            |context, request, responder| {
                let app = context.app_handle().clone();
                let webview_label = context.webview_label().to_owned();
                std::thread::spawn(move || {
                    let local_state =
                        app.state::<infrastructure::storage::local_state::LocalStateStore>();
                    responder.respond(security::background_protocol::handle_background_protocol(
                        local_state.inner(),
                        &webview_label,
                        request,
                    ));
                });
            },
        )
        .register_asynchronous_uri_scheme_protocol(
            "readloom-library",
            move |context, request, responder| {
                let app = context.app_handle().clone();
                let webview_label = context.webview_label().to_owned();
                std::thread::spawn(move || {
                    let local_state =
                        app.state::<infrastructure::storage::local_state::LocalStateStore>();
                    responder.respond(
                        security::library_cover_protocol::handle_library_cover_protocol(
                            local_state.inner(),
                            epub_limits,
                            &webview_label,
                            request,
                        ),
                    );
                });
            },
        )
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::system::system_probe,
            commands::system::frontend_ready,
            commands::system::apply_window_behavior,
            commands::appearance::get_background_image,
            commands::appearance::set_background_image,
            commands::appearance::clear_background_image,
            commands::backup_commands::create_books_backup,
            commands::backup_commands::restore_books_backup,
            commands::text_document::open_text_document,
            commands::text_document::reopen_text_document,
            commands::text_document::save_text_document,
            commands::text_document::save_text_document_as,
            commands::text_document::close_text_document,
            commands::text_document::save_text_bookmark,
            commands::text_document::delete_text_bookmark,
            commands::text_document::save_text_progress,
            commands::epub_commands::open_epub_document,
            commands::epub_commands::close_epub_document,
            commands::epub_commands::save_epub_progress,
            commands::epub_commands::save_epub_bookmark,
            commands::epub_commands::delete_epub_bookmark,
            commands::epub_commands::search_epub_document,
            commands::epub_commands::cancel_epub_search,
            commands::epub_commands::list_recent_documents,
            commands::epub_commands::delete_recent_document,
            commands::library_commands::list_library,
            commands::library_commands::import_library_documents,
            commands::library_commands::preview_library_documents,
            commands::library_commands::preview_library_directory,
            commands::library_commands::create_library_group,
            commands::library_commands::rename_library_group,
            commands::library_commands::delete_library_group,
            commands::library_commands::assign_library_group,
            commands::library_commands::remove_library_document,
            commands::library_commands::remove_unavailable_library_documents,
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

fn show_main_window<R: tauri::Runtime>(app: &tauri::AppHandle<R>) {
    if let Some(window) = app.get_webview_window("main") {
        let _ = window.show();
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}
