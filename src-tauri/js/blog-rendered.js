({ expected }) => new Promise((resolve, reject) => {
        const ready = () => [...document.querySelectorAll('[data-template-id="ugcItem"]')].filter(card => card.querySelector('a [class*="text-type-headline"]')).length >= expected;
        if (ready()) { resolve(); return; }
        const observer = new MutationObserver(() => { if (ready()) { clearTimeout(timer); observer.disconnect(); resolve(); } });
        const timer = setTimeout(() => { observer.disconnect(); reject(new Error('블로그 추가 목록 렌더링이 완료되지 않았습니다.')); }, 12000);
        observer.observe(document.body, { childList: true, subtree: true });
        if (ready()) { clearTimeout(timer); observer.disconnect(); resolve(); }
      })
