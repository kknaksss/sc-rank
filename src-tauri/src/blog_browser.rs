//! 블로그 탭 스크롤 수집 — PoC `server/blog-browser.mjs` `blogBatches`.
//! 배치마다 완전히 렌더된 HTML 을 돌려준다. 호출부가 일찍 끝내도 `close` 로 컨텍스트를 닫는다.

use std::sync::LazyLock;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use chromiumoxide::cdp::browser_protocol::network::{
    EventLoadingFailed, EventLoadingFinished, EventResponseReceived, GetResponseBodyParams,
    ResourceType,
};
use futures::StreamExt;
use scraper::{Html, Selector};
use serde_json::{json, Value};

use crate::browser::{BrowserManager, Context};
use crate::errors::{Result, ScrapeError};
use crate::js;

/// `blog-browser.mjs:25`
const SCROLL_JS: &str = include_str!("../js/blog-scroll.js");
/// `blog-browser.mjs:35-42`
const RENDERED_JS: &str = include_str!("../js/blog-rendered.js");
const CARD: &str = r#"[data-template-id="ugcItem"]"#;
/// 컨텍스트 수명 상한 (`blog-browser.mjs:9`).
const DEADLINE: Duration = Duration::from_secs(150);
/// `page.setDefaultTimeout(12000)` — 첫 카드 대기·추가 응답 대기에 걸린다.
const DEFAULT_TIMEOUT: Duration = Duration::from_secs(12);
const GOTO_TIMEOUT: Duration = Duration::from_secs(25);

static CARD_SELECTOR: LazyLock<Selector> =
    LazyLock::new(|| Selector::parse(CARD).expect("card selector"));

fn other(e: impl std::fmt::Display) -> ScrapeError {
    ScrapeError::other(anyhow!("{e}"))
}

pub struct Batch {
    pub html: String,
    pub scrolls: u32,
}

enum Step {
    Start,
    Scroll(u32),
    Done,
}

pub struct BlogBatches {
    url: String,
    max_scrolls: u32,
    ctx: Option<Context>,
    deadline: Instant,
    step: Step,
}

impl BlogBatches {
    /// `blogBatches(url, { maxScrolls = 3 })`
    pub fn new(url: String) -> Self {
        Self {
            url,
            max_scrolls: 3,
            ctx: None,
            deadline: Instant::now() + DEADLINE,
            step: Step::Start,
        }
    }

    /// 다음 배치. 끝이면 `None`.
    pub async fn next(&mut self, browser: &BrowserManager) -> Result<Option<Batch>> {
        match self.step {
            Step::Done => Ok(None),
            Step::Start => {
                let ctx = browser.open_context().await?;
                self.deadline = Instant::now() + DEADLINE;
                let ctx = self.ctx.insert(ctx);
                let deadline = self.deadline;
                let batch = within(deadline, async {
                    ctx.goto(&self.url, GOTO_TIMEOUT).await?;
                    let attached = format!("!!document.querySelector({})", Value::from(CARD));
                    ctx.wait_for(DEFAULT_TIMEOUT, "blog cards", || async {
                        Ok(ctx.eval(None, &attached).await? == Value::Bool(true))
                    })
                    .await?;
                    Ok(Batch {
                        html: ctx.content().await?,
                        scrolls: 0,
                    })
                })
                .await?;
                self.step = if self.max_scrolls >= 1 {
                    Step::Scroll(1)
                } else {
                    Step::Done
                };
                Ok(Some(batch))
            }
            Step::Scroll(scrolls) => {
                let ctx = self
                    .ctx
                    .as_ref()
                    .ok_or_else(|| other("blog context missing"))?;
                let (batch, more) = within(self.deadline, scroll_once(ctx, scrolls)).await?;
                self.step = match batch {
                    Some(_) if more && scrolls < self.max_scrolls => Step::Scroll(scrolls + 1),
                    _ => Step::Done,
                };
                Ok(batch)
            }
        }
    }

    /// `finally { context.close() }`
    pub async fn close(&mut self, browser: &BrowserManager) {
        self.step = Step::Done;
        if let Some(ctx) = self.ctx.take() {
            browser.close_context(ctx).await;
        }
    }
}

/// 150초가 지나면 PoC 는 컨텍스트를 닫아 다음 페이지 조작이 실패한다 — 같은 결과를 낸다.
async fn within<T>(
    deadline: Instant,
    fut: impl std::future::Future<Output = Result<T>>,
) -> Result<T> {
    tokio::time::timeout_at(deadline.into(), fut)
        .await
        .map_err(|_| other("Target page, context or browser has been closed (150s deadline)"))?
}

/// 스크롤 1회 (`blog-browser.mjs:17-44`). 반환: (배치, `payload.url` 이 있는가).
async fn scroll_once(ctx: &Context, scrolls: u32) -> Result<(Option<Batch>, bool)> {
    let count = format!("document.querySelectorAll({}).length", Value::from(CARD));
    let before = ctx.eval(None, &count).await?.as_u64().unwrap_or(0);
    // Register the response listener before the scroll triggers the request.
    let mut responses = ctx
        .page
        .event_listener::<EventResponseReceived>()
        .await
        .map_err(other)?;
    let mut finished = ctx
        .page
        .event_listener::<EventLoadingFinished>()
        .await
        .map_err(other)?;
    let mut failed = ctx
        .page
        .event_listener::<EventLoadingFailed>()
        .await
        .map_err(other)?;
    ctx.eval(None, &format!("({})()", SCROLL_JS.trim())).await?;
    let response = tokio::time::timeout(DEFAULT_TIMEOUT, async {
        while let Some(event) = responses.next().await {
            // Playwright 의 response 이벤트에는 CORS 사전 요청(OPTIONS 204)이 없다 — CDP 는 따로 보고한다.
            if event.r#type == ResourceType::Preflight {
                continue;
            }
            if url::Url::parse(&event.response.url).is_ok_and(|u| {
                u.host_str() == Some("s.search.naver.com") && u.path().starts_with("/p/review/")
            }) {
                return Some(event);
            }
        }
        None
    })
    .await
    .map_err(|_| other("page.waitForResponse: Timeout 12000ms exceeded"))?
    .ok_or_else(|| other("page closed while waiting for response"))?;
    let status = response.response.status;
    if !(200..300).contains(&status) {
        return Err(ScrapeError::domain(format!(
            "블로그 추가 결과 요청에 실패했습니다(HTTP {status})."
        )));
    }
    // response.json() — 본문이 다 온 뒤에 읽는다.
    let id = response.request_id.clone();
    loop {
        tokio::select! {
            Some(done) = finished.next() => if done.request_id == id { break },
            Some(fail) = failed.next() => if fail.request_id == id {
                return Err(other(format!("response body failed: {}", fail.error_text)));
            },
            else => return Err(other("page closed while reading response")),
        }
    }
    let body = ctx
        .page
        .execute(GetResponseBodyParams::new(id))
        .await
        .map_err(other)?;
    let text = if body.result.base64_encoded {
        String::from_utf8(decode_base64(&body.result.body)).map_err(other)?
    } else {
        body.result.body.clone()
    };
    let payload: Value = serde_json::from_str(&text).map_err(other)?;
    let Some(collection) = payload.get("collection").and_then(Value::as_array) else {
        return Err(ScrapeError::domain(
            "블로그 추가 결과 형식을 읽지 못했습니다.",
        ));
    };
    let added: usize = collection
        .iter()
        .map(|c| {
            let html = c
                .get("html")
                .and_then(Value::as_str)
                .filter(|s| !s.is_empty())
                .unwrap_or("");
            Html::parse_document(html).select(&CARD_SELECTOR).count()
        })
        .sum();
    let more = js::truthy(payload.get("url"));
    if added == 0 {
        if !more {
            return Ok((None, false));
        }
        return Err(ScrapeError::domain(
            "추가 결과를 읽지 못해 순위를 확정할 수 없습니다.",
        ));
    }
    let expected = before as usize + added;
    ctx.eval(
        None,
        &format!(
            "({})({})",
            RENDERED_JS.trim(),
            json!({ "expected": expected })
        ),
    )
    .await?;
    Ok((
        Some(Batch {
            html: ctx.content().await?,
            scrolls,
        }),
        more,
    ))
}

fn decode_base64(input: &str) -> Vec<u8> {
    fn value(c: u8) -> Option<u32> {
        Some(match c {
            b'A'..=b'Z' => c - b'A',
            b'a'..=b'z' => c - b'a' + 26,
            b'0'..=b'9' => c - b'0' + 52,
            b'+' => 62,
            b'/' => 63,
            _ => return None,
        } as u32)
    }
    let mut out = Vec::with_capacity(input.len() * 3 / 4);
    let (mut acc, mut n) = (0u32, 0);
    for v in input.bytes().filter_map(value) {
        acc = (acc << 6) | v;
        n += 1;
        if n == 4 {
            out.extend_from_slice(&[(acc >> 16) as u8, (acc >> 8) as u8, acc as u8]);
            (acc, n) = (0, 0);
        }
    }
    match n {
        2 => out.push((acc >> 4) as u8),
        3 => out.extend_from_slice(&[(acc >> 10) as u8, (acc >> 2) as u8]),
        _ => {}
    }
    out
}
