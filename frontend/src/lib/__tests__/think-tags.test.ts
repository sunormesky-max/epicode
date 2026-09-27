import { describe, it, expect } from 'vitest';
import { stripThinkTags } from '../think-tags';

describe('stripThinkTags(拆出的纯函数)', () => {
  it('剥除单段think', () => {
    expect(stripThinkTags('<think>推理过程</think>答案')).toBe('答案');
  });
  it('剥除多段think', () => {
    expect(stripThinkTags('<think>a</think>中段<think>b</think>尾')).toBe('中段尾');
  });
  it('大小写不敏感', () => {
    expect(stripThinkTags('<THINK>x</THINK>y')).toBe('y');
  });
  it('多行think(跨行匹配)', () => {
    expect(stripThinkTags('<think>第一行\n第二行</think>ok')).toBe('ok');
  });
  it('无think原样返回并trim', () => {
    expect(stripThinkTags('  纯文本  ')).toBe('纯文本');
  });
  it('空串安全', () => expect(stripThinkTags('')).toBe(''));
});
