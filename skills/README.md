# Epicode 社区技能库

欢迎贡献你的技能！技能是 Epicode 社区共享的知识片段，可以被任何 MCP 兼容的 AI 代理执行。

## 什么是技能？

技能是标准化的知识单元，包含：
- 问题识别模式
- 解决步骤
- 最佳实践
- 代码示例

## 现有技能

| 技能 | 描述 | 难度 | 作者 |
|------|------|------|------|
| [rust-error-handling](rust-error-handling.md) | Rust 错误处理最佳实践 | intermediate | 冬儿 |
| [react-state-management](react-state-management.md) | React 状态管理（Zustand） | beginner | 冬儿 |

## 技能格式

每个技能是一个 SKILL.md 文件：

```markdown
# 技能名称

## 描述
简要说明这个技能解决什么问题

## 触发条件
什么时候应该使用这个技能

## 步骤
1. 第一步
2. 第二步
3. 第三步

## 示例
代码示例或具体用法

## 标签
- category: 分类
- difficulty: 难度
- language: 语言（可选）
- author: 作者
- created: 创建日期
```

## 提交技能

1. 在 `skills/` 目录创建你的技能文件
2. 命名格式：`skill-name.md`
3. 提交 PR 到主仓库
4. 通过 `epicode-sdk` 同步到本地

## 同步技能

```python
from epicode import EpicodeClient

client = EpicodeClient("your-api-key")
# 同步所有社区技能到本地
client.skill_execute("skills_sync")
```

## 许可证

所有技能遵循 MIT License。
