import { useContext } from 'react';
import { CognitiveContext } from './cognitive-context';

// 拆自 CognitiveContext.tsx(react-refresh only-export-components)
export function useCognitiveState() {
  return useContext(CognitiveContext);
}
