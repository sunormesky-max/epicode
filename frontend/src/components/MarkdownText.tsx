import React from 'react';

/**
 * 轻量级 Markdown 渲染器（无外部依赖）。
 * 支持：标题(#~######)、粗体(**text**)、行内代码(`code`)、
 * 列表(- / *)、有序列表(1.)、分隔线(---)、段落。
 *
 * 同时剥离 MiniMax-M3 等推理模型的 <think>...</think> 标签。
 */

/** 去除 <think>...</think> 标签，返回纯回答文本 */
export function stripThinkTags(text: string): string {
  return text.replace(/<think>[\s\S]*?<\/think>/gi, '').trim();
}

/** 行内格式化：粗体 + 行内代码 */
function renderInline(text: string, keyPrefix: string): React.ReactNode[] {
  const nodes: React.ReactNode[] = [];
  // 匹配 **bold** 或 `code`
  const regex = /(\*\*[^*]+\*\*|`[^`]+`)/g;
  let lastIndex = 0;
  let match: RegExpExecArray | null;
  let i = 0;

  while ((match = regex.exec(text)) !== null) {
    if (match.index > lastIndex) {
      nodes.push(text.slice(lastIndex, match.index));
    }
    const token = match[0];
    if (token.startsWith('**')) {
      nodes.push(
        <strong key={`${keyPrefix}-b-${i}`} style={{ fontWeight: 700, color: 'var(--accent-cyan-bright)' }}>
          {token.slice(2, -2)}
        </strong>
      );
    } else if (token.startsWith('`')) {
      nodes.push(
        <code key={`${keyPrefix}-c-${i}`} style={{
          background: 'rgba(62,207,174,0.08)',
          padding: '1px 5px',
          borderRadius: 4,
          fontSize: '0.9em',
          fontFamily: 'var(--font-mono)',
          color: 'var(--accent-cyan-bright)',
        }}>
          {token.slice(1, -1)}
        </code>
      );
    }
    lastIndex = regex.lastIndex;
    i++;
  }
  if (lastIndex < text.length) {
    nodes.push(text.slice(lastIndex));
  }
  return nodes;
}

export function MarkdownText({ content }: { content: string }): React.ReactElement {
  const cleaned = stripThinkTags(content);
  const lines = cleaned.split('\n');
  const blocks: React.ReactNode[] = [];
  let listItems: React.ReactNode[] = [];
  let listType: 'ul' | 'ol' | null = null;
  let keyCounter = 0;

  const flushList = () => {
    if (listItems.length > 0 && listType) {
      const Tag = listType;
      blocks.push(
        <Tag key={`list-${keyCounter++}`} style={{
          margin: '6px 0',
          paddingLeft: 20,
          lineHeight: 1.7,
          fontSize: 14,
        }}>
          {listItems}
        </Tag>
      );
      listItems = [];
      listType = null;
    }
  };

  for (const rawLine of lines) {
    const line = rawLine.trimEnd();

    // 空行
    if (line.trim() === '') {
      flushList();
      continue;
    }

    // 分隔线 ---
    if (/^---+$/.test(line.trim())) {
      flushList();
      blocks.push(<hr key={`hr-${keyCounter++}`} style={{ border: 'none', borderTop: '1px solid rgba(62,207,174,0.1)', margin: '12px 0' }} />);
      continue;
    }

    // 标题
    const headingMatch = line.match(/^(#{1,6})\s+(.*)/);
    if (headingMatch) {
      flushList();
      const level = headingMatch[1].length;
      const text = headingMatch[2];
      const fontSize = level === 1 ? 18 : level === 2 ? 16 : level === 3 ? 15 : 14;
      blocks.push(
        <div key={`h-${keyCounter++}`} style={{
          fontSize,
          fontWeight: 700,
          color: 'var(--text-primary)',
          margin: '10px 0 4px',
          fontFamily: 'var(--font-heading)',
        }}>
          {renderInline(text, `h-${keyCounter}`)}
        </div>
      );
      continue;
    }

    // 无序列表
    const ulMatch = line.match(/^[-*]\s+(.*)/);
    if (ulMatch) {
      if (listType && listType !== 'ul') flushList();
      listType = 'ul';
      listItems.push(
        <li key={`li-${keyCounter++}`}>{renderInline(ulMatch[1], `li-${keyCounter}`)}</li>
      );
      continue;
    }

    // 有序列表
    const olMatch = line.match(/^\d+\.\s+(.*)/);
    if (olMatch) {
      if (listType && listType !== 'ol') flushList();
      listType = 'ol';
      listItems.push(
        <li key={`li-${keyCounter++}`} value={parseInt(line)}>{renderInline(olMatch[1], `li-${keyCounter}`)}</li>
      );
      continue;
    }

    // 普通段落
    flushList();
    blocks.push(
      <p key={`p-${keyCounter++}`} style={{ margin: '4px 0', lineHeight: 1.7, fontSize: 14 }}>
        {renderInline(line, `p-${keyCounter}`)}
      </p>
    );
  }
  flushList();

  return <div>{blocks}</div>;
}
