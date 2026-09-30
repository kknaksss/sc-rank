// 화면 ↔ Tauri 명령 연결 (SPEC-001 §5). PoC 의 fetch·다운로드 자리를 대신하고, 결과 링크를 OS 브라우저로 연다.
import { invoke } from '@tauri-apps/api/core';
import { openUrl } from '@tauri-apps/plugin-opener';

// invoke 는 Err 를 문자열로 reject 한다 — PoC catch 가 읽는 Error 로 감싼다(`!res.ok` 분기와 같은 결과).
const asError = error => error instanceof Error ? error : Error((typeof error === 'string' && error) || '조회에 실패했습니다.');

// 조회 — 180초 상한은 화면에서 유지한다. 넘기면 PoC 와 같은 TimeoutError(「조회 시간이 초과되었습니다.」).
export async function checkRank(placeMode, body) {
  let timer;
  const timeout = new Promise((_, reject) => {
    timer = setTimeout(() => { const error = Error('The operation was aborted due to timeout'); error.name = 'TimeoutError'; reject(error); }, 180000);
  });
  try {
    return await Promise.race([invoke(placeMode ? 'check_place' : 'check_blog', body).catch(error => { throw asError(error); }), timeout]);
  } finally { clearTimeout(timer); }
}

// 저장 — 저장 대화상자는 명령이 연다. 취소는 조용히, 모든 실패는 PoC 저장 실패 문구.
export async function saveWorkbook(rows, mode) {
  try { return await invoke('export_xlsx', { rows, mode }); }
  catch { throw Error('엑셀을 저장하지 못했습니다. 다시 시도해 주세요.'); }
}

// 결과 표의 <a target="_blank"> 는 웹뷰에서 열리지 않는다 — 가로채 OS 기본 브라우저로 연다. 앱 창은 이동하지 않는다.
document.addEventListener('click', event => {
  const link = event.target instanceof Element ? event.target.closest('a[target="_blank"]') : null;
  if (!link || !link.href) return;
  event.preventDefault();
  openUrl(link.href).catch(() => {});
}, true);
