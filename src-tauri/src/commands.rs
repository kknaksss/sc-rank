//! Tauri 명령 셋 (SPEC-001 §2). 인자는 평평하게, JS 쪽 camelCase 키가 그대로 들어온다.

use serde::Serialize;
use serde_json::Value;
use tauri::{AppHandle, State};
use tauri_plugin_dialog::DialogExt;

use crate::blog::{self, BlogInput, BlogResult};
use crate::browser::BrowserManager;
use crate::gate::{Gate, BLOG_GAP, PLACE_GAP};
use crate::place::{self, PlaceResult};
use crate::workbook::{self, Mode, WORKBOOK_MESSAGE};

/// 앱 상태 — 브라우저 1개와 공유 잠금.
#[derive(Default)]
pub struct AppState {
    pub browser: BrowserManager,
    pub gate: Gate,
}

/// `export_xlsx` 반환 — `{ saved: true, path }` 또는 `{ saved: false }`(대화상자 취소).
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct ExportResult {
    pub saved: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub path: Option<String>,
}

/// `invoke('check_blog', { keyword, targetType, target, imageData, imageName })` — `POST /api/blog/check`.
/// Err 는 진행 중·간격 미달뿐. 입력 검증은 `status: "error"` 정상 반환.
#[tauri::command]
pub async fn check_blog(
    state: State<'_, AppState>,
    keyword: Option<Value>,
    target_type: Option<Value>,
    target: Option<Value>,
    image_data: Option<Value>,
    image_name: Option<Value>,
) -> Result<BlogResult, String> {
    let _pass = state.gate.enter(BLOG_GAP)?;
    let input = BlogInput::new(keyword, target_type, target, image_data, image_name);
    Ok(blog::check_blog(&state.browser, &input).await)
}

/// `invoke('check_place', { keyword, target })` — `POST /api/place/check`.
/// 입력 검증 Err 가 잠금 검사보다 먼저다(간격 갱신 없음).
#[tauri::command]
pub async fn check_place(
    state: State<'_, AppState>,
    keyword: Option<Value>,
    target: Option<Value>,
) -> Result<PlaceResult, String> {
    let (keyword, target) = place::validate_place_input(keyword.as_ref(), target.as_ref())?;
    let _pass = state.gate.enter(PLACE_GAP)?;
    Ok(place::check_place(&state.browser, &keyword, &target).await)
}

/// `invoke('export_xlsx', { rows, mode })` — `POST /api/export` + 저장 대화상자.
/// 행은 느슨하게 받는다(SPEC §2.2). Err 는 입력 검증(`index.mjs:45`)과 파일 생성·쓰기 실패(`index.mjs:51`).
#[tauri::command]
pub async fn export_xlsx(
    app: AppHandle,
    rows: Option<Value>,
    mode: Option<Value>,
) -> Result<ExportResult, String> {
    let rows = workbook::validate_rows(rows.as_ref())?;
    let bytes = workbook::make_workbook(rows, Mode::from_value(mode.as_ref())).map_err(|e| {
        log::error!("[export] workbook: {e}");
        WORKBOOK_MESSAGE.to_string()
    })?;
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.dialog()
        .file()
        .add_filter("Excel", &["xlsx"])
        .set_file_name(workbook::default_file_name(chrono::Utc::now()))
        .save_file(move |picked| {
            let _ = tx.send(picked);
        });
    let picked = rx.await.map_err(|e| {
        log::error!("[export] dialog: {e}");
        WORKBOOK_MESSAGE.to_string()
    })?;
    // 취소 — 화면은 메시지를 내지 않는다.
    let Some(picked) = picked else {
        return Ok(ExportResult {
            saved: false,
            path: None,
        });
    };
    let path = picked.into_path().map_err(|e| {
        log::error!("[export] path: {e}");
        WORKBOOK_MESSAGE.to_string()
    })?;
    std::fs::write(&path, bytes).map_err(|e| {
        log::error!("[export] write {}: {e}", path.display());
        WORKBOOK_MESSAGE.to_string()
    })?;
    let path = path.display().to_string();
    log::info!(
        "[export] {}",
        serde_json::json!({ "saved": true, "path": path })
    );
    Ok(ExportResult {
        saved: true,
        path: Some(path),
    })
}
