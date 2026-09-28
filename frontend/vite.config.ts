import path from "path"
const __dirname = import.meta.dirname
import react from "@vitejs/plugin-react"
import { defineConfig } from "vite"

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
        manualChunks: {
          'vendor-react': ['react', 'react-dom', 'react-router'],
          'vendor-motion': ['framer-motion'],
          // P3修复:recharts单独成chunk,避免在DashboardOverview/Benchmarks两个lazy chunk中重复打包(~200KB)
          'vendor-recharts': ['recharts'],
        }
      }
    }
  },
});
