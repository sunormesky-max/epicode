# Epicode Benchmarks — Full Evidence (static mirror)

> Mirror of https://epicode.cn/#/benchmarks for fetch-only visitors. All numbers measured 2026-08-19/20 on sandbox space (9754 memories), 2 vCPU / 4GB RAM. Metric: hit@10 loose = answer text appears within top-10 retrieved memories. This is retrieval evidence coverage, NOT LLM-judged answer accuracy — not directly comparable to vendor-published LME scores.

<!-- AUTO:BENCH -->
## LongMemEval-S oracle, 500 questions (overall, auto-generated 2026-09-26)
| mode | score |
|---|---|
| hybrid | 56.4% |
| semantic | 62% |
| graph+PPR | 65.6% |

## By question type (n / hybrid / semantic / ppr)
- knowledge-update: 78 / 76.9 / 91 / 82.1
- single-session-assistant: 56 / 60.7 / 89.3 / 82.1
- single-session-user: 70 / 71.4 / 85.7 / 90
- temporal-reasoning: 133 / 52.6 / 49.6 / 57.9
- multi-session: 133 / 51.1 / 47.4 / 57.9
- single-session-preference: 30 / 0 / 0 / 3.3

## Search latency (P50 ms / avg results)
- exact: 22.9 / 5.6
- semantic: 7.5 / 10
- graph: 32.8 / 10
- hybrid: 55.1 / 10

## Knowledge-graph health
- 密度评分: 100 / 100
- 孤儿率: 0.0%
- 平均关系数/记忆: 49.4
- 平均关系强度: 0.835
- 平均簇大小: 21.8
- 图谱导出(800节点): 50ms

## SMRP operation latency (ms; fresh loopback vs old public)
- space_stats: 4 (was 81)
- memory_search: 52 (was 144)
- memory_recall: 36 (n/a)
- memory_get: 5 (was 80)
- knowledge_relations: 5 (was 79)
- memory_create: 216 (was 282)
<!-- /AUTO:BENCH -->

## Negative results (published deliberately)
- Rule-based mode router: tested, REJECTED. auto mode scored 55.0% — worse than every fixed mode. auto currently delegates to hybrid.
- Preference-type retrieval near zero (see above).

Live latency probe (requires JS): https://epicode.cn/#/benchmarks
