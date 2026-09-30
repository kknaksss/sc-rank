//! 브라우저 탐색 (SPEC-001 §3 · D-12). CDP 기동·재사용·정리는 P3.

use std::path::{Path, PathBuf};

use crate::errors::{Result, ScrapeError};

/// SPEC-001 §3 「없을 때」 — 도메인 오류로 그대로 낸다.
pub const NOT_FOUND_MESSAGE: &str =
    "Edge 또는 Chrome 을 찾지 못했습니다. 둘 중 하나를 설치한 뒤 다시 조회해 주세요.";

/// 후보 경로를 순서대로 보고 처음 있는 실행 파일을 고른다.
pub fn find_browser<P: AsRef<Path>>(candidates: &[P]) -> Result<PathBuf> {
    candidates
        .iter()
        .map(AsRef::as_ref)
        .find(|p| p.is_file())
        .map(Path::to_path_buf)
        .ok_or_else(|| ScrapeError::domain(NOT_FOUND_MESSAGE))
}

/// OS 표준 설치 경로 — Edge 먼저, 그다음 Chrome.
#[cfg(windows)]
pub fn default_candidates() -> Vec<PathBuf> {
    let roots: Vec<PathBuf> = ["ProgramFiles", "ProgramFiles(x86)", "LOCALAPPDATA"]
        .iter()
        .filter_map(|var| std::env::var_os(var).map(PathBuf::from))
        .collect();
    let mut out = Vec::new();
    for rel in [
        r"Microsoft\Edge\Application\msedge.exe",
        r"Google\Chrome\Application\chrome.exe",
    ] {
        for root in &roots {
            out.push(root.join(rel));
        }
    }
    out
}

/// OS 표준 설치 경로 — Edge 먼저, 그다음 Chrome.
#[cfg(target_os = "macos")]
pub fn default_candidates() -> Vec<PathBuf> {
    vec![
        PathBuf::from("/Applications/Microsoft Edge.app/Contents/MacOS/Microsoft Edge"),
        PathBuf::from("/Applications/Google Chrome.app/Contents/MacOS/Google Chrome"),
    ]
}

/// 지원 OS(Windows·macOS) 밖에서는 후보가 없다.
#[cfg(not(any(windows, target_os = "macos")))]
pub fn default_candidates() -> Vec<PathBuf> {
    Vec::new()
}

// ---- CDP 브라우저 관리 (SPEC-001 §3) ----

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};

use anyhow::anyhow;
use chromiumoxide::cdp::browser_protocol::browser::BrowserContextId;
use chromiumoxide::cdp::browser_protocol::emulation::{
    SetLocaleOverrideParams, SetUserAgentOverrideParams,
};
use chromiumoxide::cdp::browser_protocol::network::EnableParams as NetworkEnableParams;
use chromiumoxide::cdp::browser_protocol::page::{EventDomContentEventFired, NavigateParams};
use chromiumoxide::cdp::browser_protocol::target::{
    CreateBrowserContextParams, CreateTargetParams,
};
use chromiumoxide::cdp::js_protocol::runtime::{
    EvaluateParams, ExceptionDetails, ExecutionContextId,
};
use chromiumoxide::handler::viewport::Viewport;
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use serde_json::Value;
use tokio::task::JoinHandle;

/// 임시 프로필 디렉터리 이름 접두 (SPEC §3).
pub const PROFILE_PREFIX: &str = "sc-rank-cdp-";
/// 조회 단위 창 크기·언어 (SPEC §3, `place.mjs:59` · `blog-browser.mjs:8`).
pub const VIEWPORT: (u32, u32) = (1400, 900);
pub const LOCALE: &str = "ko-KR";
/// Playwright `chromium.launch` 기본 기동 제한(180초).
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(180);

fn other(e: impl std::fmt::Display) -> ScrapeError {
    ScrapeError::other(anyhow!("{e}"))
}

/// 비정상 종료로 남은 `sc-rank-cdp-*` 임시 프로필을 지운다(앱 기동 때 1회).
/// 이름의 pid(`sc-rank-cdp-<pid>-<nanos>`)가 살아 있으면 다른 앱·smoke 가 쓰는 중이므로 남긴다.
pub fn remove_stale_profiles() {
    remove_stale_profiles_in(&std::env::temp_dir());
}

/// `dir` 안의 잔여 프로필 정리 — 테스트용으로 폴더를 받는다.
pub fn remove_stale_profiles_in(dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some(rest) = name.strip_prefix(PROFILE_PREFIX) else {
            continue;
        };
        let owner = rest
            .split('-')
            .next()
            .and_then(|pid| pid.parse::<u32>().ok());
        if owner.is_some_and(pid_alive) {
            continue;
        }
        let _ = std::fs::remove_dir_all(entry.path());
    }
}

/// 프로세스가 살아 있는가. pid 0 과 i32 범위 밖은 살아 있지 않은 것으로 본다(`kill` 의 그룹 의미를 피한다).
#[cfg(unix)]
fn pid_alive(pid: u32) -> bool {
    let Ok(pid) = i32::try_from(pid) else {
        return false;
    };
    if pid <= 0 {
        return false;
    }
    // 신호 0 은 존재·권한만 확인한다. EPERM 은 살아 있지만 남의 프로세스라는 뜻.
    let rc = unsafe { libc::kill(pid, 0) };
    rc == 0 || std::io::Error::last_os_error().raw_os_error() == Some(libc::EPERM)
}

/// 프로세스가 살아 있는가.
#[cfg(windows)]
fn pid_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, STILL_ACTIVE};
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };
    if pid == 0 {
        return false;
    }
    unsafe {
        let handle = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid);
        if handle.is_null() {
            return false;
        }
        let mut code = 0u32;
        let ok = GetExitCodeProcess(handle, &mut code) != 0;
        CloseHandle(handle);
        ok && code == STILL_ACTIVE as u32
    }
}

struct Running {
    browser: Browser,
    alive: Arc<AtomicBool>,
    handler: JoinHandle<()>,
    profile: PathBuf,
    user_agent: String,
}

/// 앱당 헤드리스 브라우저 1개. 블로그·플레이스가 공유·재사용하고, 끊겼으면 다음 조회에서 다시 띄운다.
pub struct BrowserManager {
    candidates: Vec<PathBuf>,
    running: tokio::sync::Mutex<Option<Running>>,
}

/// 조회 하나의 격리 컨텍스트와 그 페이지.
pub struct Context {
    pub page: Page,
    id: BrowserContextId,
}

impl Default for BrowserManager {
    fn default() -> Self {
        Self::new(default_candidates())
    }
}

impl BrowserManager {
    pub fn new(candidates: Vec<PathBuf>) -> Self {
        Self {
            candidates,
            running: tokio::sync::Mutex::new(None),
        }
    }

    async fn launch(&self) -> Result<Running> {
        let executable = find_browser(&self.candidates)?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let profile =
            std::env::temp_dir().join(format!("{PROFILE_PREFIX}{}-{nanos}", std::process::id()));
        let config = BrowserConfig::builder()
            .chrome_executable(&executable)
            .user_data_dir(&profile)
            .new_headless_mode()
            .window_size(VIEWPORT.0, VIEWPORT.1)
            .viewport(Viewport {
                width: VIEWPORT.0,
                height: VIEWPORT.1,
                device_scale_factor: Some(1.0),
                emulating_mobile: false,
                is_landscape: false,
                has_touch: false,
            })
            .launch_timeout(LAUNCH_TIMEOUT)
            .build()
            .map_err(other)?;
        let (browser, mut handler) = match Browser::launch(config).await {
            Ok(pair) => pair,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&profile);
                return Err(other(format!("browser launch failed: {e}")));
            }
        };
        let alive = Arc::new(AtomicBool::new(true));
        let flag = alive.clone();
        let handler = tokio::spawn(async move {
            while let Some(event) = handler.next().await {
                if let Err(e) = event {
                    log::warn!("[browser] CDP connection ended: {e}");
                    break;
                }
            }
            flag.store(false, Ordering::SeqCst);
        });
        let mut running = Running {
            browser,
            alive,
            handler,
            profile,
            user_agent: String::new(),
        };
        match running.browser.user_agent().await {
            // 실행 브라우저의 실제 UA 에서 HeadlessChrome → Chrome (SPEC §3)
            Ok(ua) => running.user_agent = ua.replace("HeadlessChrome", "Chrome"),
            Err(e) => {
                stop(running).await;
                return Err(other(e));
            }
        }
        log::info!(
            "[browser] {}",
            serde_json::json!({ "stage": "launched", "executable": executable.display().to_string() })
        );
        Ok(running)
    }

    /// 격리 컨텍스트 + 페이지를 연다(`browser.newContext` + `newPage`). ko-KR · 1400×900 · UA 덮어쓰기.
    pub async fn open_context(&self) -> Result<Context> {
        let mut guard = self.running.lock().await;
        if let Some(r) = guard.as_mut() {
            let exited = matches!(r.browser.try_wait(), Ok(Some(_)));
            if exited || !r.alive.load(Ordering::SeqCst) {
                if let Some(dead) = guard.take() {
                    stop(dead).await;
                }
            }
        }
        if guard.is_none() {
            *guard = Some(self.launch().await?);
        }
        let running = guard.as_ref().expect("launched above");
        let id = running
            .browser
            .create_browser_context(CreateBrowserContextParams::default())
            .await
            .map_err(other)?;
        let page = match running
            .browser
            .new_page(
                CreateTargetParams::builder()
                    .url("about:blank")
                    .browser_context_id(id.clone())
                    .build()
                    .map_err(other)?,
            )
            .await
        {
            Ok(page) => page,
            Err(e) => {
                let _ = running.browser.dispose_browser_context(id).await;
                return Err(other(e));
            }
        };
        let setup = async {
            page.execute(NetworkEnableParams::default()).await?;
            page.execute(
                SetUserAgentOverrideParams::builder()
                    .user_agent(running.user_agent.clone())
                    .accept_language(LOCALE)
                    .build()
                    .map_err(chromiumoxide::error::CdpError::msg)?,
            )
            .await?;
            page.execute(SetLocaleOverrideParams::builder().locale(LOCALE).build())
                .await?;
            Ok::<_, chromiumoxide::error::CdpError>(())
        };
        if let Err(e) = setup.await {
            let _ = running.browser.dispose_browser_context(id).await;
            return Err(other(e));
        }
        Ok(Context { page, id })
    }

    /// 조회가 끝나면 컨텍스트를 닫는다(`context.close()`). 실패는 무시한다.
    pub async fn close_context(&self, context: Context) {
        let guard = self.running.lock().await;
        if let Some(r) = guard.as_ref() {
            let _ = r.browser.dispose_browser_context(context.id).await;
        }
    }

    /// 앱 종료 — 브라우저 프로세스를 끝내고 임시 프로필을 지운다.
    pub async fn shutdown(&self) {
        if let Some(r) = self.running.lock().await.take() {
            stop(r).await;
        }
    }
}

async fn stop(mut r: Running) {
    let exited = matches!(r.browser.try_wait(), Ok(Some(_)));
    if !exited {
        let _ = tokio::time::timeout(Duration::from_secs(5), r.browser.close()).await;
        if tokio::time::timeout(Duration::from_secs(5), r.browser.wait())
            .await
            .is_err()
        {
            let _ = r.browser.kill().await;
        }
    }
    r.handler.abort();
    // Windows 는 프로세스가 끝난 직후 파일 잠금이 잠깐 남는다.
    for _ in 0..20 {
        if std::fs::remove_dir_all(&r.profile).is_ok() || !r.profile.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    log::info!(
        "[browser] {}",
        serde_json::json!({ "stage": "closed", "profileRemoved": !r.profile.exists() })
    );
}

// ---- 페이지 조작 ----

/// Playwright `page.content()` 와 같은 식.
const CONTENT_JS: &str = "(() => { let retVal = ''; if (document.doctype) retVal = new XMLSerializer().serializeToString(document.doctype); if (document.documentElement) retVal += document.documentElement.outerHTML; return retVal; })()";

impl Context {
    /// `page.goto(url, { waitUntil: 'domcontentloaded', timeout })`
    pub async fn goto(&self, url: &str, timeout: Duration) -> Result<()> {
        let mut dom_ready = self
            .page
            .event_listener::<EventDomContentEventFired>()
            .await
            .map_err(other)?;
        let work = async {
            let nav = self
                .page
                .execute(NavigateParams::new(url))
                .await
                .map_err(other)?;
            if let Some(err) = nav.result.error_text.clone() {
                return Err(other(format!("page.goto: {err} at {url}")));
            }
            dom_ready
                .next()
                .await
                .ok_or_else(|| other("page closed during navigation"))?;
            Ok(())
        };
        tokio::time::timeout(timeout, work).await.map_err(|_| {
            other(format!(
                "page.goto: Timeout {}ms exceeded",
                timeout.as_millis()
            ))
        })?
    }

    /// 페이지(또는 프레임 실행 맥락)에서 식을 평가한다. 프라미스는 기다리고 값으로 돌려받는다.
    pub async fn eval(
        &self,
        context: Option<ExecutionContextId>,
        expression: &str,
    ) -> Result<Value> {
        let mut params = EvaluateParams::builder()
            .expression(expression)
            .await_promise(true)
            .return_by_value(true)
            .build()
            .map_err(other)?;
        params.context_id = context;
        let res = self.page.execute(params).await.map_err(other)?;
        if let Some(details) = &res.result.exception_details {
            return Err(page_error(details));
        }
        Ok(res.result.result.value.clone().unwrap_or(Value::Null))
    }

    /// `page.content()`
    pub async fn content(&self) -> Result<String> {
        match self.eval(None, CONTENT_JS).await? {
            Value::String(s) => Ok(s),
            other_value => Err(other(format!("unexpected content: {other_value}"))),
        }
    }

    /// 이름이 `name` 이고 (있다면) `url_contains` 를 포함하는 프레임의 기본 실행 맥락.
    pub async fn frame_context(
        &self,
        name: &str,
        url_contains: Option<&str>,
    ) -> Result<Option<ExecutionContextId>> {
        for frame in self.page.frames().await.map_err(other)? {
            let frame_name = self.page.frame_name(frame.clone()).await.map_err(other)?;
            if frame_name.as_deref() != Some(name) {
                continue;
            }
            if let Some(part) = url_contains {
                let url = self.page.frame_url(frame.clone()).await.map_err(other)?;
                if !url.is_some_and(|u| u.contains(part)) {
                    continue;
                }
            }
            if let Some(ctx) = self
                .page
                .frame_execution_context(frame)
                .await
                .map_err(other)?
            {
                return Ok(Some(ctx));
            }
        }
        Ok(None)
    }

    /// 조건 식이 참이 될 때까지 기다린다(Playwright `waitFor({ state: 'attached' })` 의 폴링).
    pub async fn wait_for<F, Fut>(&self, timeout: Duration, what: &str, mut check: F) -> Result<()>
    where
        F: FnMut() -> Fut,
        Fut: std::future::Future<Output = Result<bool>>,
    {
        let deadline = Instant::now() + timeout;
        loop {
            // 페이지가 아직 준비되지 않아 생긴 평가 오류는 다시 본다.
            if let Ok(true) = check().await {
                return Ok(());
            }
            if Instant::now() >= deadline {
                return Err(other(format!(
                    "locator.waitFor: Timeout {}ms exceeded waiting for {what}",
                    timeout.as_millis()
                )));
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
        }
    }
}

/// 페이지 안 JS 예외 → PoC 규칙: 메시지에 한글이 있으면 도메인 오류(문구 그대로), 아니면 폴백.
fn page_error(details: &ExceptionDetails) -> ScrapeError {
    let description = details
        .exception
        .as_ref()
        .and_then(|e| e.description.clone())
        .unwrap_or_else(|| details.text.clone());
    let first = description.lines().next().unwrap_or_default();
    let message = first.strip_prefix("Error: ").unwrap_or(first).to_string();
    if message
        .chars()
        .any(|c| ('\u{AC00}'..='\u{D7A3}').contains(&c))
    {
        ScrapeError::domain(message)
    } else {
        other(format!("page.evaluate: {description}"))
    }
}
