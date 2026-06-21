# Contributing to Epicode

感谢你对 Epicode 的兴趣！我们欢迎所有形式的贡献。

## 快速开始

1. **Fork 仓库** 并克隆到本地
2. **设置开发环境**：
   - Rust 1.88+
   - Node.js 20+
   - SQLite
3. **运行测试**：
   ```bash
   cd backend && cargo test --all-targets
   cd frontend && npm test
   ```

## 贡献方式

### 1. 报告问题
- 使用 GitHub Issues 模板
- 包含复现步骤、环境信息、错误日志

### 2. 提交代码
- 遵循 [Conventional Commits](https://conventionalcommits.org/)
- 所有 PR 必须通过 CI 检查
- 至少一个维护者审核

### 3. 分享技能
Epicode 支持社区技能共享：
- 在 `skills/` 目录添加你的技能
- 使用标准 SKILL.md 格式
- 通过 `epicode-sdk` 的 `skills_sync` 工具同步

### 4. 参与讨论
- 在 GitHub Discussions 分享想法
- 回复其他用户的问题
- 参与路线图规划

## 社区记忆

Epicode 使用空间记忆系统记录社区知识：
- 技术决策存入 `decision` 标签
- 最佳实践存入 `pattern` 标签
- 用户反馈存入 `feedback` 标签

## 行为准则

参见 [CODE_OF_CONDUCT.md](CODE_OF_CONDUCT.md)。

## 获取帮助

- 📖 [文档](https://epicode.cn/docs)
- 💬 [Discussions](https://github.com/sunormesky-max/epicode/discussions)
- 🐛 [Issues](https://github.com/sunormesky-max/epicode/issues)
- 🚀 [Discord](https://discord.gg/epicode) (即将推出)

## 许可证

MIT License - 参见 [LICENSE](LICENSE)
