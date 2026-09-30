//! 블로그 판정의 나머지 규칙 — PoC `blog.mjs` 대조 (검증 문구·scope·target·목표 이미지·썸네일 주소·이미지 배치 판정·반환 모양).

use std::collections::HashMap;

use sc_rank_lib::blog::{
    blog_result, decode_target_image, image_url, judge_image_batch, new_thumbnail_urls,
    parse_blog_list, BlogInput, BlogMatch, BlogRow,
    ThumbnailCheck::{self, Distance, Error},
};
use sc_rank_lib::errors::{ScrapeError, BLOG_FALLBACK};
use serde_json::{json, Value};

fn input(v: Value) -> BlogInput {
    let g = |k: &str| v.get(k).cloned();
    BlogInput::new(
        g("keyword"),
        g("targetType"),
        g("target"),
        g("imageData"),
        g("imageName"),
    )
}

fn msg(r: Result<(), ScrapeError>) -> String {
    r.unwrap_err().to_string()
}

#[test]
fn 입력_검증_문구_3종() {
    assert_eq!(
        msg(input(json!({ "keyword": " ", "target": "t" })).validate()),
        "검색 키워드는 1~100자로 입력해 주세요."
    );
    assert_eq!(
        msg(input(json!({ "keyword": "가".repeat(101), "target": "t" })).validate()),
        "검색 키워드는 1~100자로 입력해 주세요."
    );
    assert_eq!(
        msg(input(json!({ "keyword": "k", "targetType": "video" })).validate()),
        "타겟 종류를 확인해 주세요."
    );
    assert_eq!(
        msg(input(json!({ "keyword": "k", "target": "" })).validate()),
        "타겟 키워드는 1~100자로 입력해 주세요."
    );
    assert!(input(json!({ "keyword": "가".repeat(100), "target": "t" }))
        .validate()
        .is_ok());
    assert!(input(json!({ "keyword": "k", "targetType": "image" }))
        .validate()
        .is_ok());
}

#[test]
fn scope_와_target() {
    let k = input(json!({ "keyword": "구월동 제모", "target": "썸블리" }));
    assert_eq!(
        k.scope(),
        "PC 네이버 블로그 탭 · 초기 목록 + 최대 3회 스크롤 · 제목·요약 키워드 일치"
    );
    assert_eq!(k.result_target(), json!("썸블리"));
    assert_eq!(k.search_url(), "https://search.naver.com/search.naver?ssc=tab.blog.all&sm=tab_hty.top&query=%EA%B5%AC%EC%9B%94%EB%8F%99%20%EC%A0%9C%EB%AA%A8");
    let i = input(json!({ "keyword": "k", "targetType": "image", "target": "x", "imageName": "" }));
    assert_eq!(
        i.scope(),
        "PC 네이버 블로그 탭 · 초기 목록 + 최대 3회 스크롤 · 검색 썸네일 pHash(거리 6 이하)"
    );
    assert_eq!(i.result_target(), json!("업로드 이미지"));
    assert_eq!(
        input(json!({ "keyword": "k", "targetType": "image", "imageName": "a.png" }))
            .result_target(),
        json!("a.png")
    );
}

#[test]
fn 목표_이미지_디코드와_검증() {
    let png = std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/1.png")).unwrap();
    let b64 = base64_std(&png);
    assert_eq!(
        decode_target_image(Some(&json!(format!("data:image/png;base64,{b64}")))).unwrap(),
        png
    );
    let bad_type = "PNG, JPG, WebP 이미지를 선택해 주세요.";
    for v in [
        json!("data:image/gif;base64,AAAA"),
        json!("iVBOR"),
        json!(1),
    ] {
        assert_eq!(
            decode_target_image(Some(&v)).unwrap_err().to_string(),
            bad_type
        );
    }
    assert_eq!(decode_target_image(None).unwrap_err().to_string(), bad_type);
    let size = "이미지는 4MB 이하로 선택해 주세요.";
    assert_eq!(
        decode_target_image(Some(&json!("data:image/png;base64,")))
            .unwrap_err()
            .to_string(),
        size
    );
    let big = base64_std(&vec![0u8; 4 * 1024 * 1024 + 1]);
    assert_eq!(
        decode_target_image(Some(&json!(format!("data:image/jpeg;base64,{big}"))))
            .unwrap_err()
            .to_string(),
        size
    );
    let exact = base64_std(&vec![0u8; 4 * 1024 * 1024]);
    assert_eq!(
        decode_target_image(Some(&json!(format!("data:image/webp;base64,{exact}"))))
            .unwrap()
            .len(),
        4 * 1024 * 1024
    );
}

fn base64_std(bytes: &[u8]) -> String {
    const T: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut s = String::new();
    for chunk in bytes.chunks(3) {
        let n = chunk
            .iter()
            .enumerate()
            .fold(0u32, |n, (i, b)| n | (u32::from(*b) << (16 - 8 * i)));
        for i in 0..4 {
            s.push(if i <= chunk.len() {
                T[(n >> (18 - 6 * i) & 63) as usize] as char
            } else {
                '='
            });
        }
    }
    s
}

#[test]
fn 썸네일_주소는_https_search_pstatic_net_common_sunny_만() {
    assert_eq!(
        image_url("https://search.pstatic.net/common/?src=abc&type=w").unwrap(),
        "https://search.pstatic.net/common/?src=abc&type=w"
    );
    assert!(image_url("https://search.pstatic.net/sunny/?src=x").is_ok());
    let unsupported = "지원하지 않는 검색 썸네일 주소입니다.";
    for u in [
        "http://search.pstatic.net/common/?src=x",
        "https://evil.net/common/?src=x",
        "https://search.pstatic.net/other/?src=x",
        "https://search.pstatic.net/common?src=x",
    ] {
        assert_eq!(image_url(u).unwrap_err().to_string(), unsupported, "{u}");
    }
    assert!(matches!(
        image_url("not a url").unwrap_err(),
        ScrapeError::Other(_)
    ));
}

fn row(rank: usize, thumbs: &[&str]) -> BlogRow {
    BlogRow {
        rank,
        title: format!("t{rank}"),
        url: format!("https://blog.naver.com/a/{rank}"),
        snippet: String::new(),
        blogger: String::new(),
        thumbnail_urls: thumbs.iter().map(|s| s.to_string()).collect(),
        image_url: None,
        hash_distance: None,
    }
}

fn checks(v: &[(&str, ThumbnailCheck)]) -> HashMap<String, ThumbnailCheck> {
    v.iter().map(|(k, c)| (k.to_string(), *c)).collect()
}

#[test]
fn 이미지_배치_판정() {
    let rows = [row(1, &["a"]), row(2, &["b", "c"]), row(3, &["d"])];
    // 가장 가까운 썸네일 · 거리 6 이하만 · 첫 매칭의 순위 · 비교 수
    let m = judge_image_batch(
        &rows,
        &checks(&[
            ("a", Distance(20)),
            ("b", Distance(9)),
            ("c", Distance(4)),
            ("d", Distance(6)),
        ]),
    )
    .unwrap();
    assert_eq!(
        (m.status.as_str(), m.rank, m.compared_images),
        ("found", Some(2), Some(4))
    );
    assert_eq!(m.matches.len(), 2);
    assert_eq!(
        (
            m.matches[0].image_url.as_deref(),
            m.matches[0].hash_distance
        ),
        (Some("c"), Some(4))
    );
    // 첫 매칭 앞에서 썸네일 실패 → 확정 불가, 뒤에서 실패 → 무시
    let before = judge_image_batch(
        &rows,
        &checks(&[
            ("a", Error),
            ("b", Distance(9)),
            ("c", Distance(4)),
            ("d", Distance(6)),
        ]),
    );
    assert_eq!(
        before.unwrap_err().to_string(),
        "일부 썸네일을 읽지 못해 첫 노출 순위를 확정할 수 없습니다. 다시 조회해 주세요."
    );
    assert!(judge_image_batch(
        &rows,
        &checks(&[
            ("a", Distance(1)),
            ("b", Error),
            ("c", Distance(40)),
            ("d", Distance(6))
        ])
    )
    .is_ok());
    // 매칭이 없으면 모든 실패가 앞선 실패
    assert!(judge_image_batch(
        &rows,
        &checks(&[
            ("a", Distance(30)),
            ("b", Distance(30)),
            ("c", Distance(30)),
            ("d", Error)
        ])
    )
    .is_err());
    let none = judge_image_batch(
        &rows,
        &checks(&[
            ("a", Distance(30)),
            ("b", Distance(30)),
            ("c", Distance(30)),
            ("d", Distance(7)),
        ]),
    )
    .unwrap();
    assert_eq!((none.status.as_str(), none.rank), ("not_found", None));
}

#[test]
fn 배치_간_썸네일_캐시와_썸네일_0건() {
    let rows = [row(1, &["a", "b"]), row(2, &["b", "c"])];
    assert_eq!(
        new_thumbnail_urls(&rows, &checks(&[("b", Distance(1))])).unwrap(),
        ["a", "c"]
    );
    assert_eq!(
        new_thumbnail_urls(&[row(1, &[])], &HashMap::new())
            .unwrap_err()
            .to_string(),
        "비교할 검색 썸네일을 읽지 못했습니다."
    );
}

#[test]
fn 반환_모양_성공_오류() {
    let inp = input(json!({ "keyword": "k", "target": "t" }));
    let ok = blog_result(
        &inp,
        Ok((
            BlogMatch {
                status: "not_found".into(),
                rank: None,
                matches: vec![],
                total: 7,
                compared_images: None,
            },
            2,
        )),
        "T".into(),
    );
    let v = serde_json::to_value(&ok).unwrap();
    assert_eq!(v["scrolls"], json!(2));
    assert_eq!(v["rank"], Value::Null);
    assert_eq!(
        v["message"],
        json!("이번 블로그 탭 초기 목록 + 최대 3회 스크롤에서 찾지 못했습니다.")
    );
    assert!(v.get("comparedImages").is_none());
    assert_eq!(v["targetType"], json!("keyword"));
    let err = blog_result(
        &inp,
        Err(ScrapeError::other(anyhow::anyhow!("net::ERR_TIMED_OUT"))),
        "T".into(),
    );
    let v = serde_json::to_value(&err).unwrap();
    assert_eq!(
        (
            v["status"].clone(),
            v["total"].clone(),
            v["matches"].clone()
        ),
        (json!("error"), Value::Null, json!([]))
    );
    assert!(v.get("scrolls").is_none());
    assert_eq!(v["message"], json!(BLOG_FALLBACK));
    let domain = blog_result(
        &inp,
        Err(ScrapeError::domain("블로그 결과를 읽지 못했습니다.")),
        "T".into(),
    );
    assert_eq!(domain.message, "블로그 결과를 읽지 못했습니다.");
}

#[test]
fn 파싱_오류_문구와_새_창_열림_정리() {
    assert_eq!(
        parse_blog_list("<title>x</title>").unwrap_err().to_string(),
        "정상 블로그 검색 페이지를 받지 못했습니다."
    );
    assert_eq!(
        parse_blog_list("<title>블로그검색</title><div></div>")
            .unwrap_err()
            .to_string(),
        "블로그 결과 목록을 읽지 못했습니다. 검색 결과가 없거나 구조가 바뀌었을 수 있습니다."
    );
    let no_title = r#"<title>블로그검색</title><div data-template-id="ugcItem"><a href="https://x/1">x</a></div>"#;
    assert_eq!(
        parse_blog_list(no_title).unwrap_err().to_string(),
        "일부 글의 제목을 읽지 못해 순위를 계산할 수 없습니다."
    );
    let html = r#"<title>블로그검색</title><div data-template-id="ugcItem"><a href="https://blog.naver.com/nick">닉네임  새 창 열림</a><a href="https://blog.naver.com/nick/1"><span class="x-text-type-headline1">제목   새 창 열림</span><img data-src="https://search.pstatic.net/common/?src=1"/><img src="" data-src=""/></a></div>"#;
    let rows = parse_blog_list(html).unwrap();
    assert_eq!(
        (
            rows[0].title.as_str(),
            rows[0].blogger.as_str(),
            rows[0].snippet.as_str()
        ),
        ("제목", "닉네임", "")
    );
    assert_eq!(
        rows[0].thumbnail_urls,
        ["https://search.pstatic.net/common/?src=1"]
    );
}
