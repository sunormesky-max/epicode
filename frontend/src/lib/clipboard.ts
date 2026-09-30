/**
 * 剪贴板写入 — 带兜底与成败返回。
 * navigator.clipboard.writeText 在微信内置浏览器(XWeb)/部分 WebView/
 * 非安全上下文/权限策略拒绝下会抛错或不存在; 回退到隐藏 textarea +
 * document.execCommand('copy')(同步老 API, 依赖用户手势, 本工具均在
 * 点击处理器内调用, 满足手势要求)。
 */
export async function copyText(text: string): Promise<boolean> {
  try {
    if (typeof navigator !== 'undefined' && navigator.clipboard && window.isSecureContext) {
      await navigator.clipboard.writeText(text);
      return true;
    }
  } catch {
    // 权限拒绝或不支持 → 落入兜底
  }
  try {
    const ta = document.createElement('textarea');
    ta.value = text;
    ta.setAttribute('readonly', '');
    ta.style.position = 'fixed';
    ta.style.top = '-9999px';
    ta.style.opacity = '0';
    document.body.appendChild(ta);
    const prev = document.activeElement instanceof HTMLElement ? document.activeElement : null;
    ta.focus();
    ta.select();
    ta.setSelectionRange(0, text.length);
    const ok = document.execCommand('copy');
    ta.remove();
    prev?.focus();
    return ok;
  } catch {
    return false;
  }
}
