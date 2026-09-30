next => {
        const el = [...document.querySelectorAll('a, button')].filter(e => e.textContent.trim() === String(next)).at(-1);
        if (!el) return false;
        el.click(); return true;
      }
