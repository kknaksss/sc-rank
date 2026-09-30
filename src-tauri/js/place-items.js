els => els.map(li => {
        const text = li.innerText.replace(/\s+/g, ' ');
        const spans = [...li.querySelectorAll('span')].map(el => el.textContent.trim()).filter(Boolean);
        const hrefs = [...li.querySelectorAll('a')].map(a => a.getAttribute('href') || '');
        const id = hrefs.map(h => h.match(/\/(\d{6,})(?:[/?#]|$)/)?.[1]).find(Boolean) || null;
        return { title: li.querySelector('a.uD1F4 > span:first-child, .place_bluelink')?.textContent?.trim(), spans, id, ad: /광고/.test(text), reviews: text.match(/리뷰\s*([\d,]+)/)?.[1] || null, addr: text.match(/(서울 [가-힣]+구 [가-힣\d]+동)/)?.[1] || null };
      })
