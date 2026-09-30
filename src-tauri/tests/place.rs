//! PoC `tests/place.test.mjs` 9개를 같은 단언으로 (A-1).

mod common;

use calamine::Data;
use sc_rank_lib::place::{parse_place_list, PlaceItem, PLACE_SCOPE};
use sc_rank_lib::workbook::{make_workbook, Mode};
use serde_json::{json, Value};

/// `const item = (name, ad = false, page = 1) => ({ name, ad, page })`
fn item(name: &str, ad: bool, page: u32) -> PlaceItem {
    PlaceItem {
        name: Some(name.into()),
        ad,
        page: Some(page),
        ..Default::default()
    }
}

fn spans(title: Option<&str>, spans: &[&str]) -> PlaceItem {
    PlaceItem {
        title: title.map(Into::into),
        spans: Some(spans.iter().map(|s| s.to_string()).collect()),
        ..Default::default()
    }
}

#[test]
fn 광고_사이_타겟의_전체_일반_순위와_페이지() {
    let r = parse_place_list(
        &[
            item("가병원", true, 1),
            item("나병원", false, 1),
            item("다병원", true, 1),
            item("무이성형외과의원", false, 2),
        ],
        "무이성형외과",
    )
    .unwrap();
    assert_eq!(r.rank, Some(4));
    assert_eq!(r.organic_rank, Some(2));
    assert_eq!(r.page, Some(2));
    assert_eq!(r.total_ads, 2);
}

#[test]
fn 부분_일치_공백_차이_동명_다건_보존() {
    let r = parse_place_list(
        &[
            item("무 이 성형외과의원", false, 1),
            item("무이성형외과 강남의원", false, 1),
        ],
        "무이 성형외과",
    )
    .unwrap();
    assert_eq!(r.matches.len(), 2);
    assert_eq!(r.rank, Some(1));
}

#[test]
fn 광고와_일반_결과에_모두_있으면_첫_일반_매칭의_순위를_표시() {
    let r = parse_place_list(
        &[
            item("무이성형외과의원", true, 1),
            item("다른병원", false, 1),
            item("무이성형외과의원", false, 1),
        ],
        "무이성형외과",
    )
    .unwrap();
    assert_eq!(r.rank, Some(1));
    assert_eq!(r.organic_rank, Some(2));
    assert_eq!(r.matches.len(), 2);
    assert_eq!(r.rows[0].organic_rank, None);
}

#[test]
fn 광고만_매칭되면_광고_제외_순위_없음() {
    assert_eq!(
        parse_place_list(&[item("무이성형외과의원", true, 1)], "무이")
            .unwrap()
            .organic_rank,
        None
    );
}

#[test]
fn 미노출과_읽기_실패_구분() {
    assert_eq!(
        parse_place_list(&[item("다른병원", false, 1)], "무이")
            .unwrap()
            .status,
        "not_found"
    );
    assert!(parse_place_list(&[], "무이").is_err());
    assert!(parse_place_list(&[spans(None, &["이미지 수 6"])], "무이").is_err());
}

#[test]
fn 두_span_상호_전체_title_우선_진료과_단독_선택_방지() {
    let r = parse_place_list(
        &[spans(
            Some("비티성형외과의원 강남성형외과"),
            &[
                "의191210",
                "이미지 수 6",
                "성형외과",
                "비티성형외과의원",
                "강남성형외과",
            ],
        )],
        "비티성형외과",
    )
    .unwrap();
    assert_eq!(r.status, "found");
    assert_eq!(r.rows[0].name, "비티성형외과의원 강남성형외과");
    assert_eq!(
        parse_place_list(
            &[spans(None, &["이미지 수 6", "비티", "성형외과의원"])],
            "비티성형외과"
        )
        .unwrap()
        .status,
        "found"
    );
}

#[test]
fn 플레이스_xlsx_한글_13열_숫자_한국시각_수식_텍스트() {
    let r = parse_place_list(&[item("무이성형외과의원", false, 1)], "무이").unwrap();
    // {...r, keyword:'=1+1', target:'무이', scope:PLACE_SCOPE, checkedAt:'2026-09-09T01:00:00Z'}
    let mut row: Value = serde_json::to_value(&r).unwrap();
    let obj = row.as_object_mut().unwrap();
    obj.insert("keyword".into(), json!("=1+1"));
    obj.insert("target".into(), json!("무이"));
    obj.insert("scope".into(), json!(PLACE_SCOPE));
    obj.insert("checkedAt".into(), json!("2026-09-09T01:00:00Z"));
    let b = make_workbook(&[row], Mode::Place).unwrap();
    let s = common::read_sheet(&b);
    assert_eq!(s.range.width(), 13);
    assert_eq!(
        common::cell(&s, "D1"),
        Some(&Data::String("광고 제외 순위".into()))
    );
    assert_eq!(common::cell(&s, "D2"), Some(&Data::Float(1.0)));
    assert_eq!(common::cell(&s, "A2"), Some(&Data::String("=1+1".into())));
    assert!(s
        .formulas
        .get_value(common::pos("A2"))
        .is_none_or(|f| f.is_empty()));
    assert_eq!(
        common::cell(&s, "J2"),
        Some(&Data::String("2026-09-09 10:00:00".into()))
    );
    match common::cell(&s, "K2") {
        Some(Data::String(k)) => assert!(k.contains("위치 고정 없음")),
        other => panic!("K2 = {other:?}"),
    }
}

#[test]
fn 진료과_전문의_수는_병원명이_아니다() {
    assert!(parse_place_list(
        &[spans(None, &["성형외과", "성형외과 1명", "이미지 수 6"])],
        "무이"
    )
    .is_err());
}

#[test]
fn 이전_페이지_전체와_2페이지_상호의_누적_순위() {
    let r = parse_place_list(
        &[
            item("이전광고병원", true, 1),
            item("이전일반병원", false, 1),
            item("아이그램성형외과의원", false, 2),
            item("무이성형외과의원", false, 2),
            item("윌비성형외과의원", false, 2),
        ],
        "무이성형외과",
    )
    .unwrap();
    assert_eq!(r.rank, Some(4));
    assert_eq!(r.organic_rank, Some(3));
    assert_eq!(r.page, Some(2));
    assert_eq!(r.total, 5);
}
