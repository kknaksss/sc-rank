//! 플레이스 판정 — PoC `server/place.mjs` 의 `PLACE_SCOPE`·`placeUrl`·`extractPlaceName`·`parsePlaceList`·`checkPlace` 반환 모양.
//! 수집(`collectPlaceList`)은 P3.

use std::sync::LazyLock;

use regex::Regex;
use serde::{Deserialize, Serialize};

use crate::errors::{Area, Result, ScrapeError};
use crate::js;

/// `place.mjs:4`
pub const PLACE_SCOPE: &str = "PC 네이버지도 플레이스 · 광고 포함 / 광고 제외 · 페이지 전체 수집 후 타겟 확인(최대 5페이지) · 헤드리스 기본 위치(위치 고정 없음)";
/// `place.mjs:116`
pub const NOT_FOUND_MESSAGE: &str = "수집한 최대 5페이지 목록에서 찾지 못했습니다.";
/// `place.mjs:114` · `index.mjs:35`
pub const INPUT_MESSAGE: &str = "키워드는 1~100자, 병원명은 1~50자로 입력해 주세요.";

/// JS `\s` 문자 집합 (정규식 안에서 쓰는 형태).
const JS_SPACE: &str = r"[\t\n\x0B\x0C\r \x{A0}\x{1680}\x{2000}-\x{200A}\x{2028}\x{2029}\x{202F}\x{205F}\x{3000}\x{FEFF}]";

static GENERIC: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"^(?:성형외과|피부과|외과|의원|병원|클리닉|성형외과의원)$").expect("generic regex")
});
static NAME_SUFFIX: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(&format!(r"(?:의원|병원|클리닉|외과)(?:{JS_SPACE}|$)")).expect("suffix regex")
});
static NOT_NAME: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"리뷰|이미지|진료|현재 위치|전문의|[0-9]+명").expect("not-name regex")
});
static SHORT_WORD: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"^[가-힣A-Za-z]{2,20}$").expect("short-word regex"));

/// `place.mjs:5`
pub fn place_url(keyword: &str) -> String {
    format!(
        "https://map.naver.com/p/search/{}?searchType=place",
        js::encode_uri_component(keyword)
    )
}

/// `place.mjs:6` — `String(value || '').replace(/\s/g, '')`
fn compact(value: Option<&str>) -> String {
    js::strip_spaces(value.unwrap_or(""))
}

/// 플레이스 목록 항목. 페이지 안 추출 결과(`place.mjs:79`)와 `collectPlaceList` 결과(`:88`)를 함께 담는다.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaceItem {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub spans: Option<Vec<String>>,
    #[serde(default)]
    pub id: Option<String>,
    #[serde(default)]
    pub ad: bool,
    #[serde(default)]
    pub reviews: Option<String>,
    #[serde(default)]
    pub addr: Option<String>,
    #[serde(default)]
    pub page: Option<u32>,
}

/// `parsePlaceList` 의 행 — `{ ...item, name, ad, rank, organicRank, page }`
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaceRow {
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub title: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub spans: Option<Vec<String>>,
    pub id: Option<String>,
    pub ad: bool,
    pub reviews: Option<String>,
    pub addr: Option<String>,
    pub rank: usize,
    pub organic_rank: Option<usize>,
    pub page: u32,
}

/// `parsePlaceList` 반환.
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaceParse {
    pub status: String,
    pub rank: Option<usize>,
    pub organic_rank: Option<usize>,
    pub page: Option<u32>,
    pub total: usize,
    pub total_ads: usize,
    pub matches: Vec<PlaceRow>,
    pub rows: Vec<PlaceRow>,
}

/// `checkPlace` 반환 (성공·오류 공통 모양, `place.mjs:112-118`).
#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct PlaceResult {
    pub keyword: String,
    pub target: String,
    pub search_url: String,
    pub scope: String,
    pub checked_at: String,
    pub status: String,
    pub rank: Option<usize>,
    pub organic_rank: Option<usize>,
    pub page: Option<u32>,
    pub total: Option<usize>,
    pub total_ads: Option<usize>,
    pub matches: Vec<PlaceRow>,
    pub rows: Vec<PlaceRow>,
    pub message: String,
}

/// `extractPlaceName` (`place.mjs:9-20`)
pub fn extract_place_name(item: &PlaceItem) -> String {
    if let Some(name) = item.name.as_deref().filter(|n| !n.is_empty()) {
        if !GENERIC.is_match(&compact(Some(name))) {
            return js::trim(name).to_string();
        }
    }
    // A title element can contain multiple highlighted spans. Its full text wins.
    let spans: &[String] = item.spans.as_deref().unwrap_or(&[]);
    let candidates = item
        .title
        .iter()
        .chain(spans.iter())
        .map(|x| js::trim(&js::collapse_spaces(x)).to_string());
    for x in candidates {
        if js::length(&x) < 80
            && NAME_SUFFIX.is_match(&x)
            && !GENERIC.is_match(&compact(Some(&x)))
            && !NOT_NAME.is_match(&x)
        {
            return x;
        }
    }
    for i in 1..spans.len() {
        if GENERIC.is_match(&compact(Some(&spans[i])))
            && SHORT_WORD.is_match(&spans[i - 1])
            && !GENERIC.is_match(&spans[i - 1])
        {
            return format!("{}{}", spans[i - 1], spans[i]);
        }
    }
    String::new()
}

/// `parsePlaceList` (`place.mjs:22-34`)
pub fn parse_place_list(items: &[PlaceItem], target: &str) -> Result<PlaceParse> {
    let target_compact = compact(Some(target));
    if target_compact.is_empty() {
        return Err(ScrapeError::domain("타겟 병원명을 입력해 주세요."));
    }
    if items.is_empty() {
        return Err(ScrapeError::domain("플레이스 목록을 읽지 못했습니다."));
    }
    let mut organic = 0;
    let mut rows = Vec::with_capacity(items.len());
    for (i, item) in items.iter().enumerate() {
        let name = extract_place_name(item);
        if name.is_empty() {
            return Err(ScrapeError::domain(
                "상호를 읽지 못한 항목이 있어 순위를 계산할 수 없습니다.",
            ));
        }
        if !item.ad {
            organic += 1;
        }
        rows.push(PlaceRow {
            name,
            title: item.title.clone(),
            spans: item.spans.clone(),
            id: item.id.clone(),
            ad: item.ad,
            reviews: item.reviews.clone(),
            addr: item.addr.clone(),
            rank: i + 1,
            organic_rank: if item.ad { None } else { Some(organic) },
            page: item.page.filter(|p| *p != 0).unwrap_or(1),
        });
    }
    let matches: Vec<PlaceRow> = rows
        .iter()
        .filter(|row| compact(Some(&row.name)).contains(&target_compact))
        .cloned()
        .collect();
    Ok(PlaceParse {
        status: if matches.is_empty() {
            "not_found"
        } else {
            "found"
        }
        .into(),
        rank: matches.first().map(|m| m.rank),
        organic_rank: matches.iter().find(|m| !m.ad).and_then(|m| m.organic_rank),
        page: matches.first().map(|m| m.page),
        total: rows.len(),
        total_ads: rows.iter().filter(|r| r.ad).count(),
        matches,
        rows,
    })
}

/// `check_place` 입력 검증 (`index.mjs:35` · `place.mjs:114`). 통과하면 trim 한 `(keyword, target)`.
pub fn validate_place_input(
    keyword: Option<&serde_json::Value>,
    target: Option<&serde_json::Value>,
) -> std::result::Result<(String, String), String> {
    let ok = |v: Option<&serde_json::Value>, max: usize| match v.and_then(|v| v.as_str()) {
        Some(s) if !js::trim(s).is_empty() && js::length(s) <= max => Some(js::trim(s).to_string()),
        _ => None,
    };
    match (ok(keyword, 100), ok(target, 50)) {
        (Some(k), Some(t)) => Ok((k, t)),
        _ => Err(INPUT_MESSAGE.to_string()),
    }
}

/// `checkPlace` 반환 조립 (`place.mjs:112-118`). `parsed` 는 수집 → `parse_place_list` 결과.
pub fn place_result(
    keyword: &str,
    target: &str,
    parsed: Result<PlaceParse>,
    checked_at: String,
) -> PlaceResult {
    let base = PlaceResult {
        keyword: keyword.to_string(),
        target: target.to_string(),
        search_url: place_url(keyword),
        scope: PLACE_SCOPE.to_string(),
        checked_at,
        status: "error".to_string(),
        rank: None,
        organic_rank: None,
        page: None,
        total: None,
        total_ads: None,
        matches: Vec::new(),
        rows: Vec::new(),
        message: String::new(),
    };
    match parsed {
        Ok(r) => PlaceResult {
            message: if r.status == "not_found" {
                NOT_FOUND_MESSAGE.to_string()
            } else {
                String::new()
            },
            status: r.status,
            rank: r.rank,
            organic_rank: r.organic_rank,
            page: r.page,
            total: Some(r.total),
            total_ads: Some(r.total_ads),
            matches: r.matches,
            rows: r.rows,
            ..base
        },
        Err(error) => PlaceResult {
            message: error.user_message(Area::Place),
            ..base
        },
    }
}

// ---- 수집 (`place.mjs:50-109`) · `checkPlace` (`:111-120`) ----

use std::time::{Duration, Instant};

use serde_json::{json, Map, Value};

use crate::browser::{BrowserManager, Context};

/// PoC 페이지 안 JS — 원본 파일 그대로(`place-dom.mjs` 사본), `export` 만 떼어 평가한다.
const PLACE_DOM_JS: &str = include_str!("../js/place-dom.js");
/// `place.mjs:74-80` evaluateAll 함수 본문.
const PLACE_ITEMS_JS: &str = include_str!("../js/place-items.js");
/// `place.mjs:94` evaluateAll 함수 본문.
const PLACE_SIGNATURE_JS: &str = include_str!("../js/place-signature.js");
/// `place.mjs:95-99` evaluate 함수 본문.
const PLACE_NEXT_JS: &str = include_str!("../js/place-next.js");

const LIST_SELECTOR: &str = "#_pcmap_list_scroll_container a.uD1F4 > span:first-child";
const ITEM_SELECTOR: &str = "#_pcmap_list_scroll_container > ul > li";

fn place_dom() -> String {
    PLACE_DOM_JS.replace("export async function", "async function")
}

/// `evaluateAll(fn)` — `document.querySelectorAll(selector)` 를 배열로 넘긴다.
fn evaluate_all(function: &str, selector: &str) -> String {
    format!(
        "({})([...document.querySelectorAll({})])",
        function.trim(),
        Value::from(selector)
    )
}

/// `[place]` 단계 로그 — `{ keyword, stage, elapsedMs, ...details }` (`place.mjs:53`).
fn log_place(keyword: &str, started: Instant, stage: &str, details: Value) {
    let mut line = Map::new();
    line.insert("keyword".into(), keyword.into());
    line.insert("stage".into(), stage.into());
    line.insert(
        "elapsedMs".into(),
        (started.elapsed().as_millis() as u64).into(),
    );
    if let Value::Object(extra) = details {
        line.extend(extra);
    }
    log::info!("[place] {}", Value::Object(line));
}

/// `collectPlaceList(keyword, { maxPages = 5, target })`
pub async fn collect_place_list(
    browser: &BrowserManager,
    keyword: &str,
    max_pages: u32,
    target: Option<&str>,
) -> Result<Vec<PlaceItem>> {
    if !(1..=5).contains(&max_pages) {
        return Err(ScrapeError::domain("조회 페이지는 1~5 사이여야 합니다."));
    }
    let started = Instant::now();
    log_place(keyword, started, "start", json!({ "maxPages": max_pages }));
    let mut slot: Option<Context> = None;
    let work = collect_work(browser, &mut slot, keyword, max_pages, target, started);
    let result = match tokio::time::timeout(Duration::from_secs(60), work).await {
        Ok(r) => r,
        Err(_) => Err(ScrapeError::domain(
            "조회 제한 시간 60초를 초과했습니다. 다시 조회해 주세요.",
        )),
    };
    if let Err(e) = &result {
        log_place(
            keyword,
            started,
            "error",
            json!({ "message": format!("{e:#}") }),
        );
    }
    if let Some(ctx) = slot.take() {
        browser.close_context(ctx).await;
    }
    result
}

async fn collect_work(
    browser: &BrowserManager,
    slot: &mut Option<Context>,
    keyword: &str,
    max_pages: u32,
    target: Option<&str>,
    started: Instant,
) -> Result<Vec<PlaceItem>> {
    let ctx = slot.insert(browser.open_context().await?);
    log_place(keyword, started, "browser-ready", json!({}));
    ctx.goto(&place_url(keyword), Duration::from_secs(30))
        .await?;
    log_place(keyword, started, "navigation-ready", json!({}));
    // frameLocator('iframe#searchIframe, iframe[name="searchIframe"]').locator(LIST).first().waitFor({ state: 'attached', timeout: 20000 })
    let attached = format!("!!document.querySelector({})", Value::from(LIST_SELECTOR));
    ctx.wait_for(Duration::from_secs(20), "searchIframe list", || async {
        match ctx.frame_context("searchIframe", None).await? {
            Some(frame) => Ok(ctx.eval(Some(frame), &attached).await? == Value::Bool(true)),
            None => Ok(false),
        }
    })
    .await?;
    // const frame = () => page.frames().find(f => f.name() === 'searchIframe' && f.url().includes('pcmap'))
    // 루프 안에서 프레임이 사라지면 PoC 는 `frame().evaluate` 가 TypeError(영어)라 폴백 문구가 된다 — Other.
    let frame = || async {
        ctx.frame_context("searchIframe", Some("pcmap"))
            .await?
            .ok_or_else(|| {
                ScrapeError::other(anyhow::anyhow!(
                    "Cannot read properties of undefined (reading 'evaluate'): searchIframe frame missing"
                ))
            })
    };
    // `place.mjs:68` — 도메인 문구는 목록 준비 직후 이 한 곳에서만.
    if ctx
        .frame_context("searchIframe", Some("pcmap"))
        .await?
        .is_none()
    {
        return Err(ScrapeError::domain(
            "플레이스 검색 프레임을 읽지 못했습니다.",
        ));
    }
    log_place(keyword, started, "list-ready", json!({}));

    let dom = place_dom();
    let mut all: Vec<PlaceItem> = Vec::new();
    for page_no in 1..=max_pages {
        log_place(keyword, started, "page-start", json!({ "page": page_no }));
        let hydrate = format!("(async () => {{ {dom}\nreturn await hydratePlacePage(); }})()");
        let hydration = ctx.eval(Some(frame().await?), &hydrate).await?;
        let raw_value = ctx
            .eval(
                Some(frame().await?),
                &evaluate_all(PLACE_ITEMS_JS, ITEM_SELECTOR),
            )
            .await?;
        let raw: Vec<PlaceItem> = serde_json::from_value(raw_value)
            .map_err(|e| ScrapeError::other(anyhow::anyhow!("place items: {e}")))?;
        let mut details = Map::new();
        details.insert("page".into(), page_no.into());
        if let Value::Object(h) = hydration {
            details.extend(h);
        }
        details.insert("captured".into(), raw.len().into());
        log_place(keyword, started, "scroll-complete", Value::Object(details));
        let mut added = 0;
        for item in &raw {
            let name = extract_place_name(item);
            if name.is_empty() {
                let has_title = item.title.as_deref().is_some_and(|t| !t.is_empty());
                if item.id.is_some() || has_title {
                    return Err(ScrapeError::domain("일부 업체의 상호를 읽지 못했습니다."));
                }
                continue;
            }
            added += 1;
            all.push(PlaceItem {
                name: Some(name),
                title: None,
                spans: None,
                id: item.id.clone(),
                ad: item.ad,
                reviews: item.reviews.clone(),
                addr: item.addr.clone(),
                page: Some(page_no),
            });
        }
        log_place(
            keyword,
            started,
            "page-complete",
            json!({ "page": page_no, "added": added, "total": all.len() }),
        );
        if added == 0 {
            if all.is_empty() {
                return Err(ScrapeError::domain("플레이스 목록을 읽지 못했습니다."));
            }
            break;
        }
        if let Some(t) = target {
            let t = compact(Some(t));
            if all.iter().any(|i| compact(i.name.as_deref()).contains(&t)) {
                log_place(keyword, started, "target-found", json!({ "page": page_no }));
                break;
            }
        }
        if page_no == max_pages {
            break;
        }
        let before = ctx
            .eval(
                Some(frame().await?),
                &evaluate_all(PLACE_SIGNATURE_JS, LIST_SELECTOR),
            )
            .await?;
        let next = page_no + 1;
        let clicked = ctx
            .eval(
                Some(frame().await?),
                &format!("({})({next})", PLACE_NEXT_JS.trim()),
            )
            .await?;
        if clicked != Value::Bool(true) {
            break;
        }
        let wait = format!(
            "(async () => {{ {dom}\nreturn await waitForPlacePageChange({}); }})()",
            json!({ "next": next, "before": before })
        );
        ctx.eval(Some(frame().await?), &wait).await?;
    }
    log_place(keyword, started, "complete", json!({ "total": all.len() }));
    Ok(all)
}

/// `checkPlace(keyword, target)` — 입력은 명령에서 trim·검증된 값이다.
pub async fn check_place(browser: &BrowserManager, keyword: &str, target: &str) -> PlaceResult {
    let parsed = async {
        validate_place_input(Some(&Value::from(keyword)), Some(&Value::from(target)))
            .map_err(ScrapeError::domain)?;
        let items = collect_place_list(browser, keyword, 5, Some(target)).await?;
        parse_place_list(&items, target)
    }
    .await;
    place_result(keyword, target, parsed, crate::js::now_iso())
}
