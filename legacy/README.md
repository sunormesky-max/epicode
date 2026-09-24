# legacy/ — 七月开源线独有模块 (2026-09 集成时保全)

这些模块属于 2026-07 GitHub 开源线, 在生产引擎 2026-09 集成 (#PR integration/prod-engine) 时
未被生产分支吸收。存档于此不参与编译, 待后续反向移植:

- `api/{server,authz,middleware}.rs` — 重构版 HTTP 层 (生产用的是 bin/cloud/ 模块化布局)
- `engine/{plugin,cluster,key_rotation,audit,cache,decision_center}.rs` — 插件系统/分布式部署基础/密钥轮换/审计/缓存/决策中心
- `domain/permission.rs` + `util.rs` — 权限域模型与工具函数
