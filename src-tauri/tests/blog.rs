//! PoC `tests/blog.test.mjs` 3개를 같은 단언으로 (A-1).

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;

use image::codecs::jpeg::JpegEncoder;
use image::imageops::FilterType;
use image::ImageFormat;
use sc_rank_lib::blog::{
    hash_distance, image_hash, map_concurrent, match_blog_keyword, parse_blog_list,
};

fn card(title: &str, url: &str) -> String {
    format!(
        r#"<div data-template-id="ugcItem"><a href="{url}"><span class="sds-comps-text-type-headline1">{title}</span></a><a href="{url}"><span class="sds-comps-text-type-body1">후기 내용</span></a><a href="{url}"><img src="https://search.pstatic.net/common/?src=abc"/></a></div>"#
    )
}

#[test]
fn 글_카드_순서로_순위_계산_썸네일_링크_중복은_순위에서_제외() {
    let html = format!(
        "<title>키워드 : 네이버 블로그검색</title>{}{}",
        card("다른 병원", "https://blog.naver.com/a/1"),
        card("썸 블리 의원 후기", "https://blog.naver.com/b/2")
    );
    let rows = parse_blog_list(&html).unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(
        match_blog_keyword(&rows, "썸블리의원").unwrap().rank,
        Some(2)
    );
    assert_eq!(rows[1].thumbnail_urls.len(), 1);
    assert_eq!(
        match_blog_keyword(&rows, "없는 병원").unwrap().status,
        "not_found"
    );
}

#[test]
fn 같은_이미지의_축소_jpeg는_phash_일치_다른_이미지_배치는_구분() {
    let original =
        std::fs::read(concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/1.png")).unwrap();
    let hash = image_hash(&original).unwrap();
    // sharp(original).resize(180).jpeg({ quality: 60 })
    let img = image::load_from_memory(&original).unwrap();
    let small = img.resize(180, u32::MAX, FilterType::Lanczos3);
    assert_eq!(small.width(), 180);
    let mut resized = Vec::new();
    JpegEncoder::new_with_quality(&mut resized, 60)
        .encode_image(&small.to_rgb8())
        .unwrap();
    assert!(hash_distance(&hash, &image_hash(&resized).unwrap()) <= 6);
    // sharp(original).flop().png()
    let mut mirrored = Vec::new();
    img.fliph()
        .write_to(&mut std::io::Cursor::new(&mut mirrored), ImageFormat::Png)
        .unwrap();
    assert!(hash_distance(&hash, &image_hash(&mirrored).unwrap()) > 6);
}

#[tokio::test]
async fn 병렬_비교는_제한을_지키고_입력_순서를_보존() {
    let active = Arc::new(AtomicUsize::new(0));
    let max = Arc::new(AtomicUsize::new(0));
    let result = map_concurrent(vec![1, 2, 3, 4, 5, 6, 7, 8, 9], 3, |x, _| {
        let (active, max) = (active.clone(), max.clone());
        async move {
            let now = active.fetch_add(1, Ordering::SeqCst) + 1;
            max.fetch_max(now, Ordering::SeqCst);
            tokio::task::yield_now().await; // setImmediate
            active.fetch_sub(1, Ordering::SeqCst);
            x * 2
        }
    })
    .await;
    assert_eq!(max.load(Ordering::SeqCst), 3);
    assert_eq!(result, vec![2, 4, 6, 8, 10, 12, 14, 16, 18]);
}
