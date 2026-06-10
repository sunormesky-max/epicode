# Epicode AI Memory Operating System — 源代码说明文档

## 一、项目概述

**Epicode** 是一款 AI 长期记忆管理平台的 Web 前端系统。它为 AI 代理提供持久化的向量记忆存储、语义搜索、知识图谱可视化等功能。设计采用深色科技感风格，以紫红色神经网络动态背景为核心视觉元素，覆盖全站所有页面。

## 二、技术栈

| 层级 | 技术 | 版本 |
|------|------|------|
| **前端框架** | React | 19 |
| **构建工具** | Vite | 6 |
| **语言** | TypeScript | ~5.6 |
| **样式** | Tailwind CSS | 3.4 |
| **UI 组件库** | shadcn/ui | — |
| **动画** | Framer Motion | — |
| **图表** | Recharts | — |
| **路由** | React Router | 7 |
| **后端框架** | Hono + tRPC | — |
| **数据库** | MySQL (via Drizzle ORM) | — |
| **认证** | Kimi OAuth 2.0 | — |

## 三、目录结构

```
/mnt/agents/output/app/
│
├── src/                          # 前端源代码
│   ├── components/               # React 组件
│   │   ├── ui/                   # shadcn/ui 组件库（50+ 组件）
│   │   ├── DashboardLayout.tsx   # Dashboard 侧边栏布局
│   │   ├── Footer.tsx            # 页脚组件
│   │   ├── LanguageSwitcher.tsx  # 中英文语言切换器
│   │   ├── Layout.tsx            # 首页布局（含导航+背景+页脚）
│   │   ├── Navbar.tsx            # 顶部导航栏
│   │   ├── NeuralNetworkBackground.tsx  # 神经网络 Canvas 动画背景
│   │   ├── PageBackground.tsx    # 跨页面统一背景容器
│   │   └── SacredBackground.tsx  # 完整背景层（网格+光球+Canvas+暗角）
│   ├── hooks/                    # 自定义 Hooks
│   │   └── use-mobile.ts         # 移动端检测
│   ├── i18n/                     # 国际化
│   │   ├── I18nContext.tsx       # I18n Context Provider
│   │   └── translations.ts       # 中英翻译字典（~160 条键值）
│   ├── lib/                      # 工具库
│   │   ├── api.ts                # REST API 客户端封装
│   │   └── utils.ts              # 通用工具函数
│   ├── pages/                    # 页面级组件
│   │   ├── Home.tsx              # 首页（Hero+Features+Skills+API+CTA）
│   │   ├── Login.tsx             # 登录页
│   │   ├── Register.tsx          # 注册页
│   │   ├── DashboardOverview.tsx # 控制台总览
│   │   ├── DashboardMemories.tsx # 记忆浏览管理
│   │   ├── DashboardGraph.tsx    # 知识图谱可视化
│   │   ├── DashboardSkills.tsx   # 技能管理
│   │   └── DashboardSubAccounts.tsx  # 子账户管理
│   ├── providers/                # 数据 Provider
│   │   └── trpc.tsx              # tRPC 客户端配置
│   ├── App.tsx                   # 路由定义（HashRouter）
│   ├── main.tsx                  # 应用入口
│   └── index.css                 # 全局样式（设计系统+动画+字体）
│
├── api/                          # 后端 API
│   ├── auth-router.ts            # 认证路由
│   ├── boot.ts                   # Hono 服务器启动
│   ├── context.ts                # tRPC 上下文
│   ├── middleware.ts             # 中间件
│   ├── notes-router.ts           # 笔记路由（模板遗留）
│   ├── router.ts                 # 路由注册
│   ├── kimimini/                 # Kimi 认证模块
│   │   ├── auth.ts
│   │   ├── platform.ts
│   │   ├── session.ts
│   │   └── types.ts
│   ├── lib/                      # 后端工具库
│   │   ├── cookies.ts
│   │   ├── env.ts
│   │   ├── http.ts
│   │   └── vite.ts
│   └── queries/                  # 数据库查询
│       ├── connection.ts         # MySQL 连接
│       └── users.ts
│
├── contracts/                    # 前后端共享类型
│   ├── constants.ts
│   ├── errors.ts
│   └── types.ts
│
├── db/                           # 数据库
│   ├── schema.ts                 # Drizzle ORM 表定义
│   ├── relations.ts              # 表关系
│   ├── seed.ts                   # 种子数据
│   └── migrations/               # 迁移文件
│
├── public/                       # 静态资源
├── index.html                    # HTML 入口
├── vite.config.ts                # Vite 配置
├── tailwind.config.js            # Tailwind 配置
├── tsconfig.json                 # TypeScript 配置
├── drizzle.config.ts             # Drizzle 配置
├── .env                          # 环境变量
└── package.json                  # 依赖管理
```

## 四、关键文件详解

### 4.1 页面路由（App.tsx）

| 路径 | 页面组件 | 说明 |
|------|----------|------|
| `/` | `Home` | 首页（Hero + Features + Skills + API + CTA） |
| `/login` | `Login` | 登录页 |
| `/register` | `Register` | 注册页 |
| `/dashboard` | `DashboardOverview` | 控制台总览 |
| `/dashboard/memories` | `DashboardMemories` | 记忆浏览管理 |
| `/dashboard/graph` | `DashboardGraph` | 知识图谱可视化 |
| `/dashboard/skills` | `DashboardSkills` | 技能管理 |
| `/dashboard/accounts` | `DashboardSubAccounts` | 子账户管理 |
| `/docs` | 占位 | API 文档（待开发） |
| `/guide` | 占位 | 快速上手指南（待开发） |
| `/community` | 占位 | 社区技能库（待开发） |
| `/benchmarks` | 占位 | 性能基准（待开发） |

**注意**：使用 `HashRouter`（`/#/...`），支持静态部署。

### 4.2 神经网络背景（NeuralNetworkBackground.tsx）

核心视觉组件，使用 HTML5 Canvas 2D API 渲染：

| 参数 | 值 | 说明 |
|------|-----|------|
| 神经元数量 | 350 | 密集分布 |
| 连接距离 | 120px | 短距离高密集连线 |
| 神经元半径 | 0.5~1.1px | 小体积高密度 |
| 发光半径 | 2.8x | 紧凑发光 |
| Canvas DPR | 强制 2x | 保证高分辨率 |
| 连线透明度 | 0.28 | 适中可见度 |
| 脉冲频率 | 0.15/帧 | 金色光点流动 |
| 射线数量 | 最多 18 条 | 金色光束 |
| 射线宽度 | 1.2px | 纤细锐利 |

**关键实现**：
- 神经元分布在 12×8 网格上 + 随机偏移
- 每帧更新位置（微漂移动画）
- 鼠标靠近产生引力交互
- Hub 节点（每 25 个）带多层同心发光环

### 4.3 设计系统（index.css）

颜色体系：
```
--bg-void:        #030305     # 最深背景
--bg-primary:     #0a0a0f     # 主背景
--bg-secondary:   #111118     # 次背景
--bg-card:        rgba(255,255,255,0.04)  # 卡片背景
--text-primary:   #f0f0f5     # 主文字
--text-secondary: #9ca3af     # 次文字
--text-tertiary:  #6b7280     # 辅助文字
--accent-purple:  #a855f7     # 紫色强调
--accent-magenta: #d946ef     # 品红强调
--accent-gold:    #FFD700     # 金色强调
```

组件样式：
- `btn-primary`：紫红渐变按钮 + 紫色阴影
- `btn-secondary`：透明边框按钮
- `glass-card`：毛玻璃卡片效果
- `dark-input`：深色输入框 + 紫色聚焦光环
- `feature-card-dark`：深色特性卡片 + hover 上浮

### 4.4 API 客户端（lib/api.ts）

封装所有后端接口调用：

| 模块 | 方法 | 端点 |
|------|------|------|
| 认证 | `loginUser()` | POST `/v1/login` |
| 认证 | `registerUser()` | POST `/register` |
| 记忆 | `storeMemory()` | POST `/v1/remember` |
| 记忆 | `searchMemories()` | POST `/v1/search` |
| 记忆 | `deleteMemory()` | DELETE `/v1/memories/:id` |
| 记忆 | `getTimeline()` | GET `/v1/timeline` |
| 统计 | `getStats()` | GET `/v1/stats` |
| 图谱 | `getKnowledgeGraph()` | POST `/v1/knowledge` |
| 图谱 | `getGraphExport()` | GET `/v1/graph/export` |
| 技能 | `getMySkills()` | GET `/v1/skills` |
| 技能 | `getPublicSkills()` | GET `/v1/skills/public` |
| 子账户 | `getSubAccounts()` | GET `/v1/subaccounts` |
| 子账户 | `createSubAccount()` | POST `/v1/subaccounts/create` |

缓存系统：30 秒 TTL 内存缓存，写操作自动失效。

### 4.5 国际化（i18n/）

- `I18nContext.tsx`：React Context 实现，提供 `t()` 翻译函数
- `translations.ts`：双语字典，约 160 条键值（中英对照）
- `LanguageSwitcher.tsx`：EN/ZH 切换按钮，localStorage 持久化

### 4.6 知识图谱可视化（DashboardGraph.tsx）

自定义 Canvas 力导向图引擎：

| 特性 | 实现 |
|------|------|
| 物理引擎 | 斥力 + 引力 + 中心力 + 阻尼 |
| 节点数量 | 80（mock 数据） |
| 簇着色 | 10 色 palette |
| 交互 | 滚轮缩放 + 拖拽平移 |
| 连线 | 紫色渐变，强度决定透明度 |
| 渲染 | 双层 glow + 实心 core |

### 4.7 数据图表（DashboardOverview.tsx）

使用 Recharts 库：
- `AreaChart`：30 天记忆增长趋势图
- 紫色渐变填充 + 深色网格线
- 自定义 tooltip（深色玻璃风格）

## 五、环境变量

```
DATABASE_URL=mysql://user:pass@host:port/db
VITE_APP_ID=your_app_id
VITE_KIMI_AUTH_URL=your_auth_url
```

由 `init.sh` 自动生成 `.env` 文件，**不要手动修改**。

## 六、常用命令

```bash
# 开发模式（前端+后端热重载）
npm run dev

# 类型检查
npm run check

# 生产构建
npm run build

# 数据库推送（开发）
npm run db:push

# 数据库迁移
npm run db:generate
npm run db:migrate

# 格式化
npm run format

# 测试
npm run test
```

## 七、部署建议

### 7.1 静态部署（仅前端）

当前项目配置为静态部署模式：

1. 使用 `HashRouter`（URL 格式：`/#/path`）
2. `vite.config.ts` 设置 `base: './'`
3. 构建输出目录：`dist/public/`

**部署步骤**：
```bash
npm run build
# 将 dist/public/ 部署到任意静态服务器
```

**适用平台**：
- Vercel / Netlify / Cloudflare Pages
- Nginx / Apache
- GitHub Pages
- 对象存储（OSS / S3 + CDN）

**Nginx 配置示例**：
```nginx
server {
    listen 80;
    server_name epicode.example.com;
    root /var/www/epicode/dist/public;
    index index.html;

    location / {
        try_files $uri $uri/ /index.html;
    }

    # API 反向代理到后端
    location /api {
        proxy_pass http://localhost:3000;
    }
}
```

### 7.2 全栈部署（前端 + 后端 + 数据库）

**方案一：Docker Compose**
```yaml
version: '3'
services:
  app:
    build: .
    ports:
      - "3000:3000"
    environment:
      - DATABASE_URL=mysql://root:pass@db:3306/epicode
    depends_on:
      - db
  db:
    image: mysql:8
    environment:
      - MYSQL_ROOT_PASSWORD=pass
      - MYSQL_DATABASE=epicode
    ports:
      - "3306:3306"
```

**方案二：分离部署**
- 前端：静态部署到 CDN
- 后端：部署到云服务器 / 容器平台（端口 3000）
- 数据库：云数据库（MySQL 8.0+）

**推荐架构**：
```
[CDN] ← 静态资源（HTML/CSS/JS）
  │
[LB] ← Nginx 反向代理
  │
[App Server] × N ← Hono + tRPC
  │
[MySQL] ← 主从集群
```

### 7.3 性能优化建议

1. **首屏加载**：
   - 启用 Gzip/Brotli 压缩
   - 配置 CDN 缓存（静态资源 1 年）
   - 考虑 SSR（如首屏时间 > 2s）

2. **Canvas 性能**：
   - 已限制 DPR ≤ 2x
   - 已使用 `requestAnimationFrame`
   - 移动端可考虑减少神经元数量（350 → 200）

3. **API 优化**：
   - 图表数据建议后端聚合（减少传输量）
   - 记忆列表启用虚拟滚动（> 500 条时）
   - 知识图谱建议后端 GraphQL 查询

### 7.4 安全建议

1. API Key 存储在 localStorage，生产环境建议：
   - 使用 HttpOnly Cookie
   - 或 Token 自动刷新机制
2. 启用 HTTPS（所有 API 通信）
3. 配置 CORS 白名单
4. 数据库使用独立账号，限制权限

## 八、开发路线图

| 阶段 | 状态 | 内容 |
|------|------|------|
| Phase 1 | ✅ 完成 | 首页、登录、注册、设计系统 |
| Phase 2 | ✅ 完成 | Dashboard（Overview/Memories/Graph/Skills/SubAccounts） |
| Phase 3 | 🔄 待开发 | API 文档页、快速上手指南、社区技能库、性能基准页 |
| Phase 4 | 📋 规划 | Modal 弹窗（Store/Search/Digest）、深色/浅色主题切换 |

## 九、常见问题

**Q1: 背景在某些设备上卡顿？**  
A: Canvas 动画在低端设备上可能掉帧。可在 `NeuralNetworkBackground.tsx` 中减少 `N_COUNT`（350 → 200）并增大 `step` 间隔（30 → 40）。

**Q2: API 调用报 401？**  
A: 检查 `localStorage` 中 `epicode_api_key` 是否存在。登录成功后会自动写入。

**Q3: 数据库迁移失败？**  
A: 使用 `npm run db:push` 而非 `db:migrate` 进行开发环境同步。生产环境务必先生成迁移文件。

**Q4: 构建后路由 404？**  
A: 确保 Web 服务器配置 `try_files` 指向 `index.html`。HashRouter 模式不需要此配置，但刷新页面时可能需要。
