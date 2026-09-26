import { useState, useEffect } from 'react';

/** 窄屏检测: 观测舱的绝对定位四角 HUD 在 <768px 会互相碰撞, 需堆叠分支 */
export function useIsMobile(breakpoint = 767): boolean {
  const [m, setM] = useState(() => typeof window !== 'undefined' && window.matchMedia(`(max-width: ${breakpoint}px)`).matches);
  useEffect(() => {
    const mq = window.matchMedia(`(max-width: ${breakpoint}px)`);
    const on = () => setM(mq.matches);
    on();
    mq.addEventListener('change', on);
    return () => mq.removeEventListener('change', on);
  }, [breakpoint]);
  return m;
}
