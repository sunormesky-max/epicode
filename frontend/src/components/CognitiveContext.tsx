import { useState, useEffect, type ReactNode } from 'react';
import { CognitiveContext, DEFAULT_STATE } from './cognitive-context';
import type { CognitiveState } from './cognitive-context';

// 智能化突破：全局认知状态 Context
// NeuralNetworkBackground 的 SSE 数据通过 window 事件写入这里
// 所有 Dashboard 页面可读取认知引擎的实时状态



export function CognitiveProvider({ children }: { children: ReactNode }) {
  // 惰性初始化: 直接读window已有状态(原effect里同步setState, react-hooks v6)
  const [state, setState] = useState<CognitiveState>(() => {
    const existing = (window as unknown as { __cognitiveState?: CognitiveState }).__cognitiveState;
    return existing ?? DEFAULT_STATE;
  });

  useEffect(() => {
    // 监听 NeuralNetworkBackground SSE 的认知更新事件
    const handler = (e: Event) => {
      const detail = (e as CustomEvent<CognitiveState>).detail;
      if (detail) setState(detail);
    };
    window.addEventListener('cognitive-update', handler);

    return () => window.removeEventListener('cognitive-update', handler);
  }, []);

  return <CognitiveContext.Provider value={state}>{children}</CognitiveContext.Provider>;
}
