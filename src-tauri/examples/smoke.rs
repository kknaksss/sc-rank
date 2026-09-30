//! 실수집 확인 (SPEC-001 §6, PoC `scripts/smoke.mjs` 대응). 앱과 같은 수집 함수를 부른다.
//!
//!   cargo run --example smoke -- place <키워드> <타겟 병원명>
//!   cargo run --example smoke -- blog <키워드> keyword <타겟 키워드>
//!   cargo run --example smoke -- blog <키워드> image <이미지 파일>
//!
//! 단계 로그와 요약 JSON 을 표준 출력에 낸다. `status == "error"` 면 exit 1. 끝나면 브라우저를 정리한다.

use std::path::Path;
use std::process::ExitCode;

use sc_rank_lib::blog::{check_blog, BlogInput};
use sc_rank_lib::browser::{remove_stale_profiles, BrowserManager};
use sc_rank_lib::place::{check_place, validate_place_input};
use serde_json::Value;

struct Stdout;

impl log::Log for Stdout {
    fn enabled(&self, m: &log::Metadata) -> bool {
        m.level() <= log::Level::Info && m.target().starts_with("sc_rank_lib")
    }
    fn log(&self, record: &log::Record) {
        if self.enabled(record.metadata()) {
            println!("{}", record.args());
        }
    }
    fn flush(&self) {}
}

static LOGGER: Stdout = Stdout;

const USAGE: &str = "usage: smoke place <keyword> <target> | smoke blog <keyword> keyword <target> | smoke blog <keyword> image <file>";

#[tokio::main]
async fn main() -> ExitCode {
    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(log::LevelFilter::Info));
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    remove_stale_profiles();
    let browser = BrowserManager::default();
    let summary = match args.as_slice() {
        ["place", keyword, target] => run_place(&browser, keyword, target).await,
        ["blog", keyword, "keyword", target] => {
            let input = BlogInput::new(
                Some((*keyword).into()),
                Some("keyword".into()),
                Some((*target).into()),
                None,
                Some("".into()),
            );
            Ok(serde_json::to_value(check_blog(&browser, &input).await).expect("serialize"))
        }
        ["blog", keyword, "image", file] => match image_input(keyword, file) {
            Ok(input) => {
                Ok(serde_json::to_value(check_blog(&browser, &input).await).expect("serialize"))
            }
            Err(e) => Err(e),
        },
        _ => Err(USAGE.to_string()),
    };
    browser.shutdown().await;
    match summary {
        Ok(result) => {
            println!("{}", serde_json::to_string_pretty(&result).expect("json"));
            if result.get("status").and_then(Value::as_str) == Some("error") {
                ExitCode::FAILURE
            } else {
                ExitCode::SUCCESS
            }
        }
        Err(message) => {
            eprintln!("{message}");
            ExitCode::FAILURE
        }
    }
}

async fn run_place(browser: &BrowserManager, keyword: &str, target: &str) -> Result<Value, String> {
    let (keyword, target) = validate_place_input(Some(&keyword.into()), Some(&target.into()))?;
    let mut result =
        serde_json::to_value(check_place(browser, &keyword, &target).await).expect("serialize");
    // smoke.mjs: const { rows, ...summary } = result
    if let Some(obj) = result.as_object_mut() {
        obj.remove("rows");
    }
    Ok(result)
}

/// 화면의 파일 선택과 같은 입력 — dataURL + 파일 이름.
fn image_input(keyword: &str, file: &str) -> Result<BlogInput, String> {
    let bytes = std::fs::read(file).map_err(|e| format!("{file}: {e}"))?;
    let mime = match image::guess_format(&bytes) {
        Ok(image::ImageFormat::Png) => "image/png",
        Ok(image::ImageFormat::Jpeg) => "image/jpeg",
        Ok(image::ImageFormat::WebP) => "image/webp",
        _ => return Err(format!("{file}: PNG, JPG, WebP 만 지원합니다")),
    };
    let name = Path::new(file)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let data = format!("data:{mime};base64,{}", base64(&bytes));
    Ok(BlogInput::new(
        Some(keyword.into()),
        Some("image".into()),
        Some("".into()),
        Some(data.into()),
        Some(name.into()),
    ))
}

fn base64(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            s.push(if i <= chunk.len() {
                T[((n >> (18 - 6 * i)) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    s
}
