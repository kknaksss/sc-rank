pub mod blog;
pub mod blog_browser;
pub mod browser;
pub mod commands;
pub mod errors;
pub mod gate;
pub mod js;
pub mod place;
pub mod workbook;

use tauri::Manager;
use tauri_plugin_log::{Target, TargetKind};

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    // 비정상 종료로 남은 임시 프로필은 다음 기동 때 지운다 (SPEC §3).
    browser::remove_stale_profiles();
    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .clear_targets()
                .target(Target::new(TargetKind::LogDir {
                    file_name: Some("sc-rank".into()),
                }))
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .manage(commands::AppState::default())
        .invoke_handler(tauri::generate_handler![
            commands::check_blog,
            commands::check_place,
            commands::export_xlsx
        ])
        .build(tauri::generate_context!())
        .expect("error while building SC Rank")
        .run(|app, event| {
            // 마지막 창이 닫히면 앱이 끝난다 — 브라우저를 끝내고 임시 프로필을 지운다.
            if let tauri::RunEvent::Exit = event {
                let state = app.state::<commands::AppState>();
                tauri::async_runtime::block_on(state.browser.shutdown());
            }
        });
}
