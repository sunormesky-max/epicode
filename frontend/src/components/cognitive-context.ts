import { createContext } from 'react';

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
export const DEFAULT_STATE: CognitiveState = {
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

export const CognitiveContext = createContext<CognitiveState>(DEFAULT_STATE);

