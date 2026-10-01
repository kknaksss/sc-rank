import { chromium } from 'playwright';
import { hydratePlacePage, waitForPlacePageChange } from './place-dom.mjs';

export const PLACE_SCOPE = 'PC 네이버지도 플레이스 · 광고 포함 / 광고 제외 · 페이지 전체 수집 후 타겟 확인(최대 5페이지) · 헤드리스 기본 위치(위치 고정 없음)';
export const placeUrl = keyword => `https://map.naver.com/p/search/${encodeURIComponent(keyword)}?searchType=place`;
const compact = value => String(value || '').replace(/\s/g, '');
const generic = /^(?:성형외과|피부과|외과|의원|병원|클리닉|성형외과의원)$/;

export function extractPlaceName(item) {
  if (item.name && !generic.test(compact(item.name))) return item.name.trim();
  // A title element can contain multiple highlighted spans. Its full text wins.
  const candidates = [item.title, ...(item.spans || [])].filter(x => typeof x === 'string').map(x => x.replace(/\s+/g, ' ').trim());
  const name = candidates.find(x => x.length < 80 && /(?:의원|병원|클리닉|외과)(?:\s|$)/.test(x) && !generic.test(compact(x)) && !/리뷰|이미지|진료|현재 위치|전문의|\d+명/.test(x));
  if (name) return name;
  const spans = item.spans || [];
  for (let i = 1; i < spans.length; i++) {
    if (generic.test(compact(spans[i])) && /^[가-힣A-Za-z]{2,20}$/.test(spans[i - 1]) && !generic.test(spans[i - 1])) return spans[i - 1] + spans[i];
  }
  return '';
}

export function parsePlaceList(items, target) {
  if (!compact(target)) throw new Error('타겟 병원명을 입력해 주세요.');
  if (!Array.isArray(items) || !items.length) throw new Error('플레이스 목록을 읽지 못했습니다.');
  let organic = 0;
  const rows = items.map((item, i) => {
    const name = extractPlaceName(item);
    if (!name) throw new Error('상호를 읽지 못한 항목이 있어 순위를 계산할 수 없습니다.');
    if (!item.ad) organic++;
    return { ...item, name, ad: Boolean(item.ad), rank: i + 1, organicRank: item.ad ? null : organic, page: item.page || 1 };
  });
  const matches = rows.filter(row => compact(row.name).includes(compact(target)));
  return { status: matches.length ? 'found' : 'not_found', rank: matches[0]?.rank ?? null, organicRank: matches.find(row => !row.ad)?.organicRank ?? null, page: matches[0]?.page ?? null, total: rows.length, totalAds: rows.filter(row => row.ad).length, matches, rows };
}

let browserPromise;
export async function getBrowser() {
  if (!browserPromise) browserPromise = chromium.launch({ headless: true }).then(browser => {
    browser.on('disconnected', () => { browserPromise = undefined; });
    return browser;
  }).catch(error => { browserPromise = undefined; throw error; });
  return browserPromise;
}
export async function closePlaceBrowser() {
  const promise = browserPromise;
  browserPromise = undefined;
  if (promise) await (await promise).close();
}

export async function collectPlaceList(keyword, { maxPages = 5, target, onPage } = {}) {
  if (!Number.isInteger(maxPages) || maxPages < 1 || maxPages > 5) throw new Error('조회 페이지는 1~5 사이여야 합니다.');
  const started = Date.now();
  const log = (stage, details = {}) => console.info('[place]', JSON.stringify({ keyword, stage, elapsedMs: Date.now() - started, ...details }));
  log('start', { maxPages });
  let context, expired = false, timer;
  const work = async () => {
    const browser = await getBrowser();
    if (expired) throw new Error('조회 시간 초과');
    context = await browser.newContext({ locale: 'ko-KR', viewport: { width: 1400, height: 900 }, userAgent: 'Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/128.0.0.0 Safari/537.36' });
    if (expired) { await context.close(); throw new Error('조회 시간 초과'); }
    log('browser-ready');
    const page = await context.newPage();
    page.setDefaultTimeout(8000);
    const frame = () => page.frames().find(f => f.name() === 'searchIframe' && f.url().includes('pcmap'));
    await page.goto(placeUrl(keyword), { waitUntil: 'domcontentloaded', timeout: 30000 });
    log('navigation-ready');
    await page.frameLocator('iframe#searchIframe, iframe[name="searchIframe"]').locator('#_pcmap_list_scroll_container a.uD1F4 > span:first-child').first().waitFor({ state: 'attached', timeout: 20000 });
    if (!frame()) throw new Error('플레이스 검색 프레임을 읽지 못했습니다.');
    log('list-ready');
    const all = [];
    for (let pageNo = 1; pageNo <= maxPages; pageNo++) {
      log('page-start', { page: pageNo });
      const hydration = await frame().evaluate(hydratePlacePage);
      const raw = await frame().locator('#_pcmap_list_scroll_container > ul > li').evaluateAll(els => els.map(li => {
        const text = li.innerText.replace(/\s+/g, ' ');
        const spans = [...li.querySelectorAll('span')].map(el => el.textContent.trim()).filter(Boolean);
        const hrefs = [...li.querySelectorAll('a')].map(a => a.getAttribute('href') || '');
        const id = hrefs.map(h => h.match(/\/(\d{6,})(?:[/?#]|$)/)?.[1]).find(Boolean) || null;
        return { title: li.querySelector('a.uD1F4 > span:first-child, .place_bluelink')?.textContent?.trim(), spans, id, ad: /광고/.test(text), reviews: text.match(/리뷰\s*([\d,]+)/)?.[1] || null, addr: text.match(/(서울 [가-힣]+구 [가-힣\d]+동)/)?.[1] || null };
      }));
      log('scroll-complete', { page: pageNo, ...hydration, captured: raw.length });
      if (onPage) await onPage({ page: pageNo, raw, html: await frame().content() });
      let added = 0;
      for (const item of raw) {
        const name = extractPlaceName(item);
        if (!name) { if (item.id || item.title) throw new Error('일부 업체의 상호를 읽지 못했습니다.'); else continue; }
        added++;
        all.push({ name, id: item.id, ad: item.ad, reviews: item.reviews, addr: item.addr, page: pageNo });
      }
      log('page-complete', { page: pageNo, added, total: all.length });
      if (!added) { if (!all.length) throw new Error('플레이스 목록을 읽지 못했습니다.'); break; }
      if (target && all.some(item => compact(item.name).includes(compact(target)))) { log('target-found', { page: pageNo }); break; }
      if (pageNo === maxPages) break;
      const before = await frame().locator('#_pcmap_list_scroll_container a.uD1F4 > span:first-child').evaluateAll(els => els.map(el => el.textContent).join('|'));
      const clicked = await frame().evaluate(next => {
        const el = [...document.querySelectorAll('a, button')].filter(e => e.textContent.trim() === String(next)).at(-1);
        if (!el) return false;
        el.click(); return true;
      }, pageNo + 1);
      if (!clicked) break;
      await frame().evaluate(waitForPlacePageChange, { next: pageNo + 1, before });
    }
    log('complete', { total: all.length });
    return all;
  };
  try {
    return await Promise.race([work(), new Promise((_, reject) => { timer = setTimeout(() => { expired = true; reject(new Error('조회 제한 시간 60초를 초과했습니다. 다시 조회해 주세요.')); }, 60000); })]);
  } catch (error) { log('error', { message: error.message }); throw error; } finally { clearTimeout(timer); if (context) await context.close().catch(() => {}); }
}

export async function checkPlace(keyword, target) {
  const base = { keyword, target, searchUrl: placeUrl(keyword), scope: PLACE_SCOPE };
  try {
    if (typeof keyword !== 'string' || !keyword.trim() || keyword.length > 100 || typeof target !== 'string' || !target.trim() || target.length > 50) throw new Error('키워드는 1~100자, 병원명은 1~50자로 입력해 주세요.');
    const result = parsePlaceList(await collectPlaceList(keyword, { target }), target);
    return { ...base, ...result, checkedAt: new Date().toISOString(), message: result.status === 'not_found' ? '수집한 최대 5페이지 목록에서 찾지 못했습니다.' : '' };
  } catch (error) {
    return { ...base, checkedAt: new Date().toISOString(), status: 'error', rank: null, organicRank: null, page: null, total: null, totalAds: null, matches: [], rows: [], message: /[가-힣]/.test(error.message) ? error.message : '플레이스 조회에 실패했습니다. 브라우저 설치 또는 네이버 접근 제한을 확인해 주세요.' };
  }
}
