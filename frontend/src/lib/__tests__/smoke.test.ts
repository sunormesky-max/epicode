import { describe, it, expect } from 'vitest';

// 生产前端树冒烟(覆盖开源repo时引入): 保证 vitest 有可跑用例,
// 并锁定构建元数据不被误改
describe('production frontend smoke', () => {
  it('package metadata', async () => {
    const pkg = await import('../../package.json', { with: { type: 'json' } });
    expect(pkg.default.name).toBe('epicode-frontend');
    expect(pkg.default.version).toMatch(/^\d+\.\d+\.\d+$/);
  });
});
