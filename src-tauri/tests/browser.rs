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
