# 文档国际化（i18n）

Epicode 文档采用中英双语维护。本目录存放核心文档的中文翻译。

## 目录结构

```
docs/
├── README.md                 # 英文母版（权威源）
├── architecture.md           # 架构（英文）
├── deployment.md             # 部署（英文）
├── development.md            # 开发指南（英文）
├── api-reference.md          # API 参考（英文）
├── i18n/
│   └── zh/
│       ├── README.md         # 中文索引
│       ├── architecture.md   # 架构（中文）
│       ├── deployment.md     # 部署（中文）
│       ├── development.md    # 开发指南（中文）
│       └── api-reference.md  # API 参考（中文）
└── ...
```

## 翻译规则

1. **英文为权威源**：技术细节以英文版为准，中文版为翻译+本地化补充
2. **保持结构一致**：中英文文档章节顺序、代码示例、链接必须对应
3. **代码块不翻译**：命令、代码、配置保持原文
4. **专有名词保留英文**：Epicode、Tetrahedron、MCP、ONNX 等保留英文
5. **章节标题翻译**：使用中文标题，但保留英文原标题在括号内（首次出现）

## 同步策略

- 当前采用**手动维护**策略
- 英文文档变更后，在 PR 描述中标注 `i18n: zh needs update`
- 长期可考虑接入自动化翻译工具（如 Crowdin、Weblate）

## 已翻译文档

| 文档 | 英文 | 中文 |
|------|------|------|
| README | [README.md](../README.md) | [README.zh.md](../../README.zh.md) |
| 架构 | [architecture.md](../architecture.md) | 待翻译 |
| 部署 | [deployment.md](../deployment.md) | 待翻译 |
| 开发指南 | [development.md](../development.md) | 待翻译 |
| API 参考 | [api-reference.md](../api-reference.md) | 待翻译 |
