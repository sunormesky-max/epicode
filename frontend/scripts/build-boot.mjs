// esbuild 打包 api/boot.ts — 跨平台包装
// 原 build 脚本把 --banner:js='...' 写进 package.json, POSIX 单引号在
// Windows cmd 下不被剥离, esbuild 把带引号内容当模块解析导致构建失败
// (审计 2026-09 验证发现). JS API 调用与平台 shell 无关.
import { build } from 'esbuild';

await build({
  entryPoints: ['api/boot.ts'],
  platform: 'node',
  bundle: true,
  format: 'esm',
  outdir: 'dist',
  banner: {
    js: 'import{createRequire}from"node:module";const require=createRequire(import.meta.url);',
  },
});

console.log('boot bundle written to dist/');
