import { describe, it, expect } from 'vitest';
import { I18nContext } from '../../i18n/i18n-context-object';
import { useI18nContext } from '../../i18n/useI18n';
import { CognitiveContext } from '../../components/cognitive-context';
import { useCognitiveState } from '../../components/useCognitiveState';

// 第三批拆文件的契约测试: hook绑定的context对象 === 对外导出的context对象
// (若各文件独立createContext会导致Provider注入的值消费者读不到 — 静默断连)
describe('拆分文件契约(第三批react-refresh拆分)', () => {
  it('useI18n绑定的就是导出的I18nContext', async () => {
    // hooks内部实现无法直接断言绑定对象 — 通过源码静态验证
    const src = (await import('fs')).readFileSync('src/i18n/useI18n.ts', 'utf-8');
    expect(src).toContain("import { I18nContext } from './i18n-context-object'");
    expect(src).not.toContain("createContext"); // 不得私建context
  });
  it('useCognitiveState绑定的就是cognitive-context.ts导出', async () => {
    const src = (await import('fs')).readFileSync('src/components/useCognitiveState.ts', 'utf-8');
    expect(src).toContain("from './cognitive-context'");
    expect(src).not.toContain('createContext');
  });
  it('context对象可导入且非空', () => {
    expect(I18nContext).toBeDefined();
    expect(CognitiveContext).toBeDefined();
  });
  it('hooks可导入', () => {
    expect(typeof useI18nContext).toBe('function');
    expect(typeof useCognitiveState).toBe('function');
  });
});
