import { createContext, useContext, useState, useEffect, type ReactNode } from 'react';

// 智能化突破：全局认知状态 Context
// NeuralNetworkBackground 的 SSE 数据通过 window 事件写入这里
// 所有 Dashboard 页面可读取认知引擎的实时状态

export interface EmotionState {
  pleasure: number;
  arousal: number;
  dominance: number;
  quadrant?: string;
  label?: string;
}

export interface CognitiveState {
  energy: number;
  memories: number;
  clusters: number;
  cognitiveStatus: string;
  emotion: EmotionState | null;
  drive: { dominant?: string } | null;
  decisionCount: number;
  latestThought: string;
  learning: { pattern?: string; watch_for?: string; calibration?: string } | null;
  lastReflection: { observation: string; insight: string } | null;
  timestamp: number;
}

const DEFAULT_STATE: CognitiveState = {
  energy: 0,
  memories: 0,
  clusters: 0,
  cognitiveStatus: 'unknown',
  emotion: null,
  drive: null,
  decisionCount: 0,
  latestThought: '',
  learning: null,
  lastReflection: null,
  timestamp: 0,
};

const CognitiveContext = createContext<CognitiveState>(DEFAULT_STATE);

export function CognitiveProvider({ children }: { children: ReactNode }) {
  const [state, setState] = useState<CognitiveState>(DEFAULT_STATE);

  useEffect(() => {
    // 监听 NeuralNetworkBackground SSE 的认知更新事件
    const handler = (e: Event) => {
      const detail = (e as CustomEvent<CognitiveState>).detail;
      if (detail) setState(detail);
    };
    window.addEventListener('cognitive-update', handler);

    // 也读取 window 上可能已有的初始状态
    const existing = (window as unknown as { __cognitiveState?: CognitiveState }).__cognitiveState;
    if (existing) setState(existing);

    return () => window.removeEventListener('cognitive-update', handler);
  }, []);

  return <CognitiveContext.Provider value={state}>{children}</CognitiveContext.Provider>;
}

export function useCognitiveState() {
  return useContext(CognitiveContext);
}
