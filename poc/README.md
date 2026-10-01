# poc/ — PoC 원본 사본

**수정 금지 — 동일성 검사용.**

`src-tauri/tests/collect_offline.rs` 의 `주입_js_는_poc_원문과_같다` 가 이 파일들을 읽어
`src-tauri/js/*.js`(페이지 안에 주입하는 JS)가 PoC 원문과 같은지 확인한다. 앱 빌드에는 쓰이지 않는다.

| 파일 | 출처 |
|---|---|
| `place-dom.mjs` | kknaks_profile 레포 `reference/2026-09-09-sc-prototype/server/place-dom.mjs` |
| `place.mjs` | kknaks_profile 레포 `reference/2026-09-09-sc-prototype/server/place.mjs` |
| `blog-browser.mjs` | kknaks_profile 레포 `reference/2026-09-09-sc-prototype/server/blog-browser.mjs` |

- 커밋 `d6954f0` 의 파일을 바이트 그대로 복사했다.
- PoC 원본이 바뀌면 이 사본을 고치지 말고 원본에서 다시 복사한다.
