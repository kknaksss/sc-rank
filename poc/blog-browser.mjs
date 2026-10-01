import { load } from 'cheerio';
import { getBrowser } from './place.mjs';

// Each yield is a fully rendered result batch. Closing the iterator closes the
// context, including when the caller finds its target in an early batch.
export async function* blogBatches(url, { maxScrolls = 3 } = {}) {
  const browser = await getBrowser();
  const context = await browser.newContext({ locale: 'ko-KR', viewport: { width: 1400, height: 900 } });
  const deadline = setTimeout(() => context.close().catch(() => {}), 150000);
  try {
    const page = await context.newPage();
    page.setDefaultTimeout(12000);
    await page.goto(url, { waitUntil: 'domcontentloaded', timeout: 25000 });
    await page.locator('[data-template-id="ugcItem"]').first().waitFor({ state: 'attached' });
    yield { html: await page.content(), scrolls: 0 };
    for (let scrolls = 1; scrolls <= maxScrolls; scrolls++) {
      const before = await page.locator('[data-template-id="ugcItem"]').count();
      const responsePromise = page.waitForResponse(r => {
        const u = new URL(r.url());
        return u.hostname === 's.search.naver.com' && u.pathname.startsWith('/p/review/');
      });
      // Register the response listener before the scroll triggers the request.
      const [response] = await Promise.all([
        responsePromise,
        page.evaluate(() => window.scrollTo(0, document.documentElement.scrollHeight)),
      ]);
      if (!response.ok()) throw new Error(`블로그 추가 결과 요청에 실패했습니다(HTTP ${response.status()}).`);
      const payload = await response.json();
      if (!Array.isArray(payload.collection)) throw new Error('블로그 추가 결과 형식을 읽지 못했습니다.');
      const added = payload.collection.reduce((n, c) => n + load(c.html || '')('[data-template-id="ugcItem"]').length, 0);
      if (!added) {
        if (!payload.url) return;
        throw new Error('추가 결과를 읽지 못해 순위를 확정할 수 없습니다.');
      }
      await page.evaluate(({ expected }) => new Promise((resolve, reject) => {
        const ready = () => [...document.querySelectorAll('[data-template-id="ugcItem"]')].filter(card => card.querySelector('a [class*="text-type-headline"]')).length >= expected;
        if (ready()) { resolve(); return; }
        const observer = new MutationObserver(() => { if (ready()) { clearTimeout(timer); observer.disconnect(); resolve(); } });
        const timer = setTimeout(() => { observer.disconnect(); reject(new Error('블로그 추가 목록 렌더링이 완료되지 않았습니다.')); }, 12000);
        observer.observe(document.body, { childList: true, subtree: true });
        if (ready()) { clearTimeout(timer); observer.disconnect(); resolve(); }
      }), { expected: before + added });
      yield { html: await page.content(), scrolls };
      if (!payload.url) return;
    }
  } finally { clearTimeout(deadline); await context.close().catch(() => {}); }
}
