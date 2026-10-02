//! 테스트용 가짜 런처 — Windows 의 `msedge.exe`·`chrome.exe` 가 하는 일을 그대로 따라 한다.
//!
//! 받은 인자를 그대로 `SC_RANK_FAKE_LAUNCHER_TARGET` 의 실행 파일에 넘기고 **자신은 곧바로
//! exit 0 으로 끝낸다.** 넘긴 쪽의 stderr 는 버린다 — 그래서 우리를 띄운 프로세스가 보는
//! stderr 는 빈 채로 닫힌다. 실제 브라우저 업데이트 도중에 벌어지는 일과 같은 모양이다.
//!
//! `tests/browser.rs` 가 이걸 브라우저 자리에 놓고, `browser.rs` 가 stderr 가 아니라
//! `DevToolsActivePort` 에서 접속 주소를 읽어 붙는지 확인한다.

// 넘긴 프로세스를 기다리지 않는 것이 이 예제의 요점이다 — 런처는 먼저 끝난다.
#[allow(clippy::zombie_processes)]
fn main() {
    let target = std::env::var("SC_RANK_FAKE_LAUNCHER_TARGET")
        .expect("SC_RANK_FAKE_LAUNCHER_TARGET 이 필요하다");
    let args: Vec<String> = std::env::args().skip(1).collect();
    std::process::Command::new(target)
        .args(args)
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("브라우저를 넘기지 못했다");
    // 넘기고 바로 끝낸다.
}
