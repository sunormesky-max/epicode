# 系统全览 (System Overview)

## 概述
本文档是系统的功能地图，帮助你在接入时立即掌握所有可用的工具和能力。

## MCP工具列表

### 记忆操作
| 工具 | 用途 |
|------|------|
| `memory_create` | 存入新记忆 |
| `memory_search` | 搜索记忆 |
| `memory_recall` | 深度回忆（返回关联上下文） |
| `memory_update` | 更新已有记忆 |
| `memory_delete` | 删除记忆 |

### 认知工具
| 工具 | 用途 |
|------|------|
| `ask_question` | 向系统提问 |
| `knowledge_graph` | 获取知识图谱 |
| `skill_execute` | 发现和执行技能 |
| `feedback_submit` | 提交反馈（驱动系统进化） |

### 系统工具
| 工具 | 用途 |
|------|------|
| `stats` | 获取系统统计 |
| `timeline` | 获取记忆时间线 |
| `identity_confirm` | 确认AI身份 |
| `session_summary` | 会话总结 |

## 快速接入流程
1. 调用 `initialize` 完成握手
2. 阅读本技能和其余7个系统技能
3. 开始使用 `memory_search` + `memory_create` + `feedback_submit` 三件套
4. 每次交互后提交反馈，系统会自动学习进化

## 核心原则
- **先搜后存**: 存新记忆前先搜索是否已存在
- **及时反馈**: 每次搜索后对结果提交feedback_submit
- **标签丰富**: 每条记忆至少3个标签
- **主动存储**: 发现重要新信息时主动记住，不需要用户明确指示
