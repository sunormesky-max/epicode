import { describe, it, expect } from 'vitest';
import { vendorChunk } from '../../../vite.config';

// manualChunks 回归:只有 React 运行时进固定 vendor 块,重型路由依赖交给 Rollup 自动拆分。
const nm = (p: string) => `/repo/frontend/node_modules/${p}`;

describe('vendorChunk (vite manualChunks)', () => {
  it('React 运行时(含 jsx-runtime / react-dom/client / scheduler)进 vendor-react', () => {
    for (const p of [
      'react/index.js',
      'react/cjs/react-jsx-runtime.production.js',
      'react-dom/client.js',
      'react-dom/cjs/react-dom-client.production.js',
      'scheduler/cjs/scheduler.production.js',
      'react-router/dist/development/index.mjs',
    ]) expect(vendorChunk(nm(p)), p).toBe('vendor-react');
  });

  it('recharts / framer-motion / lucide 不固定分块(由 lazy 路由按需加载)', () => {
    for (const p of [
      'recharts/es6/index.js',
      'framer-motion/dist/es/index.mjs',
      'lucide-react/dist/esm/icons/brain.js',
      'react-is/index.js',
      'react-smooth/es6/index.js',
    ]) expect(vendorChunk(nm(p)), p).toBeUndefined();
  });

  it('业务源码不进 vendor 块', () => {
    expect(vendorChunk('/repo/frontend/src/pages/Home.tsx')).toBeUndefined();
    expect(vendorChunk('/repo/frontend/src/react/thing.ts')).toBeUndefined();
  });

  it('Windows 路径分隔符同样识别', () => {
    expect(vendorChunk('C:\\repo\\frontend\\node_modules\\react-dom\\client.js')).toBe('vendor-react');
  });
});
