# Changelog

All notable changes to this project will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.1.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

## [1.8.1](https://github.com/sunormesky-max/epicode/compare/v1.8.0...v1.8.1) (2026-10-09)


### Bug Fixes

* **guard:** preserve enforcement under log and firewall churn ([#230](https://github.com/sunormesky-max/epicode/issues/230)) ([5049b4f](https://github.com/sunormesky-max/epicode/commit/5049b4f903cc2fae74e4d62aef29047b10331bb5))
* **memory:** preserve memory_improve content ([#231](https://github.com/sunormesky-max/epicode/issues/231)) ([67f6440](https://github.com/sunormesky-max/epicode/commit/67f6440665fa0f2b370577da353aee6acba34d74))

## [1.8.0](https://github.com/sunormesky-max/epicode/compare/v1.7.0...v1.8.0) (2026-10-09)


### Features

* **adaptive:** 自适应阈值跨重启持久化并在 user_stats 中可观测 ([#215](https://github.com/sunormesky-max/epicode/issues/215)) ([4bf83c4](https://github.com/sunormesky-max/epicode/commit/4bf83c46dfc00b5c78e43a235630059254eb24a1))
* **scheduler:** 分层运行协调 — 模型调用不再占用 cycle 门,冷记忆复习移出 auto_save,分层指标可观测 ([#218](https://github.com/sunormesky-max/epicode/issues/218)) ([f18cbfa](https://github.com/sunormesky-max/epicode/commit/f18cbfaf9d104cd0bf38d954678b8b8a9ecc5d96))
* **site:** per-route titles/meta (zh/en), real footer links, a11y fixes ([#217](https://github.com/sunormesky-max/epicode/issues/217)) ([22ca8db](https://github.com/sunormesky-max/epicode/commit/22ca8db61210527e70a35c8958d0799d95d3fe22))
* **site:** zero-dependency site search (pages + API endpoints) and docs filter ([#220](https://github.com/sunormesky-max/epicode/issues/220)) ([6413daf](https://github.com/sunormesky-max/epicode/commit/6413daf4650b3c661d3dc678a9b02fa25549f3ed))


### Bug Fixes

* **dream:** protect enforced memories during consolidation ([#224](https://github.com/sunormesky-max/epicode/issues/224)) ([0122814](https://github.com/sunormesky-max/epicode/commit/01228146abb641eda57ea82884cf9ad5ca4ad5bd))
* **drive:** retain signals until archival is durable ([#227](https://github.com/sunormesky-max/epicode/issues/227)) ([45c4bc3](https://github.com/sunormesky-max/epicode/commit/45c4bc32c70deba58e438c895881ce5c16c84120))
* **knowledge:** make concept membership idempotent ([#228](https://github.com/sunormesky-max/epicode/issues/228)) ([4b2036b](https://github.com/sunormesky-max/epicode/commit/4b2036b600f9b5e03116d84ec0572b1559429459))
* **pulse:** traverse logical Port links for reseeded clusters ([#221](https://github.com/sunormesky-max/epicode/issues/221)) ([9d0a530](https://github.com/sunormesky-max/epicode/commit/9d0a5306e0db9ce79f07696a1fd617a9acce7203))
* **runtime:** 只在绑定身份变化时写 binding-anchor 记忆,控制台重复注册不再无界增长 ([#214](https://github.com/sunormesky-max/epicode/issues/214)) ([184efb8](https://github.com/sunormesky-max/epicode/commit/184efb878271b9deb77c9089a687c0bcc420fdda))
* **scheduler:** 冷记忆复习每条记忆冷却期内只复习一次,判断不再被重复施加 ([#219](https://github.com/sunormesky-max/epicode/issues/219)) ([4965fc2](https://github.com/sunormesky-max/epicode/commit/4965fc2657780bc41d3531a8be2715a792f6dafb))
* **security:** enforce paid setting and grant permissions ([#229](https://github.com/sunormesky-max/epicode/issues/229)) ([770014e](https://github.com/sunormesky-max/epicode/commit/770014e8157ae61276140c1e3e1ac639eba0a2b7))
* **space:** transfer seed Port reservations by exact ID ([#213](https://github.com/sunormesky-max/epicode/issues/213)) ([991e369](https://github.com/sunormesky-max/epicode/commit/991e36986c057f1e158455b05cab4e499e04bb3b))
* **storage:** refuse incomplete encrypted memory loads ([#226](https://github.com/sunormesky-max/epicode/issues/226)) ([bb3ce24](https://github.com/sunormesky-max/epicode/commit/bb3ce24da892fa3e7598f3717f590b0940ea58cc))


### Performance Improvements

* **frontend:** stop preloading recharts + framer-motion on every page ([#216](https://github.com/sunormesky-max/epicode/issues/216)) ([33543da](https://github.com/sunormesky-max/epicode/commit/33543da0a0e09666ece15f216c5d33c78ec06866))

## [1.7.0](https://github.com/sunormesky-max/epicode/compare/v1.6.1...v1.7.0) (2026-10-08)


### Features

* **theme:** [#205](https://github.com/sunormesky-max/epicode/issues/205)硬编码色token化 + [#206](https://github.com/sunormesky-max/epicode/issues/206)用户可选强调色 (堆叠链尾段于main重建, rev of [#205](https://github.com/sunormesky-max/epicode/issues/205)/[#206](https://github.com/sunormesky-max/epicode/issues/206)/[#211](https://github.com/sunormesky-max/epicode/issues/211)) ([#212](https://github.com/sunormesky-max/epicode/issues/212)) ([b4879b5](https://github.com/sunormesky-max/epicode/commit/b4879b5be6ed2e66fc5876f6c0810d849ecdd92b))
* **theme:** 主题感知图表色板 + 无闪烁启动 + WCAG AA 对比度 / theme-aware charts, no-flash boot, AA contrast tokens ([#203](https://github.com/sunormesky-max/epicode/issues/203)) ([b38494f](https://github.com/sunormesky-max/epicode/commit/b38494fae3ed3b33b95aa84f9130dc33ff860ba5))


### Bug Fixes

* **drive:** keep proposals pending until execution feedback ([#208](https://github.com/sunormesky-max/epicode/issues/208)) ([00171c5](https://github.com/sunormesky-max/epicode/commit/00171c5e381e43a57df3268ce4b83ee96bdc44b9))
* **pulse:** persist mass updates through auto-save retries ([#201](https://github.com/sunormesky-max/epicode/issues/201)) ([71a2da9](https://github.com/sunormesky-max/epicode/commit/71a2da91cc4f2c238bf0900a23ee9918b2bd3625))
* **space:** keep ordinary vertex IDs distinct from Ports ([#209](https://github.com/sunormesky-max/epicode/issues/209)) ([e6e2787](https://github.com/sunormesky-max/epicode/commit/e6e2787e48a2fc891a99bda4d22c32397d45d07b))
* **space:** preserve Port vertices and anchors on removal ([#200](https://github.com/sunormesky-max/epicode/issues/200)) ([31b257a](https://github.com/sunormesky-max/epicode/commit/31b257a7344e3fd7bd86a1b6c3ef0a10042ee10b))
* **theme:** 补齐未定义 token + 浅色可读性 + 无 JS 浅色回退 ([#204](https://github.com/sunormesky-max/epicode/issues/204) 原diff于main重建, rev of [#210](https://github.com/sunormesky-max/epicode/issues/210)) ([#210](https://github.com/sunormesky-max/epicode/issues/210)) ([6bae477](https://github.com/sunormesky-max/epicode/commit/6bae477fef44e2199805d3e3b89dd47f906c198c))

## [1.6.1](https://github.com/sunormesky-max/epicode/compare/v1.6.0...v1.6.1) (2026-10-07)


### Bug Fixes

* **dream:** skip already-quarantined / superseded memories so dream phases are idempotent ([#196](https://github.com/sunormesky-max/epicode/issues/196)) ([337a0be](https://github.com/sunormesky-max/epicode/commit/337a0be474c835767e9ef0292f9759c75ce09325))
* **quota:** don't consume memory quota on rejected node_create / doc_import ([#194](https://github.com/sunormesky-max/epicode/issues/194)) ([1d4683e](https://github.com/sunormesky-max/epicode/commit/1d4683e61fcdf4f5c900708c2ccab115f2f5d36f))
* **scheduler:** single-flight all dream entry points and count manual dreams toward the interval ([#197](https://github.com/sunormesky-max/epicode/issues/197)) ([c0a72b7](https://github.com/sunormesky-max/epicode/commit/c0a72b7a6fa02a53cd2179c96e71496aad746db9))
* **security:** close MCP and cloud authorization bypasses ([#199](https://github.com/sunormesky-max/epicode/issues/199)) ([8926ab0](https://github.com/sunormesky-max/epicode/commit/8926ab02882514aacb36774068b9de6d879f953e))
* **timeline:** sort globally before paginating /v1/timeline ([#195](https://github.com/sunormesky-max/epicode/issues/195)) ([4f647dd](https://github.com/sunormesky-max/epicode/commit/4f647dd9823034ecd0e698176f4f9d13e90bff6d))

## [1.6.0](https://github.com/sunormesky-max/epicode/compare/v1.5.0...v1.6.0) (2026-10-06)


### Features

* 主题账户跟随+自定义主题(免费锁)+权限体系重建(11权限点+记忆输出控制) ([#181](https://github.com/sunormesky-max/epicode/issues/181)) ([d20853a](https://github.com/sunormesky-max/epicode/commit/d20853ad758362c7ee0f0932e46fa76961f2cf7c))


### Bug Fixes

* **frontend:** npm audit fix for source-map-js and others ([#180](https://github.com/sunormesky-max/epicode/issues/180)) ([ba53b14](https://github.com/sunormesky-max/epicode/commit/ba53b149b99001b7e416648cd69d6ddaeb95a6ff))
* **memory-os:** slim envelope retains valid + timestamps ([#179](https://github.com/sunormesky-max/epicode/issues/179)) ([25ecd05](https://github.com/sunormesky-max/epicode/commit/25ecd053ab7b610f39012a486fe2408720c0ecd9))
* **ui:** 压缩能量→亮度曲线并下调地平线光带 — 修复满能量背景穿透正文的穿模 ([#183](https://github.com/sunormesky-max/epicode/issues/183)) ([4ba8618](https://github.com/sunormesky-max/epicode/commit/4ba8618d732af5f6368505613ecf2c90cb8bf1c1))

## [1.5.0](https://github.com/sunormesky-max/epicode/compare/v1.4.0...v1.5.0) (2026-10-05)


### Features

* **mcp:** add Tool annotations (readOnlyHint/destructiveHint/idempotentHint) ([#167](https://github.com/sunormesky-max/epicode/issues/167)) ([ebcc58b](https://github.com/sunormesky-max/epicode/commit/ebcc58b7b8f4d49a2652389b94c4f2b7fb29441c))
* **mcp:** compact memory_recall/search responses + timestamp_iso ([#168](https://github.com/sunormesky-max/epicode/issues/168)) ([332028b](https://github.com/sunormesky-max/epicode/commit/332028b369a2e91f41c0e587b5f12e465923f4f7))
* **mcp:** paginate skills_sync manifest (limit/offset) ([#169](https://github.com/sunormesky-max/epicode/issues/169)) ([75f8344](https://github.com/sunormesky-max/epicode/commit/75f83446c8b8922827c5308ee30a35e6d598df31))
* **memory-os:** S1 slim envelope + S2 MemCard (flags default OFF) ([#175](https://github.com/sunormesky-max/epicode/issues/175)) ([153d3ff](https://github.com/sunormesky-max/epicode/commit/153d3ff4cde73aea18cbd82429e6932b54414804))
* **scheduler:** honor should_dream and count missed commits ([#174](https://github.com/sunormesky-max/epicode/issues/174)) ([acb9a00](https://github.com/sunormesky-max/epicode/commit/acb9a007bcc928e4c8994038c6a8ea624ea5d87b))
* **theme:** X 三主题 + 主题中心升级 / X themes (x-dark/x-light/x-dim) + Theme Center upgrade ([#166](https://github.com/sunormesky-max/epicode/issues/166)) ([c218086](https://github.com/sunormesky-max/epicode/commit/c21808677601eafe86facd264d6f1983bae9caf6))


### Bug Fixes

* **backend:** strip_html 遇孤立 '&lt;' 不再吞掉后续内容 ([#162](https://github.com/sunormesky-max/epicode/issues/162)) ([f960891](https://github.com/sunormesky-max/epicode/commit/f960891a1c406b4f38a4afb2185385a4ccc2ce34))
* **frontend:** 图书馆收集请求审批点击取消后不再提交 ([#164](https://github.com/sunormesky-max/epicode/issues/164)) ([783a934](https://github.com/sunormesky-max/epicode/commit/783a9342d290f481ea8cf2cefa694ac36e5c9905))
* **mcp:** feedback_submit reinforces edges and honors from_id/to_id ([#173](https://github.com/sunormesky-max/epicode/issues/173)) ([37c1393](https://github.com/sunormesky-max/epicode/commit/37c13932019874402848b28c42d7c3156f27999e))
* **memory:** unify live importance floor at 0.3 ([#176](https://github.com/sunormesky-max/epicode/issues/176)) ([beb476b](https://github.com/sunormesky-max/epicode/commit/beb476be58a0fe50f321b99a966aa10da22f1e80))
* **scheduler:** api_dream consumes energy (R4-S04) ([#172](https://github.com/sunormesky-max/epicode/issues/172)) ([b25e2cd](https://github.com/sunormesky-max/epicode/commit/b25e2cd6ef429f217fde8343a338e8e37ac36459))


### Documentation

* **roadmap:** add Memory OS next section linking [#170](https://github.com/sunormesky-max/epicode/issues/170) ([#171](https://github.com/sunormesky-max/epicode/issues/171)) ([7be6e42](https://github.com/sunormesky-max/epicode/commit/7be6e4229877f821fe66e26c2576873e7cb2c4e0))

## [1.4.0](https://github.com/sunormesky-max/epicode/compare/v1.3.0...v1.4.0) (2026-10-03)


### Features

* append-only grain ledger beside tetra placement ([#151](https://github.com/sunormesky-max/epicode/issues/151)) ([86a1795](https://github.com/sunormesky-max/epicode/commit/86a17953a12fe003d88bb936d858eb1a5f2803d6))
* horizon cadence for the central scheduler ([#153](https://github.com/sunormesky-max/epicode/issues/153)) ([5eb49af](https://github.com/sunormesky-max/epicode/commit/5eb49af0b465f0088b4e45ffc4286c5f2bd9cfb4))
* shared theme center for the site and console ([#152](https://github.com/sunormesky-max/epicode/issues/152)) ([396e821](https://github.com/sunormesky-max/epicode/commit/396e821a246c1774e0b94859ce56a323219556d4))


### Bug Fixes

* **backend:** gate cloud auth tests module with cfg(test) to fix clippy unused_imports ([#155](https://github.com/sunormesky-max/epicode/issues/155)) ([9d902fd](https://github.com/sunormesky-max/epicode/commit/9d902fd61bc4648d17eef41996d75fff3aa9df42))


### Documentation

* state that retrieval is BM25 plus HNSW ([#154](https://github.com/sunormesky-max/epicode/issues/154)) ([59032bf](https://github.com/sunormesky-max/epicode/commit/59032bfb792ceebb3b5caa848eebee41f90d78d0))

## [1.3.0](https://github.com/sunormesky-max/epicode/compare/v1.2.0...v1.3.0) (2026-10-02)


### Features

* email login and human-configured subaccount permissions ([#145](https://github.com/sunormesky-max/epicode/issues/145)) ([c31cc95](https://github.com/sunormesky-max/epicode/commit/c31cc957dc991cf78cf80ce3b8dcf77dfe8e719f))
* **ui:** improve dashboard navigation and observation clarity ([#144](https://github.com/sunormesky-max/epicode/issues/144)) ([cb162b3](https://github.com/sunormesky-max/epicode/commit/cb162b37f0e26d0c10452bc77289315c1e2b193b))


### Bug Fixes

* persist runtime bindings, expose /mcp, and stop dropping API stats ([#143](https://github.com/sunormesky-max/epicode/issues/143)) ([1ed353b](https://github.com/sunormesky-max/epicode/commit/1ed353b1d9d1d3c90460e303c7fccaa685a83ae3))

## [1.2.0](https://github.com/sunormesky-max/epicode/compare/v1.1.0...v1.2.0) (2026-10-01)


### Features

* **graph:** 总览模式 — 三层渐进披露(星域超节点/下钻/焦点) ([#105](https://github.com/sunormesky-max/epicode/issues/105)) ([e5a98b1](https://github.com/sunormesky-max/epicode/commit/e5a98b13a52d35032b5cb42db0b5ac1eea4e6e87))
* 分级权限控制系统(RBAC) — 子账户四角色×8权限点全栈 ([#123](https://github.com/sunormesky-max/epicode/issues/123)) ([f812dd9](https://github.com/sunormesky-max/epicode/commit/f812dd9c64c8e7852f05f43d9763fc98204355d6))
* 知识图谱研究级优化 — 检索强化衰减+社区发现+概念聚合倒排+前端LOD/主干道 ([#104](https://github.com/sunormesky-max/epicode/issues/104)) ([2e56374](https://github.com/sunormesky-max/epicode/commit/2e5637448fcbe0f587b3f81892295d37eb50f112))


### Bug Fixes

* **archive:** preserve body on rename and cover role permissions ([#137](https://github.com/sunormesky-max/epicode/issues/137)) ([900c188](https://github.com/sunormesky-max/epicode/commit/900c1887468a1d5474fde0e5117afae61ce7d8de))
* clippy unnecessary_cast — now_ts()本就i64(补[#126](https://github.com/sunormesky-max/epicode/issues/126)遗漏) ([#127](https://github.com/sunormesky-max/epicode/issues/127)) ([f5d902f](https://github.com/sunormesky-max/epicode/commit/f5d902fd0c68a1568aa87da7809d7885cf447633))
* **frontend:** ⌘K复制密钥无密钥态改为引导 — 安全设计下密钥仅存内存, 会话刷新后本地必无密钥 ([#118](https://github.com/sunormesky-max/epicode/issues/118)) ([4f304a3](https://github.com/sunormesky-max/epicode/commit/4f304a303f52d0fc468d05b1aa84a090f795fac5))
* **frontend:** 剪贴板写入全站兜底 — 修微信内置浏览器等环境复制静默失败 ([#117](https://github.com/sunormesky-max/epicode/issues/117)) ([f76bab0](https://github.com/sunormesky-max/epicode/commit/f76bab0777c2f4ba713f6ae5b412c681f803359a))
* **graph:** 总览模式锚定物理 — 修settled后簇漂移叠死 ([#106](https://github.com/sunormesky-max/epicode/issues/106)) ([14620f2](https://github.com/sunormesky-max/epicode/commit/14620f2715404a7ed0d6529f6ca269ded58a162c))
* **MCP:** notification响应符合规范 — 202 Accepted+空body ([#132](https://github.com/sunormesky-max/epicode/issues/132)) ([35d3e05](https://github.com/sunormesky-max/epicode/commit/35d3e0566dffa4cbd3fd1b9556b504f9b8631f59))
* persist graph and library state ([#136](https://github.com/sunormesky-max/epicode/issues/136)) ([e1acc95](https://github.com/sunormesky-max/epicode/commit/e1acc954f936c146decfefac64fb3278b5ccb540))
* 密钥操作原生对话框→内联密码卡 — 修微信内置浏览器'复制密钥无反应' ([#130](https://github.com/sunormesky-max/epicode/issues/130)) ([2a75bf9](https://github.com/sunormesky-max/epicode/commit/2a75bf9d2d851e828b402a842d70638f344e04b1))


### Documentation

* **audit:** record security findings and local reproductions ([#134](https://github.com/sunormesky-max/epicode/issues/134)) ([cb4d412](https://github.com/sunormesky-max/epicode/commit/cb4d412f2294379b7c18a68b3f40b26c27fa275c))
* 全系统运转推演v2 — 细到每一颗记忆体, 大到系统物理学 ([#125](https://github.com/sunormesky-max/epicode/issues/125)) ([2bc57e2](https://github.com/sunormesky-max/epicode/commit/2bc57e2a465f9931b6a100f9802ee8f8185db0b6))


### Security

* RBAC层级泄漏三修复 — 普通用户admin升级攻击面审计闭环 ([#129](https://github.com/sunormesky-max/epicode/issues/129)) ([82e6802](https://github.com/sunormesky-max/epicode/commit/82e68026cba1c41fe14970ce5a8a035665b5c5c8))
* 审计[#134](https://github.com/sunormesky-max/epicode/issues/134)五项P1清偿 — A01/A02/A03/A09/A11 ([#135](https://github.com/sunormesky-max/epicode/issues/135)) ([7350210](https://github.com/sunormesky-max/epicode/commit/7350210ba82182787a7eb045bbb0edecfdc3bd7f))

## [1.1.0](https://github.com/sunormesky-max/epicode/compare/v1.0.3...v1.1.0) (2026-09-28)


### Features

* frontend整体替换为生产级前端树(双树合一) ([#92](https://github.com/sunormesky-max/epicode/issues/92)) ([84910a8](https://github.com/sunormesky-max/epicode/commit/84910a870e5f0950b859325c858eb0c7070eeaaf))
* 系统能力完整性+MCP一致性+前后端一致性+SMRP协议进化 ([#101](https://github.com/sunormesky-max/epicode/issues/101)) ([61c3ea7](https://github.com/sunormesky-max/epicode/commit/61c3ea7a0165386f2599f9050982e156f743d0ea))


### Bug Fixes

* align L0 and SMRP contracts ([2d99d60](https://github.com/sunormesky-max/epicode/commit/2d99d605faf32d0e75adbba879a70ab48452ed98))
* close audit gaps across auth and deployment ([1646922](https://github.com/sunormesky-max/epicode/commit/1646922c1beabf43455a56e844e1b364b3f87ae8))
* CodeQL trivial-conditional ([#94](https://github.com/sunormesky-max/epicode/issues/94)) ([e0df34a](https://github.com/sunormesky-max/epicode/commit/e0df34aad47f74b2ace32d51ad9d0f42d22f8414))
* keep API keys out of persistent web storage ([e1b6670](https://github.com/sunormesky-max/epicode/commit/e1b6670c5774f50c3bd1006be39d9e56f4cb65f9))
* keep search mode tests after implementation ([077d93a](https://github.com/sunormesky-max/epicode/commit/077d93ab29146b86e97ea739a4a0cfa0b64f24a2))
* keep SSE test module after handler ([ccdf156](https://github.com/sunormesky-max/epicode/commit/ccdf156b56ce830fddda142813bce4993a664148))
* resolve gateway smoke test upstreams ([bbfcee3](https://github.com/sunormesky-max/epicode/commit/bbfcee335c943b264d86223616400ff61d560dad))
* verify-version.sh在set -e下grep无匹配静默杀脚本 ([#88](https://github.com/sunormesky-max/epicode/issues/88)) ([366fa78](https://github.com/sunormesky-max/epicode/commit/366fa78f9cce110ee546f0b02269711b6e1b570f))
* 三轮审计全量修复 (1严重+4高+12中+垃圾清理) ([#90](https://github.com/sunormesky-max/epicode/issues/90)) ([3e1e224](https://github.com/sunormesky-max/epicode/commit/3e1e22419da401b4ed4d2d6505669374ca380e5d))
* 四轮审计残项全清(含两处上轮replace静默失败) ([#91](https://github.com/sunormesky-max/epicode/issues/91)) ([e32d3af](https://github.com/sunormesky-max/epicode/commit/e32d3afc539e1749d7ee23c38faf0787fdd45599))
* 审查发现A — nowSnapshot改数据加载时刷新(时间筛选不再用挂载时刻) ([#97](https://github.com/sunormesky-max/epicode/issues/97)) ([0e62f00](https://github.com/sunormesky-max/epicode/commit/0e62f00b655c3ace92cec4852a35286eb12d1037))


### Performance Improvements

* 修复大账户加载O(n²) — 33分钟冷启动降回秒级(刘启航报告'大体量账户加载极慢') ([#100](https://github.com/sunormesky-max/epicode/issues/100)) ([0d39a63](https://github.com/sunormesky-max/epicode/commit/0d39a6338f567527c78c8fc37821e5410129cc50))

## [1.0.3](https://github.com/sunormesky-max/epicode/compare/v1.0.2...v1.0.3) (2026-09-25)


### Bug Fixes

* release-please extra-files的toml/yaml更新器改generic ([#84](https://github.com/sunormesky-max/epicode/issues/84)) ([232964a](https://github.com/sunormesky-max/epicode/commit/232964a1dd3f94e2a6f0fb533b8d571a38cb88ee))
* 审计二轮全量收尾 ([#86](https://github.com/sunormesky-max/epicode/issues/86)) ([d18ee88](https://github.com/sunormesky-max/epicode/commit/d18ee88c42378e5b436f6e1a9d6952f211ce9881))

## [Unreleased]

### Added

- MCP Registry discovery JSON with 35 standardized tools (`.well-known/mcp/discovery.json`).
- Python SDK with SMRP tiered recall, identity rituals, and knowledge graph support.
- TypeScript SDK with differentiated features beyond basic remember/search.
- End-to-end AI Agent memory example (`examples/python/ai_agent_memory.py`).
- Release strategy documentation (`RELEASE_STRATEGY.md`).
- GitHub community health files: issue templates, PR template, CODE_OF_CONDUCT.md, SECURITY.md.
- GitHub Actions workflows: greetings.yml, discussions.yml, scorecard.yml.
- CI optimization with `paths-ignore` for markdown and docs changes.

### Changed

- Environment variable prefix migrated from `TETRAMEM_` to `EPICODE_` across 26 files.

### Fixed

- Backend compilation warnings in `backend/src/engine/mod.rs`.


## [1.0.1] - 2026-06-21

### Fixed

- Fixed admin authentication bypass in `cloud.rs` by rejecting empty `admin_key` and missing/empty `X-Admin-Key` header.
- Converted `blocking()` wrapper to return `Result<T, String>` instead of panicking when a `spawn_blocking` task fails.
- Added `SecurityConfig::try_from_env()` to avoid startup panic; cloud mode now exits gracefully on missing `EPICODE_API_KEY`.

### Security

- Replaced `ureq` with `attohttpc` in cognitive, embedding, and classifier modules.
- Limited Cloud TCP server concurrency with a `tokio::sync::Semaphore` to prevent unbounded OS thread creation.

### Changed

- Upgraded `ort` from `2.0.0-rc.9` to `2.0.0-rc.12`.

## [1.0.0] - 2026-06-20

### Added

- Initial open-source release of Epicode.
- Spatial AI memory system: tetrahedron storage, HNSW + BM25 search, knowledge graph.
- MCP integration with 35 standardized tools and SMRP response protocol.
- Multi-tenant Cloud mode with user management, quotas, and invite codes.
- React 19 frontend dashboard and Rust Axum backend.
- `epicode-guard` defense system for SSH/Web/honeypot protection.
- Docker Compose and Kubernetes deployment templates.
- Repository docs, issue/PR templates, Dependabot configuration, and MIT license.

[Unreleased]: https://github.com/sunormesky-max/epicode/compare/v1.0.1...HEAD
[1.0.1]: https://github.com/sunormesky-max/epicode/compare/v1.0.0...v1.0.1
[1.0.0]: https://github.com/sunormesky-max/epicode/releases/tag/v1.0.0
