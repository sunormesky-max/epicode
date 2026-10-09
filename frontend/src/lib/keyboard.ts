// 轻量键盘工具:被官网 Navbar(首屏 Layout 块)引用,不能依赖任何重数据模块
/** "/" 快捷键只在用户没有在输入时触发 */
export function isTypingTarget(el: { tagName?: string; isContentEditable?: boolean } | null | undefined): boolean {
  if (!el) return false;
  const tag = (el.tagName || '').toUpperCase();
  return tag === 'INPUT' || tag === 'TEXTAREA' || tag === 'SELECT' || el.isContentEditable === true;
}
