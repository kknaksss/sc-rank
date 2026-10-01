# SC Rank (데스크톱)

네이버 **블로그 상위 노출**과 **플레이스 순위**를 조회해 엑셀로 저장하는 서버 없는 데스크톱 프로그램이다.
운영 환경은 Windows 10/11 x64, 개발 환경은 macOS 다. 웹 PoC(kknaks_profile 레포 `reference/2026-09-09-sc-prototype/server/`·`src/`)를 Tauri 2 + Rust 로 옮겼다.
계약은 SPEC-001(kknaks_profile 레포 `para/projects/summer-star/sc-rank/20-spec/spec-001-desktop-app.md`)에 있다.
동일성 테스트가 읽는 PoC 원본 사본은 `poc/` 에 있다(`poc/README.md`).

- 화면: PoC React 화면 그대로(`src/`). PoC 와 다른 곳은 `src/bridge.js` 연결과 `main.jsx` 의 호출·저장 자리뿐이다.
- 조회: PC 에 설치된 **Microsoft Edge → Google Chrome** 을 헤드리스로 띄워 CDP 로 조종한다. Chromium 은 동봉하지 않는다.
- 결과는 앱 메모리에만 있다. 「엑셀 저장하기」→ 저장 대화상자로 `.xlsx` 를 쓴다.

---

## 1. macOS 개발

### 사전 요건 (검증한 버전)

| 도구 | 버전 | 비고 |
|---|---|---|
| Rust | stable 1.97 (최소 1.85) | `rustup` · `~/.cargo/bin` 이 PATH 에 있어야 한다 |
| Node.js | 20 LTS (20.20.0) | npm 포함 |
| Xcode Command Line Tools | 최신 | `xcode-select --install` |
| Microsoft Edge 또는 Google Chrome | 최신 | `/Applications` 에 설치. 조회에 필요 |

### 명령

```bash
git clone https://github.com/kknaksss/sc-rank.git
cd sc-rank
npm install                 # 처음 한 번
npm run tauri dev           # 앱 실행 — vite 는 127.0.0.1:13100 (PoC 13000 과 동시 실행 가능)
npm run build               # 화면 빌드만

cd src-tauri
cargo test                                   # 단위 테스트 (네트워크·브라우저 없음)
cargo clippy --all-targets -- -D warnings    # 경고 0 이어야 한다
```

### 실수집 확인 (smoke)

앱과 같은 수집 함수를 부르고 단계 로그와 요약 JSON 을 표준 출력에 낸다. `status == "error"` 면 exit 1.
**네이버에 실제로 요청한다. 한 번씩만, 사이에 2초 이상 두고 돌린다.**

```bash
cd src-tauri
cargo run --example smoke -- place 강남역성형외과 무이성형외과
cargo run --example smoke -- blog 구월동레이저제모 keyword 썸블리의원
cargo run --example smoke -- blog 구월동피부과 image tests/fixtures/1.png
```

`not_found` 는 실패가 아니다(순위는 시점마다 달라진다). 끝나면 브라우저와 임시 프로필(`$TMPDIR/sc-rank-cdp-*`)이 정리된다.

---

## 2. Windows 빌드 (설치 파일 만들기)

### 기본: GitHub Actions (`.github/workflows/build-windows.yml`)

설치 파일은 **GitHub Actions 의 Windows 머신이 만든다**(DEC-001 D-11, 2026-10-01 개정). 로컬 Windows 도구가 필요 없다.

| 언제 | 무엇이 생기나 | 받는 곳 |
|---|---|---|
| PR 을 열거나 push | Windows 에서 `cargo test` + 빌드, 설치 파일을 산출물로 | PR 의 Checks → `windows-build` → Artifacts `sc-rank-windows-setup` |
| 수동 실행 (`main` 에 워크플로가 들어간 뒤) | 위와 같음 | Actions → `windows-build` → Run workflow → 실행 결과의 Artifacts |
| `v*` 태그 push | 위 + **GitHub Release 에 설치 파일 첨부** | Releases |

릴리스 순서: `package.json` · `src-tauri/Cargo.toml` · `src-tauri/tauri.conf.json` 의 `version` 을 같은 값으로 올린다 → 커밋 → `git tag v0.1.1 && git push origin v0.1.1`.

- Artifacts 는 zip 으로 내려받아진다. 안의 `SC Rank_<버전>_x64-setup.exe` 를 쓴다.

### 대안: Windows PC 에서 직접

Actions 를 못 쓸 때만. macOS 에서는 Windows 설치 파일을 만들 수 없다.
TLS 암호 라이브러리(`ring`, rustls 의 제공자)가 MSVC 로 C 코드를 컴파일해야 하기 때문이다.
어셈블리는 `ring` 이 미리 빌드된 객체를 싣고 있어 **NASM·CMake 는 필요 없다.**

### 사전 요건

| 도구 | 버전 | 설치 |
|---|---|---|
| Visual Studio 2022 Build Tools | 17.x | [Build Tools](https://visualstudio.microsoft.com/visual-cpp-build-tools/) 설치 시 **「C++를 사용한 데스크톱 개발」** 워크로드 선택. MSVC v143 과 Windows 10/11 SDK 가 함께 들어간다 |
| Rust | stable 1.97 (최소 1.85), **MSVC 툴체인** | [rustup-init.exe](https://rustup.rs) → 기본값 `x86_64-pc-windows-msvc`. 확인: `rustup show` 의 default host 가 `x86_64-pc-windows-msvc` |
| Node.js | 20 LTS | [nodejs.org](https://nodejs.org) Windows x64 설치 파일 |
| Git | 최신 | [git-scm.com](https://git-scm.com/download/win) |
| WebView2 Runtime | Evergreen | Windows 11 은 기본 설치. Windows 10 에 없으면 [WebView2](https://developer.microsoft.com/microsoft-edge/webview2/) 에서 Evergreen Bootstrapper. 설치 파일도 없을 때 부트스트래퍼를 내려받는다 |
| Microsoft Edge | 기본 탑재 | 조회에 쓴다. Edge 가 없으면 Chrome |

- ⚠ 이 절차는 macOS 에서 작성했고 Windows 실기 빌드로 아직 확인하지 않았다. 처음 빌드가 성공하면 이 줄을 지운다.
- 명령은 **「x64 Native Tools Command Prompt for VS 2022」** 또는 일반 PowerShell 에서 실행한다. rustup 이 MSVC 를 찾는다.

### 명령 (PowerShell 또는 명령 프롬프트)

```powershell
git clone https://github.com/kknaksss/sc-rank.git
cd sc-rank
npm install
npm run tauri build
```

- 산출물: `src-tauri\target\release\bundle\nsis\SC Rank_0.1.0_x64-setup.exe`
- 번들은 NSIS 하나만 만든다(`tauri.conf.json` 의 `bundle.targets = ["nsis"]`).
- 빌드 확인용: `cd src-tauri` → `cargo test` · `cargo run --example smoke -- place 강남역성형외과 무이성형외과`

### 설치와 첫 실행

1. `SC Rank_0.1.0_x64-setup.exe` 를 사용자에게 파일로 전달한다(자동 업데이트 없음).
2. 코드 서명을 하지 않으므로 첫 실행 때 **Windows SmartScreen** 이 「Windows의 PC 보호」를 띄운다.
   **「추가 정보」→「실행」** 을 누르면 설치가 진행된다. 서명하지 않기로 한 결정은 DEC-001 D-10 에 있다.
3. 시작 메뉴의 **SC Rank** 로 실행한다.

### 조회가 「플레이스 조회에 실패했습니다…」/「블로그 조회 또는 이미지 처리가 실패했습니다…」로만 끝날 때

- **Edge 원격 디버깅 정책.** 회사 PC 는 그룹 정책 `RemoteDebuggingAllowed`(Edge·Chrome 공통 이름)가 `0`(사용 안 함)으로 막혀 있을 수 있다. 그러면 헤드리스 브라우저가 CDP 연결을 거부해 조회가 실패한다.
  - 확인: Edge 주소창에 `edge://policy` → `RemoteDebuggingAllowed` 가 `false` 인지 본다. 레지스트리로는 `HKLM\SOFTWARE\Policies\Microsoft\Edge\RemoteDebuggingAllowed`.
  - 앱은 Edge 가 **설치돼 있으면** Edge 를 쓰고 Chrome 으로 넘어가지 않는다. 정책이 막혀 있으면 IT 관리자에게 허용을 요청해야 한다.
- 「Edge 또는 Chrome 을 찾지 못했습니다…」는 두 브라우저 모두 표준 경로(`Program Files`·`Program Files (x86)`·`%LOCALAPPDATA%`)에 없다는 뜻이다.
- 원래 오류는 아래 로그 파일에 남는다.

---

## 3. 로그 파일

단계 로그(`[place]`·`[blog]`·`[browser]`·`[export]`, JSON 한 줄)가 파일 하나에 쌓인다.

| OS | 위치 |
|---|---|
| Windows | `%LOCALAPPDATA%\com.summerstar.scrank\logs\sc-rank.log` |
| macOS | `~/Library/Logs/com.summerstar.scrank/sc-rank.log` |

smoke 예제는 같은 로그를 표준 출력에 낸다.

## 4. 브라우저 프로세스와 임시 프로필

- 조회할 때만 헤드리스 브라우저를 하나 띄운다. 블로그·플레이스가 이 하나를 같이 쓰고, 끊기면 다음 조회에서 다시 띄운다.
- 사용자 브라우저 프로필은 쓰지 않는다. 임시 폴더에 `sc-rank-cdp-*` 전용 프로필을 만든다.
- 앱 창을 닫으면 브라우저를 끝내고 임시 프로필을 지운다. 강제 종료로 남은 `sc-rank-cdp-*` 는 다음 실행 때 지운다 — 이름의 pid 가 아직 살아 있는 것(다른 SC Rank 창·smoke 가 쓰는 중)은 남긴다.

## 5. 폴더

```
sc-rank/
  index.html · src/        PoC 화면 사본 + src/bridge.js (invoke 연결 · 링크를 OS 브라우저로)
  src-tauri/
    src/                   commands · browser(CDP) · place · blog · blog_browser · workbook · gate · errors · js
    js/                    페이지 안에서 도는 PoC JS 원문(주입용)
    examples/smoke.rs      실수집 확인
    tests/                 단위 테스트 · fixtures/1.png
  poc/                     PoC 원본 사본 — 주입 JS 동일성 검사용, 수정 금지
```
