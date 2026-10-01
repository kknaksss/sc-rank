// Runs inside the search iframe. Timers reject stalled rendering; they never
// advance a successful crawl. Every advance follows a DOM or browser render event.
export async function hydratePlacePage() {
  const container = document.querySelector('#_pcmap_list_scroll_container');
  if (!container) throw new Error('플레이스 목록 영역을 찾지 못했습니다.');
  const placeholderSelector = '.oncelazyload-placeholder, .lazyload-placeholder';
  const waitForDOM = (ready, act) => new Promise((resolve, reject) => {
    let observer;
    const timer = setTimeout(() => {
      observer.disconnect();
      reject(new Error('스크롤 후 업체 정보가 로딩되지 않았습니다.'));
    }, 8000);
    const check = () => {
      if (!ready()) return;
      clearTimeout(timer);
      observer.disconnect();
      resolve();
    };
    observer = new MutationObserver(check);
    observer.observe(container, { childList: true, subtree: true, attributes: true, characterData: true });
    act();
    check();
  });
  let loadedGroups = 0;
  for (let pass = 0; pass < 200; pass++) {
    const placeholder = container.querySelector(placeholderSelector);
    if (placeholder) {
      await waitForDOM(
        () => !container.contains(placeholder),
        () => {
          placeholder.scrollIntoView({ block: 'center', behavior: 'instant' });
          // A placeholder already in view still needs the list's scroll handler.
          container.dispatchEvent(new Event('scroll'));
        },
      );
      loadedGroups++;
      continue;
    }
    container.scrollTop = container.scrollHeight;
    // Let the scroll and intersection handlers render. No millisecond sleep.
    await new Promise(resolve => requestAnimationFrame(() => requestAnimationFrame(resolve)));
    if (container.querySelector(placeholderSelector)) continue;
    if (container.querySelector('[aria-busy="true"]')) {
      await waitForDOM(() => !container.querySelector('[aria-busy="true"]'), () => {});
      continue;
    }
    if (container.scrollTop + container.clientHeight < container.scrollHeight - 3) continue;
    return { loadedGroups, atBottom: true };
  }
  throw new Error('페이지 전체 로딩을 확인하지 못했습니다.');
}

export async function waitForPlacePageChange({ next, before }) {
  const signature = () => [...document.querySelectorAll('#_pcmap_list_scroll_container a.uD1F4 > span:first-child')].map(el => el.textContent).join('|');
  const ready = () => document.querySelector('a.mBN2s.qxokY')?.textContent.trim() === String(next) && signature() && signature() !== before;
  if (ready()) return;
  await new Promise((resolve, reject) => {
    const observer = new MutationObserver(() => {
      if (!ready()) return;
      clearTimeout(timer); observer.disconnect(); resolve();
    });
    const timer = setTimeout(() => { observer.disconnect(); reject(new Error('다음 페이지 목록을 읽지 못했습니다.')); }, 8000);
    observer.observe(document.body, { childList: true, subtree: true, attributes: true, characterData: true });
    if (ready()) { clearTimeout(timer); observer.disconnect(); resolve(); }
  });
}
