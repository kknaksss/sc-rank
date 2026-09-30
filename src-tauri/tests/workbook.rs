//! 엑셀 열·타입 (A-2) — PoC `workbook.mjs` 대조.

mod common;

use calamine::Data;
use sc_rank_lib::workbook::{default_file_name, make_workbook, validate_rows, Mode, ROWS_MESSAGE};
use serde_json::{json, Value};

fn headers(s: &common::Sheet) -> Vec<String> {
    (0..s.range.width() as u32)
        .map(|c| match s.range.get_value((0, c)) {
            Some(Data::String(h)) => h.clone(),
            other => panic!("header {c} = {other:?}"),
        })
        .collect()
}

#[test]
fn 블로그_11열_머리와_순서() {
    let b = make_workbook(&[json!({ "keyword": "k" })], Mode::Blog).unwrap();
    let s = common::read_sheet(&b);
    assert_eq!(
        headers(&s),
        [
            "키워드",
            "상태",
            "노출 순위",
            "확인 글 수",
            "타겟",
            "매칭 글 제목",
            "매칭 글 URL",
            "조회 시각 (한국)",
            "조회 기준",
            "검색 URL",
            "안내"
        ]
    );
}

#[test]
fn 플레이스_13열_머리와_순서() {
    let b = make_workbook(&[json!({ "keyword": "k" })], Mode::Place).unwrap();
    let s = common::read_sheet(&b);
    assert_eq!(
        headers(&s),
        [
            "키워드",
            "상태",
            "전체 순위",
            "광고 제외 순위",
            "페이지",
            "확인 업체 수",
            "광고 수",
            "타겟 병원명",
            "매칭 상호",
            "조회 시각 (한국)",
            "조회 기준",
            "검색 URL",
            "안내"
        ]
    );
}

#[test]
fn 블로그_행_값_숫자_순위_파생_열_빈_값_수식형_텍스트() {
    let found = json!({
        "keyword": "=SUM(1,2)", "status": "found", "rank": 3, "total": 30, "target": "썸블리의원",
        "matches": [{ "title": "첫 글", "url": "https://blog.naver.com/a/1" }, { "title": "둘째", "url": "https://blog.naver.com/b/2" }],
        "checkedAt": "2026-09-09T15:30:00.000Z", "scope": "S", "searchUrl": "https://search.naver.com/x", "message": ""
    });
    let not_found = json!({ "keyword": "k2", "status": "not_found", "rank": null, "total": 12, "matches": [], "message": "" });
    let cancelled = json!({ "keyword": "k3", "status": "cancelled", "rank": null });
    let unknown = json!({ "keyword": "k4", "status": "weird" });
    let b = make_workbook(&[found, not_found, cancelled, unknown], Mode::Blog).unwrap();
    let s = common::read_sheet(&b);
    let c = |a: &str| common::cell(&s, a).cloned();
    // 행 1
    assert_eq!(c("A2"), Some(Data::String("=SUM(1,2)".into())));
    assert!(s
        .formulas
        .get_value(common::pos("A2"))
        .is_none_or(|f| f.is_empty()));
    assert_eq!(c("B2"), Some(Data::String("노출".into())));
    assert_eq!(c("C2"), Some(Data::Float(3.0)));
    assert_eq!(c("D2"), Some(Data::Float(30.0)));
    assert_eq!(c("F2"), Some(Data::String("첫 글".into())));
    assert_eq!(
        c("G2"),
        Some(Data::String("https://blog.naver.com/a/1".into()))
    );
    assert_eq!(c("H2"), Some(Data::String("2026-09-10 00:30:00".into())));
    assert!(matches!(c("K2"), None | Some(Data::Empty)), "빈 안내");
    // 행 2 — 순위 없음은 빈 셀, not_found 빈 메시지는 PoC 폴백 문구 그대로
    assert_eq!(c("B3"), Some(Data::String("미노출".into())));
    assert!(matches!(c("C3"), None | Some(Data::Empty)));
    assert!(matches!(c("F3"), None | Some(Data::Empty)));
    assert!(matches!(c("G3"), None | Some(Data::Empty)));
    assert!(matches!(c("H3"), None | Some(Data::Empty)));
    assert_eq!(
        c("K3"),
        Some(Data::String(
            "이번 응답의 첫 파워링크 광고 목록에 없음".into()
        ))
    );
    // 행 3·4 — 중단 라벨, 모르는 상태는 원문
    assert_eq!(c("B4"), Some(Data::String("중단".into())));
    assert_eq!(c("B5"), Some(Data::String("weird".into())));
}

#[test]
fn 플레이스_매칭_상호는_슬래시로_잇는다() {
    let row = json!({ "keyword": "k", "status": "found", "rank": 2, "organicRank": null, "page": 1, "total": 50, "totalAds": 3,
        "matches": [{ "name": "무이성형외과의원" }, { "name": "무이성형외과 강남" }] });
    let b = make_workbook(&[row], Mode::Place).unwrap();
    let s = common::read_sheet(&b);
    assert_eq!(
        common::cell(&s, "I2"),
        Some(&Data::String("무이성형외과의원 / 무이성형외과 강남".into()))
    );
    assert!(matches!(common::cell(&s, "D2"), None | Some(Data::Empty)));
    assert_eq!(common::cell(&s, "G2"), Some(&Data::Float(3.0)));
}

#[test]
fn 머리행_스타일_필터_틀고정_작성자() {
    let rows: Vec<Value> = (0..3)
        .map(|i| json!({ "keyword": format!("k{i}") }))
        .collect();
    let b = make_workbook(&rows, Mode::Place).unwrap();
    let sheet = common::zip_text(&b, "xl/worksheets/sheet1.xml");
    assert!(sheet.contains(r#"<autoFilter ref="A1:M4"/>"#), "{sheet}");
    assert!(sheet.contains(r#"ySplit="1""#) && sheet.contains(r#"state="frozen""#));
    assert!(sheet.contains(r#"<row r="1""#) && sheet.contains(r#"ht="28""#));
    assert!(sheet.contains(r#"ht="25""#));
    assert!(
        sheet.contains(r#"width="80.7109375""#) || sheet.contains(r#"width="80.7"#),
        "조회 기준 너비 80"
    );
    let styles = common::zip_text(&b, "xl/styles.xml");
    assert!(styles.contains(r#"rgb="FF5467F7""#));
    assert!(styles.contains(r#"<b/>"#) && styles.contains(r#"rgb="FFFFFFFF""#));
    assert!(styles.contains(r#"vertical="center""#));
    let core = common::zip_text(&b, "docProps/core.xml");
    assert!(core.contains("<dc:creator>SCAX</dc:creator>"));
    let blog = make_workbook(&rows[..1], Mode::Blog).unwrap();
    assert!(common::zip_text(&blog, "xl/worksheets/sheet1.xml")
        .contains(r#"<autoFilter ref="A1:K2"/>"#));
}

#[test]
fn 저장_검증은_1_50행_keyword_문자열() {
    let ok = json!([{ "keyword": "a" }]);
    assert_eq!(validate_rows(Some(&ok)).unwrap().len(), 1);
    let fifty = Value::Array(
        (0..50)
            .map(|_| json!({ "keyword": "a", "anything": [1, 2] }))
            .collect(),
    );
    assert!(validate_rows(Some(&fifty)).is_ok());
    let bad = [
        json!([]),
        Value::Array((0..51).map(|_| json!({ "keyword": "a" })).collect()),
        json!([{ "keyword": 1 }]),
        json!([null]),
        json!(["a"]),
        json!({ "keyword": "a" }),
    ];
    for rows in &bad {
        assert_eq!(
            validate_rows(Some(rows)).unwrap_err(),
            ROWS_MESSAGE,
            "{rows}"
        );
    }
    assert_eq!(validate_rows(None).unwrap_err(), ROWS_MESSAGE);
}

#[test]
fn 기본_파일명은_한국_날짜() {
    use chrono::TimeZone;
    // 2026-09-30 15:30 UTC = 2026-10-01 00:30 KST
    let at = chrono::Utc
        .with_ymd_and_hms(2026, 9, 30, 15, 30, 0)
        .unwrap();
    assert_eq!(default_file_name(at), "네이버_키워드순위_2026-10-01.xlsx");
    let before = chrono::Utc
        .with_ymd_and_hms(2026, 9, 30, 14, 59, 59)
        .unwrap();
    assert_eq!(
        default_file_name(before),
        "네이버_키워드순위_2026-09-30.xlsx"
    );
}
