//! 플레이스 반환 모양·입력 검증 — PoC `place.mjs:111-118` · `index.mjs:35` 대조.

use sc_rank_lib::errors::{ScrapeError, PLACE_FALLBACK};
use sc_rank_lib::place::{
    parse_place_list, place_result, validate_place_input, PlaceItem, INPUT_MESSAGE, PLACE_SCOPE,
};
use serde_json::{json, Value};

#[test]
fn 입력_검증은_trim_하고_길이를_본다() {
    assert_eq!(
        validate_place_input(Some(&json!(" 강남 ")), Some(&json!(" 무이 "))).unwrap(),
        ("강남".into(), "무이".into())
    );
    for (k, t) in [
        (json!(""), json!("t")),
        (json!("k"), json!("  ")),
        (json!("가".repeat(101)), json!("t")),
        (json!("k"), json!("가".repeat(51))),
        (json!(1), json!("t")),
    ] {
        assert_eq!(
            validate_place_input(Some(&k), Some(&t)).unwrap_err(),
            INPUT_MESSAGE
        );
    }
    assert_eq!(
        validate_place_input(None, Some(&json!("t"))).unwrap_err(),
        INPUT_MESSAGE
    );
}

#[test]
fn 반환_모양() {
    let item = PlaceItem {
        name: Some("무이성형외과의원".into()),
        id: Some("123456".into()),
        ..Default::default()
    };
    let ok = place_result(
        "강남역성형외과",
        "무이",
        parse_place_list(&[item], "무이"),
        "T".into(),
    );
    let v = serde_json::to_value(&ok).unwrap();
    assert_eq!(v["searchUrl"], json!("https://map.naver.com/p/search/%EA%B0%95%EB%82%A8%EC%97%AD%EC%84%B1%ED%98%95%EC%99%B8%EA%B3%BC?searchType=place"));
    assert_eq!(v["scope"], json!(PLACE_SCOPE));
    assert_eq!(
        (
            v["status"].clone(),
            v["rank"].clone(),
            v["organicRank"].clone(),
            v["page"].clone()
        ),
        (json!("found"), json!(1), json!(1), json!(1))
    );
    assert_eq!(v["message"], json!(""));
    assert_eq!(v["rows"][0]["reviews"], Value::Null);
    let nf = place_result(
        "k",
        "없음",
        parse_place_list(
            &[PlaceItem {
                name: Some("다른병원".into()),
                ..Default::default()
            }],
            "없음",
        ),
        "T".into(),
    );
    assert_eq!(nf.message, "수집한 최대 5페이지 목록에서 찾지 못했습니다.");
    let err = place_result(
        "k",
        "t",
        Err(ScrapeError::other(anyhow::anyhow!(
            "Timeout 30000ms exceeded"
        ))),
        "T".into(),
    );
    let v = serde_json::to_value(&err).unwrap();
    for key in ["rank", "organicRank", "page", "total", "totalAds"] {
        assert_eq!(v[key], Value::Null, "{key}");
    }
    assert_eq!(
        (v["matches"].clone(), v["rows"].clone(), v["status"].clone()),
        (json!([]), json!([]), json!("error"))
    );
    assert_eq!(v["message"], json!(PLACE_FALLBACK));
    let domain = place_result(
        "k",
        "t",
        Err(ScrapeError::domain(
            "플레이스 검색 프레임을 읽지 못했습니다.",
        )),
        "T".into(),
    );
    assert_eq!(domain.message, "플레이스 검색 프레임을 읽지 못했습니다.");
}
