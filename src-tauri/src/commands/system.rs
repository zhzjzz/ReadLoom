use std::{
    fs::OpenOptions,
    io::Write,
    path::Path,
    sync::atomic::{AtomicBool, Ordering},
    time::{SystemTime, UNIX_EPOCH},
};

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::{StartupState, error::AppError};

const MAX_PROBE_MESSAGE_CHARS: usize = 128;
pub(crate) const TRAY_ID: &str = "readloom-tray";

#[derive(Default)]
pub(crate) struct WindowBehaviorState {
    minimize_to_tray: AtomicBool,
}

impl WindowBehaviorState {
    pub(crate) fn minimize_to_tray(&self) -> bool {
        self.minimize_to_tray.load(Ordering::Relaxed)
    }
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ApplyWindowBehaviorRequest {
    tray_visible: bool,
    minimize_to_tray: bool,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SystemProbeRequest {
    message: String,
    client_timestamp_ms: u64,
}

#[derive(Debug, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct SystemProbeDto {
    app_name: &'static str,
    app_version: &'static str,
    platform: &'static str,
    architecture: &'static str,
    protocol_version: u16,
    echoed_message: String,
    client_timestamp_ms: u64,
    server_timestamp_ms: u64,
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StartupMetricsDto {
    process_id: u32,
    main_to_frontend_ready_ms: u64,
    recorded_at_unix_ms: u64,
}

#[tauri::command]
pub fn system_probe(request: SystemProbeRequest) -> Result<SystemProbeDto, AppError> {
    let message = request.message.trim();
    if message.is_empty() {
        return Err(AppError::validation(
            "INPUT_EMPTY",
            "通信测试内容不能为空。",
            "请输入测试内容后重试。",
        ));
    }

    if message.chars().count() > MAX_PROBE_MESSAGE_CHARS {
        return Err(AppError::validation(
            "INPUT_TOO_LONG",
            "通信测试内容过长。",
            "请将内容缩短到 128 个字符以内。",
        ));
    }

    Ok(SystemProbeDto {
        app_name: "Readloom",
        app_version: env!("CARGO_PKG_VERSION"),
        platform: std::env::consts::OS,
        architecture: std::env::consts::ARCH,
        protocol_version: 1,
        echoed_message: message.to_owned(),
        client_timestamp_ms: request.client_timestamp_ms,
        server_timestamp_ms: unix_time_ms(),
    })
}

#[tauri::command]
pub fn frontend_ready(state: State<'_, StartupState>) -> Result<StartupMetricsDto, AppError> {
    let metrics = StartupMetricsDto {
        process_id: std::process::id(),
        main_to_frontend_ready_ms: state.elapsed_ms(),
        recorded_at_unix_ms: unix_time_ms(),
    };

    if let Ok(output_path) = std::env::var("READLOOM_BASELINE_OUTPUT") {
        write_benchmark_marker(Path::new(&output_path), &metrics)?;
    }

    Ok(metrics)
}

#[tauri::command]
pub fn apply_window_behavior(
    app: AppHandle,
    state: State<'_, WindowBehaviorState>,
    request: ApplyWindowBehaviorRequest,
) -> Result<(), AppError> {
    state
        .minimize_to_tray
        .store(request.minimize_to_tray, Ordering::Relaxed);
    let tray = app
        .tray_by_id(TRAY_ID)
        .ok_or_else(|| AppError::internal("TRAY_UNAVAILABLE", "find Readloom tray icon"))?;
    tray.set_visible(request.tray_visible)
        .map_err(|_| AppError::internal("TRAY_UNAVAILABLE", "update Readloom tray icon"))
}

fn unix_time_ms() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis().try_into().unwrap_or(u64::MAX))
        .unwrap_or_default()
}

fn write_benchmark_marker(path: &Path, metrics: &StartupMetricsDto) -> Result<(), AppError> {
    if path.extension().and_then(|extension| extension.to_str()) != Some("json") {
        return Err(AppError::validation(
            "INVALID_METRIC_PATH",
            "启动基线输出路径必须使用 .json 扩展名。",
            "更换输出路径后重试。",
        ));
    }

    let payload = serde_json::to_vec(metrics)
        .map_err(|_| AppError::internal("METRIC_SERIALIZE_FAILED", "serialize startup marker"))?;
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map_err(|_| AppError::internal("METRIC_WRITE_FAILED", "create startup marker"))?;

    file.write_all(&payload)
        .and_then(|_| file.sync_all())
        .map_err(|_| AppError::internal("METRIC_WRITE_FAILED", "flush startup marker"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn probe_round_trips_trimmed_unicode_text() {
        let response = system_probe(SystemProbeRequest {
            message: "  阅织通信正常  ".to_owned(),
            client_timestamp_ms: 42,
        })
        .expect("valid probe request");

        assert_eq!(response.echoed_message, "阅织通信正常");
        assert_eq!(response.client_timestamp_ms, 42);
        assert_eq!(response.protocol_version, 1);
    }

    #[test]
    fn probe_rejects_blank_text_with_typed_error() {
        let error = system_probe(SystemProbeRequest {
            message: "   ".to_owned(),
            client_timestamp_ms: 0,
        })
        .expect_err("blank probe must be rejected");

        assert_eq!(error.to_dto().code, "INPUT_EMPTY");
    }

    #[test]
    fn probe_counts_unicode_characters_instead_of_bytes() {
        let response = system_probe(SystemProbeRequest {
            message: "阅".repeat(MAX_PROBE_MESSAGE_CHARS),
            client_timestamp_ms: 0,
        });

        assert!(response.is_ok());
    }
}
