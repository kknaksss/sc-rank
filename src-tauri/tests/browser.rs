//! 브라우저 탐색 (A-7).

use std::path::PathBuf;

use sc_rank_lib::browser::{find_browser, NOT_FOUND_MESSAGE};
use sc_rank_lib::errors::{Area, ScrapeError};

#[test]
fn 빈_후보는_설치_안내_문구로_실패() {
    let err = find_browser::<PathBuf>(&[]).unwrap_err();
    assert!(matches!(err, ScrapeError::Domain(_)));
    assert_eq!(err.user_message(Area::Place), NOT_FOUND_MESSAGE);
    assert_eq!(err.user_message(Area::Blog), NOT_FOUND_MESSAGE);
    assert_eq!(
        NOT_FOUND_MESSAGE,
        "Edge 또는 Chrome 을 찾지 못했습니다. 둘 중 하나를 설치한 뒤 다시 조회해 주세요."
    );
}

#[test]
fn 없는_경로와_디렉터리는_건너뛰고_첫_실행_파일을_고른다() {
    let dir = std::env::temp_dir().join(format!("sc-rank-browser-test-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let edge = dir.join("edge");
    let chrome = dir.join("chrome");
    std::fs::write(&chrome, b"").unwrap();
    std::fs::write(&edge, b"").unwrap();
    let missing = dir.join("missing");
    assert_eq!(
        find_browser(&[missing.clone(), dir.clone(), edge.clone(), chrome.clone()]).unwrap(),
        edge
    );
    assert_eq!(
        find_browser(&[missing.clone(), chrome.clone()]).unwrap(),
        chrome
    );
    assert_eq!(
        find_browser(&[missing])
            .unwrap_err()
            .user_message(Area::Place),
        NOT_FOUND_MESSAGE
    );
    std::fs::remove_dir_all(&dir).unwrap();
}

/// 잔여 프로필 정리는 이름의 pid 가 살아 있지 않은 것만 지운다 (W-4).
#[test]
fn 잔여_프로필은_죽은_pid_만_지운다() {
    use sc_rank_lib::browser::{remove_stale_profiles_in, PROFILE_PREFIX};
    let root = std::env::temp_dir().join(format!("sc-rank-stale-test-{}", std::process::id()));
    std::fs::create_dir_all(&root).unwrap();
    // 끝난 자식 프로세스의 pid 는 죽은 pid 다.
    #[cfg(unix)]
    let mut child = std::process::Command::new("true").spawn().unwrap();
    #[cfg(windows)]
    let mut child = std::process::Command::new("cmd")
        .args(["/C", "exit"])
        .spawn()
        .unwrap();
    let dead_pid = child.id();
    child.wait().unwrap();
    let live = root.join(format!("{PROFILE_PREFIX}{}-1", std::process::id()));
    let dead = root.join(format!("{PROFILE_PREFIX}{dead_pid}-2"));
    let other = root.join("not-ours-3");
    for d in [&live, &dead, &other] {
        std::fs::create_dir_all(d.join("Default")).unwrap();
    }
    remove_stale_profiles_in(&root);
    assert!(live.exists(), "살아 있는 pid 의 프로필은 남는다");
    assert!(!dead.exists(), "죽은 pid 의 프로필은 지운다");
    assert!(other.exists(), "접두가 다른 폴더는 건드리지 않는다");
    std::fs::remove_dir_all(&root).unwrap();
}

// ---- CDP 기동 (실제 브라우저가 있을 때만 돈다) ----

use sc_rank_lib::browser::{default_candidates, BrowserManager, PROFILE_PREFIX};

/// 설치된 브라우저. 없으면 기동 테스트를 건너뛴다.
fn installed_browser() -> Option<PathBuf> {
    default_candidates().into_iter().find(|p| p.is_file())
}

/// `cargo test` 가 같이 빌드하는 `examples/fake_launcher`.
/// 테스트 실행 파일은 `target/<profile>/deps/` 에 있고 예제는 `target/<profile>/examples/` 에 있다.
fn fake_launcher() -> PathBuf {
    let mut dir = std::env::current_exe().expect("테스트 실행 파일 경로");
    dir.pop();
    dir.pop();
    dir.push("examples");
    dir.push(if cfg!(windows) {
        "fake_launcher.exe"
    } else {
        "fake_launcher"
    });
    assert!(
        dir.is_file(),
        "{} 가 없다 — `cargo build --examples` 로 먼저 만든다",
        dir.display()
    );
    dir
}

/// 이 프로세스가 만든 임시 프로필 중 남아 있는 것.
fn 남은_프로필() -> Vec<String> {
    let mine = format!("{PROFILE_PREFIX}{}-", std::process::id());
    std::fs::read_dir(std::env::temp_dir())
        .expect("임시 폴더")
        .flatten()
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(&mine))
        .collect()
}

/// 기동 두 갈래가 모두 열리고, 끝내면 브라우저도 임시 프로필도 남지 않는다.
///
/// 임시 프로필은 이름에 이 프로세스의 pid 를 쓴다 — 같은 프로세스의 다른 테스트와 섞이지
/// 않도록 두 갈래를 한 테스트에서 차례로 본다.
#[tokio::test]
async fn 브라우저를_띄우고_컨텍스트를_열고_정리한다() {
    let Some(browser) = installed_browser() else {
        return;
    };

    // 1) 평소 경로 — 브라우저 실행 파일을 직접 띄운다.
    let manager = BrowserManager::default();
    let context = manager.open_context().await.expect("컨텍스트가 열린다");
    manager.close_context(context).await;
    manager.shutdown().await;
    assert!(
        남은_프로필().is_empty(),
        "직접 띄운 뒤 임시 프로필이 남았다: {:?}",
        남은_프로필()
    );

    // 2) 런처가 실제 브라우저를 다른 프로세스로 넘기고 자신은 exit 0 으로 먼저 끝나는 경로.
    //    Windows 의 `msedge.exe`·`chrome.exe` 가 브라우저 업데이트 도중에 이렇게 움직인다.
    //    접속 주소를 **띄운 자식의 stderr** 에서 읽으면 여기서 기동이 실패하고, 넘겨받은
    //    브라우저는 살아서 고아로 남는다. 그래서 주소는 `DevToolsActivePort` 에서 읽는다.
    std::env::set_var("SC_RANK_FAKE_LAUNCHER_TARGET", &browser);
    let manager = BrowserManager::new(vec![fake_launcher()]);
    let context = manager
        .open_context()
        .await
        .expect("런처가 먼저 끝나도 컨텍스트가 열린다");
    manager.close_context(context).await;
    manager.shutdown().await;
    assert!(
        남은_프로필().is_empty(),
        "런처가 넘긴 뒤 임시 프로필이 남았다: {:?}",
        남은_프로필()
    );
}
