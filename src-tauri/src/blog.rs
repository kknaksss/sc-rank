//! 블로그 판정 — PoC `server/blog.mjs` 의 순수 부분.
//! 브라우저 스크롤 수집(`blog-browser.mjs`)과 썸네일 HTTP 다운로드는 P3.

use std::collections::HashMap;
use std::future::Future;
use std::io::Cursor;
use std::sync::LazyLock;

use anyhow::anyhow;
use futures::stream::{self, StreamExt};
use image::{DynamicImage, ImageDecoder, ImageFormat, ImageReader, RgbImage};
use regex::Regex;
use scraper::{ElementRef, Html, Selector};
use serde::Serialize;
use serde_json::Value;

use crate::errors::{Area, Result, ScrapeError};
use crate::js;

/// 썸네일 HTTP 요청 UA (`blog.mjs:7`) — PoC 문자열 그대로 (SPEC §3).
pub const THUMBNAIL_UA: &str = "Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/140.0.0.0 Safari/537.36";
/// 썸네일 다운로드 제한 시간 초 (`blog.mjs:92` `seconds: 8`).
pub const THUMBNAIL_TIMEOUT_SECS: u64 = 8;
/// 썸네일 최대 크기 (`blog.mjs:55` `--max-filesize 6291456`).
pub const THUMBNAIL_MAX_BYTES: u64 = 6_291_456;
/// 썸네일 동시 비교 수 (`blog.mjs:91`).
pub const THUMBNAIL_CONCURRENCY: usize = 8;
/// 같은 이미지로 보는 해시 거리 상한 (`blog.mjs:98`).
pub const MATCH_DISTANCE: u32 = 6;
/// 입력 픽셀 상한 (`blog.mjs:35` `limitInputPixels`).
pub const MAX_INPUT_PIXELS: u64 = 25_000_000;
/// 목표 이미지 최대 크기 (`blog.mjs:61`).
pub const TARGET_IMAGE_MAX_BYTES: usize = 4 * 1024 * 1024;
/// `blog.mjs:109`
pub const NOT_FOUND_MESSAGE: &str =
    "이번 블로그 탭 초기 목록 + 최대 3회 스크롤에서 찾지 못했습니다.";

/// `blog.mjs:8`
pub fn blog_url(keyword: &str) -> String {
    format!(
        "https://search.naver.com/search.naver?ssc=tab.blog.all&sm=tab_hty.top&query={}",
        js::encode_uri_component(keyword)
    )
}

/// `blog.mjs:9` — `'새 창 열림'` 제거 · 공백 접기 · trim
fn clean(value: &str) -> String {
    js::trim(&js::collapse_spaces(&value.replace("새 창 열림", ""))).to_string()
}

/// `blog.mjs:10`
fn compact(value: &str) -> String {
    js::strip_spaces(&clean(value)).to_lowercase()
}

fn sel(css: &str) -> Selector {
    Selector::parse(css).expect("static selector")
}

static TITLE: LazyLock<Selector> = LazyLock::new(|| sel("title"));
static CARD: LazyLock<Selector> = LazyLock::new(|| sel(r#"[data-template-id="ugcItem"]"#));
static ANCHOR: LazyLock<Selector> = LazyLock::new(|| sel("a"));
static HEADLINE: LazyLock<Selector> = LazyLock::new(|| sel(r#"[class*="text-type-headline"]"#));
static BODY1: LazyLock<Selector> = LazyLock::new(|| sel(r#"[class*="text-type-body1"]"#));
static IMG: LazyLock<Selector> = LazyLock::new(|| sel("img"));
static HTTP_URL: LazyLock<Regex> = LazyLock::new(|| Regex::new(r"^https?://").expect("url regex"));
static BLOGGER_HREF: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"blog\.naver\.com/[^/?]+/?$").expect("blogger regex"));

/// 블로그 글 카드 한 줄 (`blog.mjs:25`). 이미지 모드 매칭이면 `imageUrl`·`hashDistance` 가 붙는다(`:98`).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BlogRow {
    pub rank: usize,
    pub title: String,
    pub url: String,
    pub snippet: String,
    pub blogger: String,
    pub thumbnail_urls: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub image_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hash_distance: Option<u32>,
}

/// 배치 판정 결과 (`blog.mjs:31` · `:103`).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BlogMatch {
    pub status: String,
    pub rank: Option<usize>,
    pub matches: Vec<BlogRow>,
    pub total: usize,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compared_images: Option<usize>,
}

fn text(el: ElementRef) -> String {
    el.text().collect()
}

/// `parseBlogList` (`blog.mjs:11-27`)
pub fn parse_blog_list(html: &str) -> Result<Vec<BlogRow>> {
    let doc = Html::parse_document(html);
    let title: String = doc.select(&TITLE).map(text).collect();
    if !title.contains("블로그검색") {
        return Err(ScrapeError::domain(
            "정상 블로그 검색 페이지를 받지 못했습니다.",
        ));
    }
    let cards: Vec<ElementRef> = doc.select(&CARD).collect();
    if cards.is_empty() {
        return Err(ScrapeError::domain(
            "블로그 결과 목록을 읽지 못했습니다. 검색 결과가 없거나 구조가 바뀌었을 수 있습니다.",
        ));
    }
    let mut rows = Vec::with_capacity(cards.len());
    for (i, card) in cards.into_iter().enumerate() {
        let anchors: Vec<ElementRef> = card.select(&ANCHOR).collect();
        let link = anchors
            .iter()
            .find(|a| a.select(&HEADLINE).next().is_some());
        let title = link
            .and_then(|a| a.select(&HEADLINE).next())
            .map(|h| clean(&text(h)))
            .unwrap_or_default();
        let url = link.and_then(|a| a.value().attr("href"));
        let url = match url {
            Some(u) if !title.is_empty() && HTTP_URL.is_match(u) => u.to_string(),
            _ => {
                return Err(ScrapeError::domain(
                    "일부 글의 제목을 읽지 못해 순위를 계산할 수 없습니다.",
                ))
            }
        };
        let same_url = || {
            anchors
                .iter()
                .filter(|a| a.value().attr("href") == Some(url.as_str()))
        };
        let snippet = same_url()
            .find(|a| a.select(&BODY1).next().is_some())
            .and_then(|a| a.select(&BODY1).next())
            .map(|b| clean(&text(b)))
            .unwrap_or_default();
        let mut thumbnail_urls: Vec<String> = Vec::new();
        for a in same_url() {
            for img in a.select(&IMG) {
                let src = img
                    .value()
                    .attr("src")
                    .filter(|s| !s.is_empty())
                    .or_else(|| img.value().attr("data-src"))
                    .filter(|s| !s.is_empty());
                if let Some(src) = src {
                    if !thumbnail_urls.iter().any(|u| u == src) {
                        thumbnail_urls.push(src.to_string());
                    }
                }
            }
        }
        let blogger = anchors
            .iter()
            .filter(|a| BLOGGER_HREF.is_match(a.value().attr("href").unwrap_or("")))
            .map(|a| clean(&text(*a)))
            .find(|s| !s.is_empty())
            .unwrap_or_default();
        rows.push(BlogRow {
            rank: i + 1,
            title,
            url,
            snippet,
            blogger,
            thumbnail_urls,
            image_url: None,
            hash_distance: None,
        });
    }
    Ok(rows)
}

/// `matchBlogKeyword` (`blog.mjs:28-32`)
pub fn match_blog_keyword(rows: &[BlogRow], target: &str) -> Result<BlogMatch> {
    let target = compact(target);
    if target.is_empty() {
        return Err(ScrapeError::domain("타겟 키워드를 입력해 주세요."));
    }
    let matches: Vec<BlogRow> = rows
        .iter()
        .filter(|row| compact(&format!("{} {}", row.title, row.snippet)).contains(&target))
        .cloned()
        .collect();
    Ok(BlogMatch {
        status: if matches.is_empty() {
            "not_found"
        } else {
            "found"
        }
        .into(),
        rank: matches.first().map(|m| m.rank),
        matches,
        total: rows.len(),
        compared_images: None,
    })
}

// ---- 이미지 해시 (`blog.mjs:33-46`) ----

/// 디코드 + EXIF 회전 (`rotate()`) · 입력 픽셀 상한.
pub fn decode_image(bytes: &[u8]) -> Result<DynamicImage> {
    let reader = ImageReader::new(Cursor::new(bytes))
        .with_guessed_format()
        .map_err(ScrapeError::other)?;
    let mut decoder = reader.into_decoder().map_err(ScrapeError::other)?;
    let (w, h) = decoder.dimensions();
    if u64::from(w) * u64::from(h) > MAX_INPUT_PIXELS {
        return Err(ScrapeError::other(anyhow!(
            "Input image exceeds pixel limit ({w}x{h})"
        )));
    }
    let orientation = decoder
        .orientation()
        .unwrap_or(image::metadata::Orientation::NoTransforms);
    let mut img = DynamicImage::from_decoder(decoder).map_err(ScrapeError::other)?;
    img.apply_orientation(orientation);
    Ok(img)
}

/// 전처리(흰 배경 합성 → 32×32 fill → 회색조) 후 63비트 DCT 해시. 입력은 이미 회전된 이미지.
pub fn hash_image(img: &DynamicImage) -> String {
    // flatten({ background: '#fff' })
    let rgba = img.to_rgba8();
    let flat = RgbImage::from_fn(rgba.width(), rgba.height(), |x, y| {
        let p = rgba.get_pixel(x, y);
        let a = f32::from(p[3]) / 255.0;
        let mix = |c: u8| (f32::from(c) * a + 255.0 * (1.0 - a)).round() as u8;
        image::Rgb([mix(p[0]), mix(p[1]), mix(p[2])])
    });
    // resize(32, 32, { fit: 'fill' }) → greyscale()
    let small = image::imageops::resize(&flat, 32, 32, image::imageops::FilterType::Lanczos3);
    let pixels = DynamicImage::ImageRgb8(small).to_luma8().into_raw();
    // 63 low-frequency DCT bits (DC excluded). Compare visual identity, not semantics.
    let pi = std::f64::consts::PI;
    let mut coefficients = Vec::with_capacity(63);
    for u in 0..8u32 {
        for v in 0..8u32 {
            if u == 0 && v == 0 {
                continue;
            }
            let mut sum = 0.0f64;
            for x in 0..32u32 {
                for y in 0..32u32 {
                    let px = f64::from(pixels[(y * 32 + x) as usize]);
                    let cx = (f64::from(2 * x + 1) * f64::from(u) * pi / 64.0).cos();
                    let cy = (f64::from(2 * y + 1) * f64::from(v) * pi / 64.0).cos();
                    sum += px * cx * cy;
                }
            }
            coefficients.push(sum);
        }
    }
    let mut sorted = coefficients.clone();
    sorted.sort_by(|a, b| a.total_cmp(b));
    let median = sorted[31];
    coefficients
        .iter()
        .map(|v| if *v > median { '1' } else { '0' })
        .collect()
}

/// `imageHash(buffer)`
pub fn image_hash(bytes: &[u8]) -> Result<String> {
    Ok(hash_image(&decode_image(bytes)?))
}

/// 목표 해시 2종 (`blog.mjs:78`): 원본, 그리고 `rotate().resize(250,208,{fit:'cover'})` 본.
/// sharp 는 입력 형식으로 다시 인코딩하므로 JPEG 는 품질 80(sharp 기본)으로 다시 인코딩해 해시한다.
pub fn target_hashes(bytes: &[u8]) -> Result<[String; 2]> {
    let img = decode_image(bytes)?;
    let original = hash_image(&img);
    let cover = img.resize_to_fill(250, 208, image::imageops::FilterType::Lanczos3);
    let cover = if image::guess_format(bytes).ok() == Some(ImageFormat::Jpeg) {
        let mut buf = Vec::new();
        image::codecs::jpeg::JpegEncoder::new_with_quality(&mut buf, 80)
            .encode_image(&cover.to_rgb8())
            .map_err(ScrapeError::other)?;
        decode_image(&buf)?
    } else {
        cover
    };
    Ok([original, hash_image(&cover)])
}

/// `hashDistance` (`blog.mjs:46`)
pub fn hash_distance(a: &str, b: &str) -> u32 {
    let b: Vec<char> = b.chars().collect();
    a.chars()
        .enumerate()
        .filter(|(i, bit)| b.get(*i) != Some(bit))
        .count() as u32
}

/// 두 목표 해시 중 최솟값 거리 (`blog.mjs:92`).
pub fn min_distance(targets: &[String], hash: &str) -> u32 {
    targets
        .iter()
        .map(|t| hash_distance(t, hash))
        .min()
        .unwrap_or(u32::MAX)
}

/// `mapConcurrent` (`blog.mjs:47-53`) — 동시에 최대 `limit` 개, 결과는 입력 순서.
/// PoC 호출부는 언제나 `limit = 8` 이다. `limit` 은 1 이상이어야 한다.
pub async fn map_concurrent<T, R, F, Fut>(items: Vec<T>, limit: usize, f: F) -> Vec<R>
where
    F: Fn(T, usize) -> Fut,
    Fut: Future<Output = R>,
{
    assert!(limit >= 1, "map_concurrent limit must be >= 1");
    let len = items.len();
    let mut done: Vec<(usize, R)> = stream::iter(items.into_iter().enumerate().map(|(i, x)| {
        let fut = f(x, i);
        async move { (i, fut.await) }
    }))
    .buffer_unordered(limit)
    .collect()
    .await;
    debug_assert_eq!(done.len(), len);
    done.sort_by_key(|(i, _)| *i);
    done.into_iter().map(|(_, r)| r).collect()
}

// ---- 목표 이미지·썸네일 주소 (`blog.mjs:58-69`) ----

static DATA_URL: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^data:image/(png|jpeg|webp);base64,").expect("data url regex"));

/// `decodeTargetImage` (`blog.mjs:58-63`)
pub fn decode_target_image(data: Option<&Value>) -> Result<Vec<u8>> {
    let data = match data.and_then(Value::as_str) {
        Some(s) if DATA_URL.is_match(s) => s,
        _ => {
            return Err(ScrapeError::domain(
                "PNG, JPG, WebP 이미지를 선택해 주세요.",
            ))
        }
    };
    let comma = data.find(',').map(|i| i + 1).unwrap_or(0);
    let buffer = node_base64_decode(&data[comma..]);
    if buffer.is_empty() || buffer.len() > TARGET_IMAGE_MAX_BYTES {
        return Err(ScrapeError::domain("이미지는 4MB 이하로 선택해 주세요."));
    }
    Ok(buffer)
}

/// Node `Buffer.from(s, 'base64')` — 표준·URL 안전 문자를 받고, 그 밖의 문자는 건너뛰고, `=` 에서 멈춘다.
fn node_base64_decode(input: &str) -> Vec<u8> {
    fn value(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' | b'-' => 62,
            b'/' | b'_' => 63,
            _ => return None,
        } as u32)
    }
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let mut acc = 0u32;
    let mut n = 0;
    for c in input.bytes() {
        if c == b'=' {
            break;
        }
        let Some(v) = value(c) else { continue };
        acc = (acc << 6) | v;
        n += 1;
        if n == 4 {
            out.extend_from_slice(&[(acc >> 16) as u8, (acc >> 8) as u8, acc as u8]);
            acc = 0;
            n = 0;
        }
    }
    match n {
        2 => out.push((acc >> 4) as u8),
        3 => out.extend_from_slice(&[(acc >> 10) as u8, (acc >> 2) as u8]),
        _ => {}
    }
    out
}

/// `imageUrl` (`blog.mjs:64-69`) — 검색 결과가 준 썸네일 주소 그대로.
pub fn image_url(source: &str) -> Result<String> {
    let url = url::Url::parse(source).map_err(ScrapeError::other)?;
    if url.scheme() != "https"
        || url.host_str() != Some("search.pstatic.net")
        || !["/common/", "/sunny/"].contains(&url.path())
    {
        return Err(ScrapeError::domain("지원하지 않는 검색 썸네일 주소입니다."));
    }
    Ok(url.to_string())
}

// ---- 이미지 모드 배치 판정 (`blog.mjs:86-103`) ----

/// 썸네일 하나의 비교 결과 (`blog.mjs:92-93`).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ThumbnailCheck {
    Distance(u32),
    Error,
}

/// 이번 배치에서 새로 비교할 썸네일 주소 (`blog.mjs:87-89`). 배치 전체에 썸네일이 없으면 오류.
pub fn new_thumbnail_urls(
    rows: &[BlogRow],
    checked: &HashMap<String, ThumbnailCheck>,
) -> Result<Vec<String>> {
    let mut all: Vec<&String> = Vec::new();
    for url in rows.iter().flat_map(|r| r.thumbnail_urls.iter()) {
        if !all.contains(&url) {
            all.push(url);
        }
    }
    if all.is_empty() {
        return Err(ScrapeError::domain("비교할 검색 썸네일을 읽지 못했습니다."));
    }
    Ok(all
        .into_iter()
        .filter(|u| !checked.contains_key(*u))
        .cloned()
        .collect())
}

/// 누적된 썸네일 비교(`byUrl`)로 배치를 판정 (`blog.mjs:96-103`).
pub fn judge_image_batch(
    rows: &[BlogRow],
    checked: &HashMap<String, ThumbnailCheck>,
) -> Result<BlogMatch> {
    let matches: Vec<BlogRow> = rows
        .iter()
        .filter_map(|row| {
            let best = row
                .thumbnail_urls
                .iter()
                .filter_map(|url| match checked.get(url) {
                    Some(ThumbnailCheck::Distance(d)) => Some((url, *d)),
                    _ => None,
                })
                .min_by_key(|(_, d)| *d)?;
            (best.1 <= MATCH_DISTANCE).then(|| BlogRow {
                image_url: Some(best.0.clone()),
                hash_distance: Some(best.1),
                ..row.clone()
            })
        })
        .collect();
    let rank = matches.first().map(|m| m.rank);
    let failed_before = rows.iter().any(|row| {
        rank.is_none_or(|r| row.rank < r)
            && row
                .thumbnail_urls
                .iter()
                .any(|u| checked.get(u) == Some(&ThumbnailCheck::Error))
    });
    if failed_before {
        return Err(ScrapeError::domain(
            "일부 썸네일을 읽지 못해 첫 노출 순위를 확정할 수 없습니다. 다시 조회해 주세요.",
        ));
    }
    Ok(BlogMatch {
        status: if matches.is_empty() {
            "not_found"
        } else {
            "found"
        }
        .into(),
        rank,
        matches,
        total: rows.len(),
        compared_images: Some(checked.len()),
    })
}

// ---- 입력·반환 모양 (`blog.mjs:70-77` · `:109` · `:112`) ----

/// `check_blog` 입력. 인자가 없으면(`undefined`) PoC 기본값(`targetType='keyword'`, `target=''`, `imageName=''`).
#[derive(Debug, Clone, PartialEq)]
pub struct BlogInput {
    pub keyword: Option<Value>,
    pub target_type: Value,
    pub target: Value,
    pub image_data: Option<Value>,
    pub image_name: Value,
}

impl BlogInput {
    pub fn new(
        keyword: Option<Value>,
        target_type: Option<Value>,
        target: Option<Value>,
        image_data: Option<Value>,
        image_name: Option<Value>,
    ) -> Self {
        Self {
            keyword,
            target_type: target_type.unwrap_or_else(|| Value::from("keyword")),
            target: target.unwrap_or_else(|| Value::from("")),
            image_data,
            image_name: image_name.unwrap_or_else(|| Value::from("")),
        }
    }

    pub fn is_image(&self) -> bool {
        self.target_type == "image"
    }

    /// `blogUrl(keyword)` — 키워드는 trim 하지 않는다(PoC 그대로).
    pub fn search_url(&self) -> String {
        blog_url(&js::to_js_string(self.keyword.as_ref()))
    }

    /// `scope` (`blog.mjs:72`)
    pub fn scope(&self) -> String {
        format!(
            "PC 네이버 블로그 탭 · 초기 목록 + 최대 3회 스크롤 · {}",
            if self.is_image() {
                "검색 썸네일 pHash(거리 6 이하)"
            } else {
                "제목·요약 키워드 일치"
            }
        )
    }

    /// 반환 `target` (`blog.mjs:72`) — 이미지 모드는 `imageName || '업로드 이미지'`.
    pub fn result_target(&self) -> Value {
        if self.is_image() {
            if js::truthy(Some(&self.image_name)) {
                self.image_name.clone()
            } else {
                Value::from("업로드 이미지")
            }
        } else {
            self.target.clone()
        }
    }

    /// 입력 검증 3종 (`blog.mjs:74-76`). 명령 실패가 아니라 `status: "error"` 로 돌아간다.
    pub fn validate(&self) -> Result<()> {
        let bounded = |v: Option<&Value>| {
            v.and_then(Value::as_str)
                .is_some_and(|s| !js::trim(s).is_empty() && js::length(s) <= 100)
        };
        if !bounded(self.keyword.as_ref()) {
            return Err(ScrapeError::domain(
                "검색 키워드는 1~100자로 입력해 주세요.",
            ));
        }
        if self.target_type != "keyword" && self.target_type != "image" {
            return Err(ScrapeError::domain("타겟 종류를 확인해 주세요."));
        }
        if self.target_type == "keyword" && !bounded(Some(&self.target)) {
            return Err(ScrapeError::domain(
                "타겟 키워드는 1~100자로 입력해 주세요.",
            ));
        }
        Ok(())
    }
}

/// `checkBlog` 반환. 성공이면 `scrolls`, 이미지 모드면 `comparedImages` 가 붙고 오류면 둘 다 없다.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct BlogResult {
    pub keyword: Value,
    pub target: Value,
    pub target_type: Value,
    pub search_url: String,
    pub scope: String,
    pub status: String,
    pub rank: Option<usize>,
    pub matches: Vec<BlogRow>,
    pub total: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub compared_images: Option<usize>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scrolls: Option<u32>,
    pub checked_at: String,
    pub message: String,
}

/// `checkBlog` 반환 조립 (`blog.mjs:109` · `:112`). `outcome` 은 마지막 배치 판정과 스크롤 수.
pub fn blog_result(
    input: &BlogInput,
    outcome: Result<(BlogMatch, u32)>,
    checked_at: String,
) -> BlogResult {
    let base = BlogResult {
        keyword: input.keyword.clone().unwrap_or(Value::Null),
        target: input.result_target(),
        target_type: input.target_type.clone(),
        search_url: input.search_url(),
        scope: input.scope(),
        status: "error".into(),
        rank: None,
        matches: Vec::new(),
        total: None,
        compared_images: None,
        scrolls: None,
        checked_at,
        message: String::new(),
    };
    match outcome {
        Ok((m, scrolls)) => BlogResult {
            message: if m.status == "not_found" {
                NOT_FOUND_MESSAGE.to_string()
            } else {
                String::new()
            },
            status: m.status,
            rank: m.rank,
            matches: m.matches,
            total: Some(m.total),
            compared_images: m.compared_images,
            scrolls: Some(scrolls),
            ..base
        },
        Err(error) => BlogResult {
            message: error.user_message(Area::Blog),
            ..base
        },
    }
}

// ---- 썸네일 다운로드 (`blog.mjs:54-57` curl) · `checkBlog` (`:70-114`) ----

use std::time::{Duration, Instant};

use serde_json::{json, Map};

use crate::blog_browser::BlogBatches;
use crate::browser::BrowserManager;

static HTTP: LazyLock<std::result::Result<reqwest::Client, String>> = LazyLock::new(|| {
    // rustls 암호 제공자를 ring 으로 고정한다(reqwest `rustls-no-provider`). 이미 설치돼 있으면 그대로 쓴다.
    let _ = rustls::crypto::ring::default_provider().install_default();
    reqwest::Client::builder()
        .user_agent(THUMBNAIL_UA)
        // curl 은 리다이렉트를 따라가지 않는다(-L 없음).
        .redirect(reqwest::redirect::Policy::none())
        .timeout(Duration::from_secs(THUMBNAIL_TIMEOUT_SECS))
        .build()
        .map_err(|e| e.to_string())
});

/// `curl --fail --compressed --max-time 8 --max-filesize 6291456 --user-agent UA <url>`
pub async fn download_thumbnail(url: &str) -> Result<Vec<u8>> {
    let client = HTTP
        .as_ref()
        .map_err(|e| ScrapeError::other(anyhow!("http client: {e}")))?;
    let mut response = client.get(url).send().await.map_err(ScrapeError::other)?;
    let status = response.status();
    if !status.is_success() {
        return Err(ScrapeError::other(anyhow!("HTTP {status} for {url}")));
    }
    if response
        .content_length()
        .is_some_and(|n| n > THUMBNAIL_MAX_BYTES)
    {
        return Err(ScrapeError::other(anyhow!("Maximum file size exceeded")));
    }
    let mut body = Vec::new();
    while let Some(chunk) = response.chunk().await.map_err(ScrapeError::other)? {
        body.extend_from_slice(&chunk);
        if body.len() as u64 > THUMBNAIL_MAX_BYTES {
            return Err(ScrapeError::other(anyhow!("Maximum file size exceeded")));
        }
    }
    Ok(body)
}

/// `[blog]` 로그 한 줄 (`blog.mjs:84·90·108·111`).
fn log_blog(fields: Vec<(&str, Value)>) {
    let line: Map<String, Value> = fields
        .into_iter()
        .map(|(k, v)| (k.to_string(), v))
        .collect();
    log::info!("[blog] {}", Value::Object(line));
}

/// `checkBlog(keyword, { targetType, target, imageData, imageName })`
pub async fn check_blog(browser: &BrowserManager, input: &BlogInput) -> BlogResult {
    let started = Instant::now();
    let keyword = input.keyword.clone().unwrap_or(Value::Null);
    let outcome = run_blog(browser, input, &keyword, started).await;
    if let Err(e) = &outcome {
        let message: String = format!("{e:#}").chars().take(200).collect();
        log_blog(vec![
            ("keyword", keyword.clone()),
            ("stage", "error".into()),
            ("elapsedMs", (started.elapsed().as_millis() as u64).into()),
            ("message", message.into()),
        ]);
    }
    blog_result(input, outcome, js::now_iso())
}

async fn run_blog(
    browser: &BrowserManager,
    input: &BlogInput,
    keyword: &Value,
    started: Instant,
) -> Result<(BlogMatch, u32)> {
    input.validate()?;
    let targets: Vec<String> = if input.is_image() {
        let buffer = decode_target_image(input.image_data.as_ref())?;
        target_hashes(&buffer)?.to_vec()
    } else {
        Vec::new()
    };
    let mut batches = BlogBatches::new(input.search_url());
    let result = collect_batches(browser, input, keyword, &targets, &mut batches).await;
    batches.close(browser).await;
    let (m, scrolls) = result?;
    log_blog(vec![
        ("keyword", keyword.clone()),
        ("stage", "complete".into()),
        ("elapsedMs", (started.elapsed().as_millis() as u64).into()),
        ("rank", json!(m.rank)),
        ("total", m.total.into()),
        ("scrolls", scrolls.into()),
    ]);
    Ok((m, scrolls))
}

async fn collect_batches(
    browser: &BrowserManager,
    input: &BlogInput,
    keyword: &Value,
    targets: &[String],
    batches: &mut BlogBatches,
) -> Result<(BlogMatch, u32)> {
    let mut result: Option<BlogMatch> = None;
    let mut last_scrolls = 0;
    let mut checked: HashMap<String, ThumbnailCheck> = HashMap::new();
    while let Some(batch) = batches.next(browser).await? {
        last_scrolls = batch.scrolls;
        let rows = parse_blog_list(&batch.html)?;
        log_blog(vec![
            ("keyword", keyword.clone()),
            ("stage", "batch".into()),
            ("scrolls", batch.scrolls.into()),
            ("total", rows.len().into()),
        ]);
        let m = if !input.is_image() {
            match_blog_keyword(&rows, input.target.as_str().unwrap_or_default())?
        } else {
            let urls = new_thumbnail_urls(&rows, &checked)?;
            log_blog(vec![
                ("keyword", keyword.clone()),
                ("stage", "images-start".into()),
                ("images", urls.len().into()),
                ("concurrency", THUMBNAIL_CONCURRENCY.into()),
            ]);
            let checks = map_concurrent(urls, THUMBNAIL_CONCURRENCY, |url, _| async move {
                let check = async {
                    let bytes = download_thumbnail(&image_url(&url)?).await?;
                    image_hash(&bytes)
                }
                .await;
                let check = match check {
                    Ok(hash) => ThumbnailCheck::Distance(min_distance(targets, &hash)),
                    Err(_) => ThumbnailCheck::Error,
                };
                (url, check)
            })
            .await;
            checked.extend(checks);
            judge_image_batch(&rows, &checked)?
        };
        let found = m.status == "found";
        result = Some(m);
        if found {
            break;
        }
    }
    let m = result.ok_or_else(|| ScrapeError::domain("블로그 결과를 읽지 못했습니다."))?;
    Ok((m, last_scrolls))
}
