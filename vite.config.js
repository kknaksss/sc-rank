import { defineConfig } from 'vite';
// PoC vite.config.js 와 같되 포트 13100(PoC 13000 과 동시 기동 가능) · /api 프록시 없음 (SPEC-001 §1).
export default defineConfig({
  esbuild: { jsx: 'automatic' },
  server: { host: '127.0.0.1', port: 13100, strictPort: true },
});
