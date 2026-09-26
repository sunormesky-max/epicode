/** 去除 <think>...</think> 标签，返回纯回答文本（拆自 MarkdownText, react-refresh only-export-components） */
export function stripThinkTags(text: string): string {
  return text.replace(/<think>[\s\S]*?<\/think>/gi, '').trim();
}
