# CLAUDE.md — Epicode 统一项目

本文件为 AI 助手在 `C:\epicode` 仓库中工作时提供指引。

## 项目概述

Epicode 是一个空间 AI 记忆系统，由三大子系统组成：

| 子系统 | 目录 | 语言 | 用途 |
|--------|------|------|------|
| 前端 | `frontend/` | TypeScript / React 19 | epicode.cn 网站（仪表盘 + 公共页面） |
| 后端 | `backend/` | Rust / Axum | TetraMem 空间记忆引擎 + Cloud API |
| 防御系统 | `guard/` | Rust | epicode-guard 服务器自动防御守护进程 |

## 子系统详情

### 前端 (`frontend/`)

- **技术栈**：React 19 + TypeScript + Vite 7 + Tailwind CSS + HashRouter + Framer Motion + Recharts
- **源码**：`src/`（73 个文件）、`api/`（Hono SSR）、`db/`（Drizzle ORM）、`contracts/`
- **构建**：`npm run build`（Vite build → esbuild api/boot.ts）
- **开发**：`npm run dev`（Vite dev server :5173）
- **部署目标**：`/var/www/tetramem-xl.com/`，Nginx 代理
- **UI 组件库**：26 个 `@radix-ui/*` 包 + `src/components/ui/`（51 个封装组件）
- **页面**：5 个公共页（Home/Docs/Guide/Community/Benchmarks）+ 5 个仪表盘页 + Login/Register
- **API 层**：`src/lib/api.ts`，所有请求通过 `X-API-Key` 认证
- **注意**：`esbuild api/boot.ts` 构建报错可忽略，Vite build 成功即可

### 后端 (`backend/`)

- **技术栈**：Rust + Axum + Tokio + SQLite + ONNX Runtime
- **源码**：`src/`（50+ 个模块）、`src/bin/cloud.rs`（Cloud 主入口）
- **构建**：`cargo.bat build`（Windows）/ `cargo build --release`（Linux）
- **Windows 注意**：必须用 `cargo.bat`，它会设置 `CXXFLAGS=/MD`、`CFLAGS=/MD`
- **运行**：`cargo.bat run` → 监听 `127.0.0.1:9110`（本地）/ `127.0.0.1:9111`（Cloud）
- **测试**：`cargo.bat test --lib`
- **领域层**：`src/domain/` — 顶点、四面体、空间索引、圆柱体、通道、脉冲
- **引擎层**：`src/engine/` — 38 个模块（网关、认知、调度、知识图谱、HNSW 等）
- **API 层**：`src/api/routes.rs` + `src/bin/cloud.rs` — 所有 HTTP 路由
- **关键常量**：`EDGE_LENGTH=1.0`、`VERTEX_MERGE_EPSILON=0.05`、端口 ID 从 `1_000_000` 起始

### 防御系统 (`guard/`)

- **技术栈**：Rust + chrono + serde_json
- **源码**：`src/main.rs`（974 行单文件）、`install.sh`、`epicode-guard.service`
- **构建**：`cargo build --release`（LTO + strip）
- **部署位置**：`/usr/local/bin/epicode-guard`
- **功能**：SSH 暴力破解检测、Web 攻击检测（62 种模式）、TCP 蜜罐（8 端口）、连接洪水检测、文件完整性监控、nftables 封禁
- **状态文件**：`/var/lib/epicode-guard/state.json`
- **日志**：`/var/log/epicode-guard/guard.log`

## 环境变量

| 变量 | 用途 | 用在 |
|------|------|------|
| `TETRAMEM_API_KEY` | API 认证密钥 | 后端 |
| `TETRAMEM_MASTER_KEY` | 主账户密钥 | 后端（Cloud） |
| `TETRAMEM_ADMIN_KEY` | 管理员密钥 | 后端（Cloud） |
| `DEEPSEEK_API_KEY` | LLM 认知功能 | 后端 |

## 服务器信息

- **IP**：`111.231.24.199`（OpenCloudOS 9.4），仅 SSH 密钥认证
- **域名**：`epicode.cn`，Nginx + Let's Encrypt
- **后端运行**：`/opt/tetramem/epicode-cloud`，用户 `tetramem`，监听 `127.0.0.1:9111`
- **前端部署**：`/var/www/tetramem-xl.com/`
- **Nginx 配置**：`/etc/nginx/conf.d/epocode.cn.conf`
- **防御系统**：`/usr/local/bin/epicode-guard`，systemd 管理

## AI 身份

- **名称**：大卫 (David)
- **作者**：刘启航
- **使命**：空间 AI 记忆系统

## 开发规范

- 界面文字使用中文（技术术语如 API Key、ID 等保留英文）
- 代码注释不添加（除非明确要求）
- 后端已冻结（v1.0.0 稳定），无需后端更改
- `position:fixed` 背景（z:0）必须确保内容容器有 `position:relative + z-index>0`
- `useMemo` 必须放在 early return 之前（React hooks 规则）
- 批量操作不能替换 import 名、变量名、API 字段名、CSS 类名
- scp 上传后必须 `chown nginx:nginx`
