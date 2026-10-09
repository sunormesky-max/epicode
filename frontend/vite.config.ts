import path from "path"
const __dirname = import.meta.dirname
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

// React 运行时(含 react/jsx-runtime、react-dom/client、scheduler)= 每个页面都需要的最小集合
const REACT_RUNTIME = /[\\/]node_modules[\\/](react|react-dom|react-router|scheduler|cookie|set-cookie-parser)[\\/]/;
export function vendorChunk(id: string): string | undefined {
  return REACT_RUNTIME.test(id) ? 'vendor-react' : undefined;
}

// https://vite.dev/config/
export default defineConfig({
  base: './',
  plugins: [react()],
  server: {
    port: 3000,
    // dev 代理：前端 /api → Rust 后端（kimi #9）
    proxy: {
      '/api': {
        target: 'http://127.0.0.1:9111',
        changeOrigin: true,
        rewrite: (p) => p.replace(/^\/api/, ''),
      },
    },
  },
  resolve: {
    alias: {
      "@": path.resolve(__dirname, "./src"),
    },
  },
  envDir: path.resolve(__dirname),
  build: {
    outDir: path.resolve(__dirname, "dist/public"),
    emptyOutDir: true,
    rollupOptions: {
      output: {
        // 只把"首屏必经"的 React 运行时固定成 vendor 块;recharts / framer-motion 交给 Rollup
        // 按 lazy 路由自动拆分共享块(Rollup 不会重复打包同一模块)。
        // 旧的对象写法会把 react/jsx-runtime 和 react-dom 的 CJS 依赖分配进 vendor-motion /
        // vendor-recharts,导致入口静态 import 这两个块 → 每个页面(含登录页)都预加载
        // ~157 KB gzip 的图表/动画库。见 src/lib/__tests__/entry-chunks.test.ts。
        manualChunks: vendorChunk,
      }
    }
  },
});
