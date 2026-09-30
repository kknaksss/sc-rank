//! 네트워크·브라우저 없이 확인하는 P3 규칙.

use std::path::PathBuf;

use sc_rank_lib::blog::{check_blog, BlogInput};
use sc_rank_lib::browser::{BrowserManager, NOT_FOUND_MESSAGE};
use sc_rank_lib::place::check_place;
use serde_json::{json, Value};

fn read(rel: &str) -> String {
    std::fs::read_to_string(PathBuf::from(env!("CARGO_MANIFEST_DIR")).join(rel)).unwrap()
}

/// 페이지 안 JS 는 PoC 문자열 그대로 주입한다 (SPEC §4).
#[test]
fn 주입_js_는_poc_원문과_같다() {
    assert_eq!(read("js/place-dom.js"), read("../../server/place-dom.mjs"));
    let place = read("../../server/place.mjs");
    for f in [
        "js/place-items.js",
        "js/place-signature.js",
        "js/place-next.js",
    ] {
        let snippet = read(f);
        assert!(
            place.contains(snippet.trim()),
            "{f} 가 place.mjs 에 그대로 없다"
        );
    }
    let blog = read("../../server/blog-browser.mjs");
    for f in ["js/blog-scroll.js", "js/blog-rendered.js"] {
        let snippet = read(f);
        assert!(
            blog.contains(snippet.trim()),
            "{f} 가 blog-browser.mjs 에 그대로 없다"
        );
    }
}

/// 브라우저가 없으면 해당 조회만 `status: "error"` + §3 문구 (A-7 명령 경로).
#[tokio::test]
async fn 브라우저_없음은_조회_오류_행이_된다() {
    let browser = BrowserManager::new(Vec::new());
    let place = check_place(&browser, "강남역성형외과", "무이성형외과").await;
    assert_eq!(
        (place.status.as_str(), place.message.as_str()),
        ("error", NOT_FOUND_MESSAGE)
    );
    assert_eq!((place.total, place.rows.len()), (None, 0));

    let input = BlogInput::new(
        Some(json!("구월동레이저제모")),
        Some(json!("keyword")),
        Some(json!("썸블리의원")),
        None,
        Some(json!("")),
    );
    let blog = serde_json::to_value(check_blog(&browser, &input).await).unwrap();
    assert_eq!(blog["status"], json!("error"));
    assert_eq!(blog["message"], json!(NOT_FOUND_MESSAGE));
    assert!(blog.get("scrolls").is_none());
    browser.shutdown().await;
}

/// 블로그 입력 검증은 브라우저 전에 `status: "error"` 로 돌아간다 (`blog.mjs:74-77`).
#[tokio::test]
async fn 블로그_입력_검증은_브라우저_앞() {
    let browser = BrowserManager::new(Vec::new());
    let input = BlogInput::new(Some(json!(" ")), None, None, None, None);
    let r = serde_json::to_value(check_blog(&browser, &input).await).unwrap();
    assert_eq!(
        r["message"],
        json!("검색 키워드는 1~100자로 입력해 주세요.")
    );
    assert_eq!(r["target"], json!(""));
    assert_eq!(r["targetType"], json!("keyword"));
    let image = BlogInput::new(
        Some(json!("k")),
        Some(json!("image")),
        None,
        Some(json!("data:image/gif;base64,AAAA")),
        None,
    );
    let r = serde_json::to_value(check_blog(&browser, &image).await).unwrap();
    assert_eq!(
        r["message"],
        json!("PNG, JPG, WebP 이미지를 선택해 주세요.")
    );
    assert_eq!(r["target"], Value::from("업로드 이미지"));
}
