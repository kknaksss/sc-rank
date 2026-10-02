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
use chromiumoxide::async_process::Child;
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
use chromiumoxide::handler::HandlerConfig;
use chromiumoxide::{Browser, BrowserConfig, Page};
use futures::StreamExt;
use serde_json::Value;
use tokio::task::JoinHandle;

/// 임시 프로필 디렉터리 이름 접두 (SPEC §3).
pub const PROFILE_PREFIX: &str = "sc-rank-cdp-";
/// 조회 단위 창 크기·언어 (SPEC §3, `place.mjs:59` · `blog-browser.mjs:8`).
pub const VIEWPORT: (u32, u32) = (1400, 900);
pub const LOCALE: &str = "ko-KR";
/// 기동 제한 — 브라우저가 CDP 주소를 적기까지 기다리는 한계.
/// Playwright `chromium.launch` 기본값과 같은 180초.
const LAUNCH_TIMEOUT: Duration = Duration::from_secs(180);
/// 브라우저가 기동하면서 프로필에 쓰는 CDP 주소 파일 (1줄 포트 · 2줄 대상 경로).
const PORT_FILE: &str = "DevToolsActivePort";
/// 그 파일을 기다리는 폴링 간격.
const PORT_POLL: Duration = Duration::from_millis(50);
/// 기동한 프로세스가 끝난 뒤에도 주소 파일을 기다려 주는 시간.
const PORT_GRACE_AFTER_EXIT: Duration = Duration::from_secs(10);
/// 브라우저가 스스로 끝나기를 기다리는 한계(종료 · 프로필 잠금 해제).
const CLOSE_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// 우리가 띄운 프로세스. Windows 에서는 이게 브라우저 본체가 아닐 수 있다(아래 `launch` 주석).
    child: Child,
    alive: Arc<AtomicBool>,
    handler: JoinHandle<()>,
    stderr: JoinHandle<()>,
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

    /// 헤드리스 브라우저를 띄우고 CDP 로 붙는다.
    ///
    /// `Browser::launch` 를 쓰지 않는다. 그쪽은 **띄운 자식의 stderr** 에서
    /// `DevTools listening on ws://…` 줄을 읽어 접속 주소를 얻는데, Windows 의
    /// `msedge.exe`·`chrome.exe` 는 런처라서 조건이 맞으면(브라우저 업데이트 도중,
    /// 권한이 다른 부모에서 띄울 때) **실제 브라우저를 다른 프로세스로 넘기고 자신은
    /// 곧바로 exit 0 으로 끝낸다.** 그러면 stderr 는 빈 채로 닫히고 chromiumoxide 는
    /// `Browser process exited … before websocket URL could be resolved` 로 실패하는데,
    /// 넘겨받은 브라우저는 살아서 고아로 남는다(조회마다 하나씩 쌓인다).
    ///
    /// 그래서 접속 주소는 stderr 가 아니라 **브라우저 본체가 프로필에 쓰는
    /// `DevToolsActivePort`** 에서 읽고 `Browser::connect` 로 붙는다. 어느 프로세스가
    /// 브라우저가 되었든 상관없고, 붙은 뒤에는 CDP `Browser.close` 로 확실히 끝낼 수 있다.
    async fn launch(&self) -> Result<Running> {
        let executable = find_browser(&self.candidates)?;
        let nanos = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos())
            .unwrap_or_default();
        let profile =
            std::env::temp_dir().join(format!("{PROFILE_PREFIX}{}-{nanos}", std::process::id()));
        let viewport = Viewport {
            width: VIEWPORT.0,
            height: VIEWPORT.1,
            device_scale_factor: Some(1.0),
            emulating_mobile: false,
            is_landscape: false,
            has_touch: false,
        };
        // 기동 인자는 chromiumoxide 가 만든 것을 그대로 쓴다(`--remote-debugging-port=0`
        // 이므로 실제 포트는 브라우저가 `DevToolsActivePort` 에 적는다).
        let config = BrowserConfig::builder()
            .chrome_executable(&executable)
            .user_data_dir(&profile)
            .new_headless_mode()
            .window_size(VIEWPORT.0, VIEWPORT.1)
            .viewport(viewport.clone())
            .build()
            .map_err(other)?;
        let mut child = match config.launch() {
            Ok(child) => child,
            Err(e) => {
                let _ = std::fs::remove_dir_all(&profile);
                return Err(other(format!("browser launch failed: {e}")));
            }
        };
        // stderr 는 파이프다 — 아무도 읽지 않으면 버퍼가 차서 브라우저가 멈춘다. 읽어서 로그로 보낸다.
        let stderr = drain_stderr(&mut child);
        let ws_url = match devtools_ws_url(&profile, &mut child).await {
            Ok(url) => url,
            Err(e) => {
                stop_child(&mut child, Duration::ZERO).await;
                stderr.abort();
                remove_profile(&profile).await;
                return Err(e);
            }
        };
        let handler_config = HandlerConfig {
            viewport: Some(viewport),
            ..HandlerConfig::default()
        };
        let (browser, mut handler) =
            match Browser::connect_with_config(&*ws_url, handler_config).await {
                Ok(pair) => pair,
                Err(e) => {
                    stop_child(&mut child, Duration::ZERO).await;
                    stderr.abort();
                    remove_profile(&profile).await;
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
            child,
            alive,
            handler,
            stderr,
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
        // 끊긴 브라우저는 버리고 다시 띄운다. 판단은 CDP 연결 상태(`alive`)로 한다 —
        // `connect` 로 붙었으므로 우리가 띄운 프로세스의 종료 여부는 브라우저의 생존과 다르다.
        if let Some(r) = guard.as_mut() {
            if !r.alive.load(Ordering::SeqCst) {
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

/// 브라우저를 끝내고 임시 프로필을 지운다.
///
/// 끝내는 길이 둘이다 — CDP `Browser.close` 는 **붙어 있는 브라우저 본체**를 닫고,
/// `stop_child` 는 **우리가 띄운 프로세스**를 거둔다. Windows 에서 런처가 본체를 넘긴
/// 경우 이 둘이 다른 프로세스이므로 모두 해야 한다.
async fn stop(mut r: Running) {
    if r.alive.load(Ordering::SeqCst) {
        let _ = tokio::time::timeout(CLOSE_TIMEOUT, r.browser.close()).await;
        // CDP 연결이 끊기면 브라우저가 정말로 끝난 것이다. 넘겨받은 브라우저는 우리가
        // 프로세스로 거둘 수 없으니(핸들이 없다) 이 신호를 기다린다 — 끝나기 전에 프로필을
        // 지우려 하면 파일이 잠겨 있어 실패한다.
        let deadline = Instant::now() + CLOSE_TIMEOUT;
        while r.alive.load(Ordering::SeqCst) && Instant::now() < deadline {
            tokio::time::sleep(PORT_POLL).await;
        }
    }
    stop_child(&mut r.child, CLOSE_TIMEOUT).await;
    r.handler.abort();
    r.stderr.abort();
    remove_profile(&r.profile).await;
    log::info!(
        "[browser] {}",
        serde_json::json!({ "stage": "closed", "profileRemoved": !r.profile.exists() })
    );
}

/// 우리가 띄운 프로세스를 거둔다. 이미 끝났으면 거두기만 하고,
/// 살아 있으면 `grace` 동안 스스로 끝나기를 기다린 뒤 강제로 끝낸다.
async fn stop_child(child: &mut Child, grace: Duration) {
    match child.try_wait() {
        Ok(Some(_)) => return,
        Err(e) => log::warn!("[browser] try_wait failed: {e}"),
        Ok(None) => {}
    }
    if tokio::time::timeout(grace, child.wait()).await.is_err() {
        let _ = child.kill().await;
    }
}

/// 임시 프로필을 지운다. Windows 는 프로세스가 끝난 직후 파일 잠금이 잠깐 남는다.
async fn remove_profile(profile: &Path) {
    let deadline = Instant::now() + CLOSE_TIMEOUT;
    loop {
        if std::fs::remove_dir_all(profile).is_ok() || !profile.exists() {
            return;
        }
        if Instant::now() >= deadline {
            log::warn!(
                "[browser] 임시 프로필을 지우지 못했다(다음 기동 때 지운다): {}",
                profile.display()
            );
            return;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
}

/// 브라우저가 프로필에 적은 CDP 주소를 기다려 `ws://…` 로 만든다.
///
/// 파일은 두 줄이다 — 1줄 포트, 2줄 대상 경로(`/devtools/browser/<id>`). 쓰는 중에
/// 읽을 수 있으므로 두 줄이 다 채워질 때까지 기다린다.
///
/// 띄운 프로세스가 먼저 끝나도 바로 실패로 보지 않는다(런처가 본체를 넘긴 경우가 그렇다).
/// 다만 끝난 뒤로도 주소가 안 나오면 `PORT_GRACE_AFTER_EXIT` 만큼만 더 기다린다 —
/// 정말로 브라우저가 못 뜬 경우에 `LAUNCH_TIMEOUT` 을 꽉 채우지 않기 위해서다.
async fn devtools_ws_url(profile: &Path, child: &mut Child) -> Result<String> {
    let path = profile.join(PORT_FILE);
    let deadline = Instant::now() + LAUNCH_TIMEOUT;
    let mut exited_at: Option<Instant> = None;
    loop {
        if let Some(url) = read_ws_url(&path) {
            return Ok(url);
        }
        if exited_at.is_none() && matches!(child.try_wait(), Ok(Some(_))) {
            exited_at = Some(Instant::now());
        }
        let now = Instant::now();
        if now >= deadline {
            return Err(other(format!(
                "browser launch failed: Timeout {}ms exceeded waiting for {PORT_FILE}",
                LAUNCH_TIMEOUT.as_millis()
            )));
        }
        if exited_at.is_some_and(|at| now.duration_since(at) >= PORT_GRACE_AFTER_EXIT) {
            return Err(other(format!(
                "browser launch failed: browser process exited without writing {PORT_FILE}"
            )));
        }
        tokio::time::sleep(PORT_POLL).await;
    }
}

/// 주소 파일을 읽어 `ws://127.0.0.1:<포트><대상 경로>` 로 만든다. 아직 덜 쓰였으면 `None`.
fn read_ws_url(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let port = lines.next()?.trim();
    let target = lines.next()?.trim();
    if port.is_empty() || !target.starts_with('/') {
        return None;
    }
    port.parse::<u16>().ok()?;
    Some(format!("ws://127.0.0.1:{port}{target}"))
}

/// stderr 를 끝까지 읽어 로그로 보낸다. 읽지 않으면 파이프가 차서 브라우저가 멈춘다.
fn drain_stderr(child: &mut Child) -> JoinHandle<()> {
    let Some(stderr) = child.stderr.take() else {
        return tokio::spawn(async {});
    };
    tokio::spawn(async move {
        use futures::AsyncBufReadExt;
        let mut lines = futures::io::BufReader::new(stderr).lines();
        while let Some(line) = lines.next().await {
            match line {
                Ok(line) if !line.trim().is_empty() => log::debug!("[browser] stderr: {line}"),
                Ok(_) => {}
                Err(_) => break,
            }
        }
    })
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

#[cfg(test)]
mod tests {
    use super::{read_ws_url, PORT_FILE};

    fn write(name: &str, body: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("sc-rank-port-test-{}-{name}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(PORT_FILE);
        std::fs::write(&path, body).unwrap();
        path
    }

    /// 두 줄이 다 있으면 ws 주소가 된다.
    #[test]
    fn 포트와_대상으로_ws_주소를_만든다() {
        let path = write(
            "ok",
            "59202
/devtools/browser/96878e4b
",
        );
        assert_eq!(
            read_ws_url(&path).as_deref(),
            Some("ws://127.0.0.1:59202/devtools/browser/96878e4b")
        );
        std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
    }

    /// 쓰는 중이라 덜 찬 파일은 아직 주소가 아니다 — 다음 폴링에서 다시 본다.
    #[test]
    fn 덜_쓰인_파일은_주소가_아니다() {
        for (name, body) in [
            ("empty", ""),
            ("port-only", "59202"),
            (
                "port-newline",
                "59202
",
            ),
            (
                "blank-target",
                "59202

",
            ),
            (
                "bad-port",
                "nope
/devtools/browser/x
",
            ),
            (
                "bad-target",
                "59202
devtools/browser/x
",
            ),
        ] {
            let path = write(name, body);
            assert!(read_ws_url(&path).is_none(), "{name} 은 주소가 아니다");
            std::fs::remove_dir_all(path.parent().unwrap()).unwrap();
        }
    }

    /// 파일이 아직 없으면 주소가 아니다.
    #[test]
    fn 없는_파일은_주소가_아니다() {
        let missing = std::env::temp_dir().join(format!(
            "sc-rank-port-missing-{}/{}",
            std::process::id(),
            PORT_FILE
        ));
        assert!(read_ws_url(&missing).is_none());
    }
}
