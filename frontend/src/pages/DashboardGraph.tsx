import { useEffect, useRef, useState, useCallback } from 'react';
import DashboardLayout from '@/components/DashboardLayout';
import { DashboardLoading } from '@/components/DashboardUI';
import { errMsg, getGraphExport, getGraphAnalysis, getNodeRelations, getKgQuality } from '@/lib/api';
import type { KgQuality } from '@/lib/api';
import { Search, ZoomIn, ZoomOut, RotateCcw, X, GitBranch, Tag, Activity, ChevronDown, ChevronUp, Route, Navigation, HeartPulse, Target } from 'lucide-react';
import { useI18nContext } from '@/i18n/useI18n';
import type { TranslationKey } from '@/i18n/translations';

const CLUSTER_COLORS = [
  // 青蓝能量谱（主）→ 紫罗兰 → 金红（辅），吞噬星空双能量配色
  '#3ecfae', '#3ecfae', '#3ecfae', '#7ba3ff', '#3ecfae',
  '#8b7ec8', '#8b7ec8', '#ec4899', '#8b7ec8', '#3ecfae',
  '#3ecfae', '#22d3ee', '#818cf8', '#e879f9', '#facc15',
];
const EDGE_COLORS: Record<string, string> = {
  similar: '#3ecfae', related: '#3ecfae', contradicts: '#ff3860',
  precedes: '#3ecfae', contains: '#3ecfae',
};
const EDGE_LABEL_KEYS: Record<string, string> = {
  similar: 'dash.graph.edge.similar',
  related: 'dash.graph.edge.related',
  contradicts: 'dash.graph.edge.contradicts',
  precedes: 'dash.graph.edge.precedes',
  contains: 'dash.graph.edge.contains',
};

interface SNode {
  id: number; idx: number;
  x: number; y: number; vx: number; vy: number;
  mass: number; labels: string[]; content: string;
  cluster: number; timestamp: number;
}
interface SEdge { s: number; t: number; type: string; strength: number; }
interface ClusterInfo { size: number; top_labels: { label: string; count: number }[]; }
interface HoverInfo { x: number; y: number; node: SNode; }

export default function DashboardGraph() {
  const { t } = useI18nContext();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [graphMeta, setGraphMeta] = useState<{ truncated: boolean; total: number }>({ truncated: false, total: 0 });
  const [searchQ, setSearchQ] = useState('');
  const [, setZoom] = useState(1);
  const [, setOffset] = useState({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const [stats, setStats] = useState({ nodes: 0, edges: 0, clusters: 0, interCluster: 0 });
  const [selectedCluster, setSelectedCluster] = useState<number | null>(null);
  const [clusterInfo, setClusterInfo] = useState<ClusterInfo[]>([]);
  const [selectedNode, setSelectedNode] = useState<SNode | null>(null);
  const [nodeRelations, setNodeRelations] = useState(0);
  const [hover, setHover] = useState<HoverInfo | null>(null);
  const [edgeTypeCounts, setEdgeTypeCounts] = useState<Record<string, number>>({});
  const [topConcepts, setTopConcepts] = useState<{ label: string; member_count: number }[]>([]);
  const [dataReady, setDataReady] = useState(false);
  const [statsPanelOpen, setStatsPanelOpen] = useState(false);
  // ── 功能性能力 state ──
  const [clusterMembers, setClusterMembers] = useState<{ id: number; content: string; labels: string[] }[] | null>(null);
  const [pathMode, setPathMode] = useState(false); // 路径查找模式
  const [pathResult, setPathResult] = useState<number[] | null>(null); // 路径节点 idx 数组
  const [, setPathStart] = useState<number | null>(null); // 路径起点 idx
  const [pathNotFound, setPathNotFound] = useState(false); // 路径未找到提示
  const [nodeRelationsDetail, setNodeRelationsDetail] = useState<{
    typeDist: Record<string, number>;
    strongest: { target: number; type: string; strength: number }[];
    degree: number;
  } | null>(null);
  const pathModeRef = useRef(false);
  const pathStartRef = useRef<number | null>(null);
  const pathResultRef = useRef<number[] | null>(null);
  const clusterMemberIdsRef = useRef<Set<number> | null>(null); // 当前高亮的聚类成员 idx 集合
  // clusters 数据缓存（含 member_ids）
  const clustersDataRef = useRef<{ member_ids: number[]; top_labels: unknown[] }[]>([]);
  // 图谱健康评估（能力4）
  const [kgQuality, setKgQuality] = useState<KgQuality | null>(null);
  const [kgLoading, setKgLoading] = useState(false);

  const dragRef = useRef({ x: 0, y: 0 });
  const nodesRef = useRef<SNode[]>([]);
  const edgesRef = useRef<SEdge[]>([]);
  const interEdgesRef = useRef<SEdge[]>([]);
  const zoomRef = useRef(1);
  const offsetRef = useRef({ x: 0, y: 0 });
  const searchQRef = useRef('');
  const selectedClusterRef = useRef<number | null>(null);
  const hoverRef = useRef<HoverInfo | null>(null);
  const selectedNodeRef = useRef<SNode | null>(null);
  const drawRef = useRef<(() => void) | null>(null);
  const frameRef = useRef(0);
  const rafRef = useRef(0);
  const dimsRef = useRef({ w: 1200, h: 720 }); // 自适应容器尺寸
  const visRef = useRef<{ sq: string; set: Set<number> | null }>({ sq: '', set: null }); // 缓存搜索过滤结果

  useEffect(() => {
    let mounted = true;
    async function load() {
      try {
        const [data, analysis] = await Promise.all([getGraphExport(), getGraphAnalysis()]);
        if (!mounted) return;
        setGraphMeta({ truncated: !!data.truncated, total: data.total_nodes || (data.nodes || []).length });
        const cMap = new Map<number, number>();
        (data.clusters || []).forEach((c: { member_ids: number[] }, ci: number) => {
          (c.member_ids || []).forEach(id => cMap.set(id, ci));
        });
        const idToIdx = new Map<number, number>();
        // 坐标归一化：后端 core_x/y/z 范围极大(-141~1200, z 0~96)且分布不均(z=0 堆积半数节点)。
        // 直接线性映射会导致节点飞出画布或挤成一团。改用 cluster 分层 + 画布内极坐标散布：
        // - Y 轴(垂直层)：按 cluster 映射到 6 层圆柱语义层(每层多 cluster 共享层带)
        // - X 轴：cluster 中心按层内均匀分布 + 节点在 cluster 附近散布
        // 力导向只需微调即可收敛成美观的簇团。
        const W0 = 1200, H0 = 600;
        const numClusters = (data.clusters || []).length || 1;
        // 每个 cluster 分配一个中心点(极坐标：按 cluster index 均匀分布到画布)
        const clusterCenters = new Map<number, { x: number; y: number }>();
        (data.clusters || []).forEach((_c: { member_ids: number[] }, ci: number) => {
          // 6 层分层：cluster index 决定层(0-5)
          const layer = ci % 6;
          const layerH = (H0 - 80) / 6;
          const cy_center = 40 + layer * layerH + layerH / 2;
          // 同层内多 cluster 横向均匀分布
          const clustersInLayer = Math.ceil(numClusters / 6);
          const posInLayer = Math.floor(ci / 6);
          const cx_center = W0 * (0.15 + 0.7 * (posInLayer + 0.5) / clustersInLayer);
          clusterCenters.set(ci, { x: cx_center, y: cy_center });
        });

        const ns: SNode[] = (data.nodes || []).map((rn, i) => {
          idToIdx.set(rn.id, i);
          const clusterIdx = cMap.get(rn.id) ?? -1;
          const center = clusterIdx >= 0 ? clusterCenters.get(clusterIdx) : null;
          let px: number, py: number;
          if (center) {
            // 在 cluster 中心附近散布(半径随 cluster size 缩减)
            const angle = (rn.id * 2.399) % (Math.PI * 2);  // 黄金角，均匀散布
            const radius = 20 + (rn.id % 7) * 8;
            px = center.x + Math.cos(angle) * radius;
            py = center.y + Math.sin(angle) * radius;
          } else {
            // 无 cluster 的孤立节点：随机散布在画布边缘
            px = W0 * 0.5 + (Math.sin(rn.id * 1.7) * W0 * 0.4);
            py = H0 * 0.5 + (Math.cos(rn.id * 2.1) * H0 * 0.4);
          }
          return {
            id: rn.id, idx: i,
            x: px,
            y: py,
            vx: 0, vy: 0,
            mass: rn.mass || 1,
            labels: rn.labels || [], content: rn.content || '',
            cluster: clusterIdx, timestamp: rn.timestamp || 0,
          };
        });
        const es: SEdge[] = [];
        const tc: Record<string, number> = {};
        for (const re of (data.edges || [])) {
          const si = idToIdx.get(re.source), ti = idToIdx.get(re.target);
          if (si !== undefined && ti !== undefined) {
            const rt = (re.relation_type || 'related').toLowerCase();
            es.push({ s: si, t: ti, type: rt, strength: re.strength });
            tc[rt] = (tc[rt] || 0) + 1;
          }
        }
        const ies: SEdge[] = [];
        for (const re of (data.inter_cluster_edges || [])) {
          const si = idToIdx.get(re.source), ti = idToIdx.get(re.target);
          if (si !== undefined && ti !== undefined)
            ies.push({ s: si, t: ti, type: (re.relation_type || 'related').toLowerCase(), strength: re.strength });
        }
        nodesRef.current = ns; edgesRef.current = es; interEdgesRef.current = ies;
        setStats({ nodes: ns.length, edges: es.length, clusters: (data.clusters || []).length, interCluster: ies.length });
        setClusterInfo(analysis?.cluster_analysis || []);
        setEdgeTypeCounts(tc);
        setTopConcepts((data.concepts || []).slice(0, 12));
        // 缓存 clusters 的 member_ids（能力2：聚类下钻看成员）
        clustersDataRef.current = (data.clusters || []) as { member_ids: number[]; top_labels: unknown[] }[];
        setDataReady(true);
      } catch (e: unknown) { if (mounted) setError(errMsg(e)); }
      if (mounted) setLoading(false);
    }
    load();
    return () => { mounted = false; };
  }, []);

  useEffect(() => {
    const canvas = canvasRef.current;
    if (!canvas || !nodesRef.current.length) return;
    const ctx0 = canvas.getContext('2d');
    if (!ctx0) return;
    // 显式非空注解: 闭包(draw/simulate内嵌函数)不继承收窄, 119处 ctx possibly-null 根治
    const ctx: CanvasRenderingContext2D = ctx0;
    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    const parent = canvas.parentElement;
    let W: number, H: number;
    if (parent) {
      const rect = parent.getBoundingClientRect();
      W = Math.max(400, Math.round(rect.width));
      H = Math.max(400, Math.round(rect.height));
    } else {
      W = 1200; H = 600;
    }
    dimsRef.current = { w: W, h: H };
    // 关键修复：backing store 设为 W*dpr × H*dpr，CSS 保持 100%（由 React 管理）。
    // 之前用 canvas.style.width = W+'px' 会被 React 每次重渲染覆盖回 100%，
    // 而 backing store 与 CSS 尺寸不一致 → canvas 被浏览器拉伸 → 看起来"被压成长方形"。
    // 现在 backing store 像素值 = CSS 像素值 × dpr，CSS 撑满容器，两者比例一致，无拉伸。
    canvas.width = W * dpr;
    canvas.height = H * dpr;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    frameRef.current = 0;
    const maxFrames = 250;

    function simulate() {
      const ns = nodesRef.current; const es = edgesRef.current;
      if (!ns.length) return;
      const settled = frameRef.current >= maxFrames;
      const { w: W, h: H } = dimsRef.current;
      // 圆形力场参数每帧重算（随容器尺寸自适应）
      const fieldR = Math.min(W, H) * 0.46;
      const cx = W / 2, cy = H / 2;

      // 性能优化：收敛后跳过 O(n²) 斥力计算，仅保留呼吸/阻尼效果
      if (!settled) {
        for (let i = 0; i < ns.length; i++) {
          for (let j = i + 1; j < ns.length; j++) {
            const dx = ns[j].x - ns[i].x, dy = ns[j].y - ns[i].y;
            const d2 = dx * dx + dy * dy; if (d2 < 1) continue;
            const d = Math.sqrt(d2);
            const same = ns[i].cluster >= 0 && ns[i].cluster === ns[j].cluster;
            const rep = same ? 60 / d2 : 180 / d2;
            const f = Math.min(rep, same ? 0.25 : 0.7);
            const fx = (dx / d) * f, fy = (dy / d) * f;
            ns[i].vx -= fx; ns[i].vy -= fy; ns[j].vx += fx; ns[j].vy += fy;
          }
        }
      }
      for (const e of es) {
        const a = ns[e.s], b = ns[e.t]; if (!a || !b) continue;
        const dx = b.x - a.x, dy = b.y - a.y;
        const d = Math.sqrt(dx * dx + dy * dy) || 1;
        const target = a.cluster === b.cluster && a.cluster >= 0 ? 35 : 75;
        const f = (d - target) * 0.0005;
        a.vx += (dx / d) * f; a.vy += (dy / d) * f;
        b.vx -= (dx / d) * f; b.vy -= (dy / d) * f;
      }
      const centers = new Map<number, { x: number; y: number; n: number }>();
      for (const n of ns) {
        if (n.cluster < 0) continue;
        const c = centers.get(n.cluster) || { x: 0, y: 0, n: 0 };
        c.x += n.x; c.y += n.y; c.n++; centers.set(n.cluster, c);
      }
      // 收敛后加微振荡（呼吸感），让神经网络"活着"
      const breathe = settled ? 0.0003 : 0.0008;
      const damping = settled ? 0.95 : 0.82;
      for (const n of ns) {
        const tgt = centers.get(n.cluster);
        if (tgt) { n.vx += (tgt.x / tgt.n - n.x) * breathe; n.vy += (tgt.y / tgt.n - n.y) * breathe; }
        else { n.vx += (cx - n.x) * 0.0004; n.vy += (cy - n.y) * 0.0004; }
        // 收敛后加随机微扰（布朗运动，模拟神经活动）
        if (settled) { n.vx += (Math.random() - 0.5) * 0.01; n.vy += (Math.random() - 0.5) * 0.01; }
        n.vx *= damping; n.vy *= damping; n.x += n.vx; n.y += n.vy;
        // 关键修复：用圆形力场代替矩形 clamp，避免网络被矩形边框"压成长方形"。
        // 节点离中心超过 fieldR 时，施加向心推力（弹性边界），形成自然的圆形/有机团块。
        const ddx = n.x - cx, ddy = n.y - cy;
        const distC = Math.sqrt(ddx * ddx + ddy * ddy);
        if (distC > fieldR) {
          const k = (distC - fieldR) * 0.18; // 向心弹性
          n.vx -= (ddx / distC) * k;
          n.vy -= (ddy / distC) * k;
        }
      }
      frameRef.current++;
    }

    function draw() {
      const { w: W, h: H } = dimsRef.current;
      // 完全透明 clearRect — 让底层 SacredBackground（神经网络+星尘+扫描线）直接透出
      ctx.clearRect(0, 0, W, H);

      // 六层圆柱体分层线(语义层级可视化) — 极低透明度，不遮挡活体背景
      const layerNames = ['Identity', 'Cycle', 'Service', 'Cognitive', 'Relation', 'Instinct'];
      const layerColors = ["rgba(62,207,174,0.02)", "rgba(62,207,174,0.017)", "rgba(90,154,140,0.016)", "rgba(139,126,200,0.015)", "rgba(139,126,200,0.013)", "rgba(230,200,120,0.012)"];
      const H0 = 600;
      const lh = (H0 - 80) / 6;
      ctx.font = "500 10px JetBrains Mono, monospace";
      ctx.textAlign = 'left';
      const z = zoomRef.current;
      const oy = offsetRef.current.y;
      for (let li = 0; li < 6; li++) {
        const ly = 40 + li * lh + lh / 2;
        const screenY = ly * z + oy;
        ctx.fillStyle = layerColors[li];
        ctx.fillRect(0, screenY - lh * z / 2, W, lh * z);
        ctx.fillStyle = 'rgba(62,207,174,0.18)';
        ctx.fillText(layerNames[li], 8, screenY);
      }

      const ns = nodesRef.current; const es = edgesRef.current; const ies = interEdgesRef.current;
      const o = offsetRef.current;
      const sq = searchQRef.current.toLowerCase(); const sc = selectedClusterRef.current;
      const hv = hoverRef.current; const sel = selectedNodeRef.current;
      const t = Date.now() * 0.001; // 用于流动动画
      ctx.save(); ctx.translate(o.x, o.y); ctx.scale(z, z);
      // 缓存 vis Set：仅在搜索词变化时重建（避免每帧 O(n) 过滤）
      let vis: Set<number> | null;
      if (sq) {
        if (visRef.current.sq !== sq) {
          visRef.current = { sq, set: new Set(ns.filter(n => n.content.toLowerCase().includes(sq) || n.labels.some(l => l.toLowerCase().includes(sq))).map(n => n.idx)) };
        }
        vis = visRef.current.set;
      } else {
        visRef.current = { sq: '', set: null };
        vis = null;
      }
      const hvNode = hv?.node;

      // ── 焦点高亮集合计算（邻居/聚类/路径三种模式）──
      // 模式优先级：pathResult > selectedNode 邻居 > clusterMemberIds > 搜索过滤 vis
      let highlightSet: Set<number> | null = null;
      let pathEdges: Set<string> | null = null;
      const curPath = pathResultRef.current;
      const curPathStart = pathStartRef.current;
      if (curPath && curPath.length > 1) {
        // 路径模式：高亮路径上的节点和边
        highlightSet = new Set(curPath);
        pathEdges = new Set<string>();
        for (let i = 0; i < curPath.length - 1; i++) {
          pathEdges.add(`${Math.min(curPath[i], curPath[i+1])}-${Math.max(curPath[i], curPath[i+1])}`);
        }
      } else if (sel) {
        // 邻居模式：选中节点 + 直接邻居
        highlightSet = new Set<number>([sel.idx]);
        for (const e of es) {
          if (e.s === sel.idx) highlightSet.add(e.t);
          if (e.t === sel.idx) highlightSet.add(e.s);
        }
      } else if (clusterMemberIdsRef.current && clusterMemberIdsRef.current.size > 0) {
        // 聚类模式
        highlightSet = clusterMemberIdsRef.current;
      }
      const hasFocus = highlightSet !== null;

      // 跨簇边（暗，流动效果）
      for (const e of ies) {
        const a = ns[e.s], b = ns[e.t]; if (!a || !b) continue;
        if (vis && !vis.has(e.s) && !vis.has(e.t)) continue;
        // 焦点模式下，跨簇边非高亮的全暗
        if (hasFocus && !highlightSet!.has(e.s) && !highlightSet!.has(e.t)) continue;
        const ec = EDGE_COLORS[e.type] || '#3ecfae';
        // 渐变边
        const grad = ctx.createLinearGradient(a.x, a.y, b.x, b.y);
        grad.addColorStop(0, ec + '15');
        grad.addColorStop(0.5, ec + '30');
        grad.addColorStop(1, ec + '15');
        ctx.globalAlpha = 0.15;
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y);
        ctx.strokeStyle = grad; ctx.lineWidth = 1.5; ctx.stroke();
      }
      // 簇内边（亮，流动粒子 — 突触放电系统）
      for (const e of es) {
        const a = ns[e.s], b = ns[e.t]; if (!a || !b) continue;
        if (vis && !vis.has(e.s) && !vis.has(e.t)) continue;
        if (sc !== null && a.cluster !== sc && b.cluster !== sc) continue;
        const isHv = hvNode && (hvNode.idx === e.s || hvNode.idx === e.t);
        const isSelEdge = sel && (sel.idx === e.s || sel.idx === e.t);
        const ec = EDGE_COLORS[e.type] || '#3ecfae';
        // 焦点模式 dim 逻辑
        const edgeKey = `${Math.min(e.s, e.t)}-${Math.max(e.s, e.t)}`;
        const isPathEdge = pathEdges && pathEdges.has(edgeKey);
        if (hasFocus && !isPathEdge && !highlightSet!.has(e.s)) {
          // 非高亮边在焦点模式下极暗
          ctx.globalAlpha = 0.03;
        } else if (isPathEdge) {
          // 路径边：金色高亮
          ctx.globalAlpha = 0.9;
          ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y);
          ctx.strokeStyle = "#e6c878";
          ctx.lineWidth = 2.5;
          ctx.shadowColor = '#FFD700'; ctx.shadowBlur = 8;
          ctx.stroke();
          ctx.shadowBlur = 0;
          // 路径流动粒子
          const pp = (t * 0.8 + e.s * 0.3) % 1;
          const px = a.x + (b.x - a.x) * pp, py = a.y + (b.y - a.y) * pp;
          ctx.globalAlpha = 1;
          ctx.beginPath(); ctx.arc(px, py, 3, 0, Math.PI * 2);
          ctx.fillStyle = '#fff'; ctx.fill();
          continue;
        } else if (hasFocus) {
          ctx.globalAlpha = isHv ? 0.7 : 0.25;
        } else {
          ctx.globalAlpha = isHv ? 0.6 : 0.12;
        }
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y);
        ctx.strokeStyle = ec;
        ctx.lineWidth = isHv || isSelEdge ? 1.5 : 0.5;
        ctx.stroke();

        // ── 突触能量粒子流 ──
        // 稳定种子（每条边独立相位，不随帧跳变）
        const seed = ((e.s * 73856093) ^ (e.t * 19349663)) >>> 0;
        const phase = (seed % 1000) / 1000;
        const aIsKey = a.mass >= 8;
        const bIsKey = b.mass >= 8;

        if (isHv || isSelEdge) {
          // hover/selected 边：密集能量束（3 个粒子，相位差 1/3）
          for (let k = 0; k < 3; k++) {
            const fp = ((t * 0.6 + k / 3 + phase) % 1);
            const px = a.x + (b.x - a.x) * fp;
            const py = a.y + (b.y - a.y) * fp;
            const fade = Math.sin(fp * Math.PI); // 中间最亮
            ctx.globalAlpha = 0.9 * fade;
            // 外层光晕
            ctx.beginPath(); ctx.arc(px, py, 3, 0, Math.PI * 2);
            ctx.fillStyle = ec + '60'; ctx.fill();
            // 核心亮点
            ctx.beginPath(); ctx.arc(px, py, 1.4, 0, Math.PI * 2);
            ctx.fillStyle = '#ffffff'; ctx.fill();
          }
        } else {
          // 普通边：低密度随机放电（~5% 时间活跃，营造背景神经活动）
          const cycle = (t * 0.35 + phase) % 1;
          const discharging = cycle < 0.05; // 5% 占空比
          if (discharging || aIsKey || bIsKey) {
            // 关键节点边持续低强度流动；普通边周期性放电
            const intensity = (aIsKey || bIsKey) ? 0.5 : (1 - cycle / 0.05);
            const fp = cycle / (aIsKey || bIsKey ? 1 : 0.05); // 放电期内从 0→1
            if (fp <= 1) {
              const px = a.x + (b.x - a.x) * fp;
              const py = a.y + (b.y - a.y) * fp;
              ctx.globalAlpha = intensity * 0.7;
              ctx.beginPath(); ctx.arc(px, py, 1.8, 0, Math.PI * 2);
              ctx.fillStyle = ec + '80'; ctx.fill();
              ctx.beginPath(); ctx.arc(px, py, 0.8, 0, Math.PI * 2);
              ctx.fillStyle = '#ffffff'; ctx.fill();
            }
          }
        }
      }

      // 跨簇边：能量脉冲流（更稀疏，连接不同能量域）
      for (const e of ies) {
        const a = ns[e.s], b = ns[e.t]; if (!a || !b) continue;
        if (vis && !vis.has(e.s) && !vis.has(e.t)) continue;
        const ec = EDGE_COLORS[e.type] || '#3ecfae';
        const seed = ((e.s * 73856093) ^ (e.t * 19349663)) >>> 0;
        const phase = (seed % 1000) / 1000;
        // 跨簇边 ~3% 时间放电（长程连接，偶尔脉冲）
        const cycle = (t * 0.2 + phase) % 1;
        if (cycle < 0.03) {
          const fp = cycle / 0.03;
          const px = a.x + (b.x - a.x) * fp;
          const py = a.y + (b.y - a.y) * fp;
          ctx.globalAlpha = (1 - cycle / 0.03) * 0.8;
          ctx.beginPath(); ctx.arc(px, py, 2.2, 0, Math.PI * 2);
          ctx.fillStyle = ec; ctx.fill();
          ctx.beginPath(); ctx.arc(px, py, 1, 0, Math.PI * 2);
          ctx.fillStyle = '#ffffff'; ctx.fill();
        }
      }
      ctx.globalAlpha = 1;

      // 节点（神经元：双层 glow + 能量核 + mass 映射大小）
      for (const n of ns) {
        const dim = (vis && !vis.has(n.idx)) || (sc !== null && n.cluster !== sc);
        const color = n.cluster >= 0 ? CLUSTER_COLORS[n.cluster % CLUSTER_COLORS.length] : '#6b7280';
        const isHv = hvNode?.idx === n.idx; const isSel = sel?.idx === n.idx;
        // 路径起点/终点标记
        const isPathEndpoint = curPathStart === n.idx || (curPath && curPath[curPath.length - 1] === n.idx);
        // 半径按 mass 映射（mass 越大节点越大，体现记忆重要性）
        const baseR = 2.5 + Math.min(n.mass / 20, 4);
        const r = (isHv || isSel || isPathEndpoint) ? baseR + 3 : baseR;
        // alpha 综合：搜索/聚类过滤 dim + 焦点高亮 dim
        let alpha: number;
        if (dim) {
          alpha = 0.06;
        } else if (hasFocus) {
          // 焦点模式：高亮集合内 = 1，外面 = 0.08
          alpha = highlightSet!.has(n.idx) ? (isHv || isSel ? 1 : 0.95) : 0.08;
        } else {
          alpha = isHv ? 1 : 0.75;
        }

        // 1. 外层 glow（径向发光，增强）— 双层光晕
        if (!dim) {
          // 外层大光晕
          const glowR = r * (isHv ? 6 : 4);
          const glowGrad = ctx.createRadialGradient(n.x, n.y, 0, n.x, n.y, glowR);
          glowGrad.addColorStop(0, color + (isHv ? '60' : '30'));
          glowGrad.addColorStop(0.4, color + '12');
          glowGrad.addColorStop(1, color + '00');
          ctx.globalAlpha = alpha * 0.85;
          ctx.beginPath(); ctx.arc(n.x, n.y, glowR, 0, Math.PI * 2);
          ctx.fillStyle = glowGrad; ctx.fill();
          // 内层青白能量核晕（高亮节点）
          if (isHv || isSel) {
            const coreGlow = ctx.createRadialGradient(n.x, n.y, 0, n.x, n.y, r * 2);
            coreGlow.addColorStop(0, 'rgba(255,255,255,0.4)');
            coreGlow.addColorStop(1, 'rgba(255,255,255,0)');
            ctx.globalAlpha = alpha * 0.7;
            ctx.beginPath(); ctx.arc(n.x, n.y, r * 2, 0, Math.PI * 2);
            ctx.fillStyle = coreGlow; ctx.fill();
          }
        }

        // 2. 选中/hover 时的双层 ripple 扩散（能量波）
        if (isHv || isSel) {
          const rippleR1 = r + 5 + Math.sin(t * 3) * 3;
          const rippleR2 = r + 10 + Math.sin(t * 3 + 1.5) * 4;
          ctx.globalAlpha = 0.4;
          ctx.beginPath(); ctx.arc(n.x, n.y, rippleR1, 0, Math.PI * 2);
          ctx.strokeStyle = color + '80'; ctx.lineWidth = 1.5; ctx.stroke();
          ctx.globalAlpha = 0.2;
          ctx.beginPath(); ctx.arc(n.x, n.y, rippleR2, 0, Math.PI * 2);
          ctx.strokeStyle = color + '40'; ctx.lineWidth = 1; ctx.stroke();
        }

        // 3. 节点核心（更亮的能量球）
        ctx.globalAlpha = alpha;
        ctx.beginPath(); ctx.arc(n.x, n.y, r, 0, Math.PI * 2);
        ctx.fillStyle = color; ctx.fill();

        // 4. 节点高光（能量球 3D 感 + 青白核）
        if (!dim) {
          ctx.globalAlpha = alpha * 0.7;
          ctx.beginPath(); ctx.arc(n.x - r*0.3, n.y - r*0.3, r*0.45, 0, Math.PI * 2);
          ctx.fillStyle = 'rgba(255,255,255,0.55)'; ctx.fill();
          // 中心高亮白核
          ctx.globalAlpha = alpha * 0.9;
          ctx.beginPath(); ctx.arc(n.x, n.y, r * 0.25, 0, Math.PI * 2);
          ctx.fillStyle = 'rgba(240,250,255,0.9)'; ctx.fill();
        }

        // 5. 高 mass 节点常显标签（重要性可视化，无需 hover 即可识别关键节点）
        if (!dim && n.mass >= 5) {
          const labelText = n.labels.length > 0
            ? n.labels[0]
            : (n.content || '').slice(0, 12);
          if (labelText) {
            ctx.globalAlpha = isHv ? 1 : 0.7;
            ctx.font = `600 ${Math.min(11, 8 + n.mass / 8)}px JetBrains Mono, var(--font-mono), monospace`;
            ctx.textAlign = 'left';
            ctx.fillStyle = isHv ? '#f0f0f5' : color;
            // 标签微光
            ctx.shadowColor = color;
            ctx.shadowBlur = isHv ? 8 : 4;
            ctx.fillText(labelText, n.x + r + 4, n.y + 3);
            ctx.shadowBlur = 0;
          }
        }
      }
      ctx.globalAlpha = 1; ctx.restore();
    }

    drawRef.current = draw;
    let paused = false;
    function loop() {
      if (paused) return; // 后台 tab 时停止调度
      simulate();
      draw();
      rafRef.current = requestAnimationFrame(loop);
    }
    loop();
    // 后台 tab 时暂停 RAF（省电），回到前台恢复
    const onVis = () => {
      if (document.hidden) {
        paused = true;
        if (rafRef.current) { cancelAnimationFrame(rafRef.current); rafRef.current = 0; }
      } else if (!paused || rafRef.current === 0) {
        paused = false;
        loop();
      }
    };
    document.addEventListener('visibilitychange', onVis);
    // 容器尺寸变化时重新匹配 backing store（圆形力场半径随容器自适应）
    const ro = new ResizeObserver(() => {
      const p = canvas.parentElement; if (!p) return;
      const r = p.getBoundingClientRect();
      const nW = Math.max(400, Math.round(r.width));
      const nH = Math.max(400, Math.round(r.height));
      if (nW !== dimsRef.current.w || nH !== dimsRef.current.h) {
        dimsRef.current = { w: nW, h: nH };
        canvas.width = nW * dpr; canvas.height = nH * dpr;
        ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
      }
    });
    if (canvas.parentElement) ro.observe(canvas.parentElement);
    return () => { cancelAnimationFrame(rafRef.current); ro.disconnect(); document.removeEventListener('visibilitychange', onVis); drawRef.current = null; };
  }, [dataReady]);

  const refresh = useCallback(() => { if (drawRef.current) drawRef.current(); }, []);
  const findNodeAt = useCallback((mx: number, my: number): SNode | null => {
    let closest: SNode | null = null, closestD = Infinity;
    for (const n of nodesRef.current) {
      const d = Math.sqrt((n.x - mx) ** 2 + (n.y - my) ** 2);
      if (d < 10 && d < closestD) { closest = n; closestD = d; }
    }
    return closest;
  }, []);

  // ── BFS 路径查找（前端图算法，用已加载的 edges）──
  const findPath = useCallback((startIdx: number, endIdx: number): number[] | null => {
    const edges = edgesRef.current;
    // 构建邻接表（一次性，加速查询）
    const adj = new Map<number, number[]>();
    for (const e of edges) {
      if (!adj.has(e.s)) adj.set(e.s, []);
      if (!adj.has(e.t)) adj.set(e.t, []);
      adj.get(e.s)!.push(e.t);
      adj.get(e.t)!.push(e.s);
    }
    // BFS，限制最大跳数 6（避免大图卡顿）
    const maxHops = 6;
    const queue: number[][] = [[startIdx]];
    const visited = new Set<number>([startIdx]);
    let iterations = 0;
    const maxIter = 50000; // 安全阀
    while (queue.length && iterations < maxIter) {
      iterations++;
      const path = queue.shift()!;
      if (path.length - 1 > maxHops) continue;
      const last = path[path.length - 1];
      if (last === endIdx) return path;
      const neighbors = adj.get(last) || [];
      for (const next of neighbors) {
        if (!visited.has(next)) {
          visited.add(next);
          queue.push([...path, next]);
        }
      }
    }
    return null;
  }, []);

  // ── 节点定位（居中 + 缩放）──
  const focusNode = useCallback((idx: number) => {
    const n = nodesRef.current[idx];
    if (!n) return;
    const { w: W, h: H } = dimsRef.current;
    zoomRef.current = 2.2;
    offsetRef.current = { x: W / 2 - n.x * 2.2, y: H / 2 - n.y * 2.2 };
    setZoom(2.2); setOffset(offsetRef.current);
    refresh();
  }, [refresh]);

  // ── 加载节点真实关系详情（能力1）──
  const loadNodeRelations = useCallback(async (nodeId: number) => {
    setNodeRelationsDetail(null);
    try {
      const res = await getNodeRelations(nodeId);
      const relations = res.relations || [];
      // 解析关系类型分布
      const typeDist: Record<string, number> = {};
      const strongest: { target: number; type: string; strength: number }[] = [];
      for (const r of relations) {
        const type = (r.type || 'related').toLowerCase();
        typeDist[type] = (typeDist[type] || 0) + 1;
        strongest.push({ target: r.target, type, strength: r.strength });
      }
      strongest.sort((a, b) => b.strength - a.strength);
      setNodeRelationsDetail({
        typeDist,
        strongest: strongest.slice(0, 5),
        degree: relations.length,
      });
    } catch { /* 静默失败，保留本地统计 */ }
  }, []);

  const handleMouseDown = (e: React.MouseEvent) => {
    setDragging(true); dragRef.current = { x: e.clientX - offsetRef.current.x, y: e.clientY - offsetRef.current.y };
  };
  const handleMouseMove = (e: React.MouseEvent) => {
    if (dragging) {
      offsetRef.current = { x: e.clientX - dragRef.current.x, y: e.clientY - dragRef.current.y };
      setOffset(offsetRef.current); refresh(); return;
    }
    const canvas = canvasRef.current; if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const { w: W, h: H } = dimsRef.current;
    const sx = (e.clientX - rect.left) / rect.width * W, sy = (e.clientY - rect.top) / rect.height * H;
    const node = findNodeAt((sx - offsetRef.current.x) / zoomRef.current, (sy - offsetRef.current.y) / zoomRef.current);
    if (node) { hoverRef.current = { x: e.clientX, y: e.clientY, node }; setHover(hoverRef.current); canvas.style.cursor = 'pointer'; }
    else { if (hoverRef.current) { hoverRef.current = null; setHover(null); } canvas.style.cursor = 'grab'; /* else分支dragging恒false(CodeQL) */ }
    refresh();
  };
  const handleClick = (e: React.MouseEvent) => {
    const canvas = canvasRef.current; if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const { w: W, h: H } = dimsRef.current;
    const sx = (e.clientX - rect.left) / rect.width * W, sy = (e.clientY - rect.top) / rect.height * H;
    const node = findNodeAt((sx - offsetRef.current.x) / zoomRef.current, (sy - offsetRef.current.y) / zoomRef.current);
    if (node) {
      // 路径模式：点第二个节点计算路径
      if (pathModeRef.current && pathStartRef.current !== null) {
        const path = findPath(pathStartRef.current, node.idx);
        pathResultRef.current = path;
        setPathResult(path);
        setPathNotFound(!path); // 没找到路径则提示
        if (path) setPathNotFound(false);
        setPathMode(false);
        pathModeRef.current = false;
        // 清除选中态，避免和路径高亮冲突
        setSelectedNode(null); selectedNodeRef.current = null;
        clusterMemberIdsRef.current = null;
        // 3秒后自动消失"无路径"提示
        if (!path) setTimeout(() => { setPathNotFound(false); }, 3000);
        refresh();
        return;
      }
      // 普通模式：选中节点 + 加载真实关系
      setSelectedNode(node); selectedNodeRef.current = node;
      window.dispatchEvent(new CustomEvent("field-probe", { detail: { count: Math.min(8, node.mass ? Math.round(node.mass / 8) + 1 : 3) } }));
      setNodeRelations(edgesRef.current.filter(ed => ed.s === node.idx || ed.t === node.idx).length);
      loadNodeRelations(node.id);
      // 清除聚类高亮和路径
      clusterMemberIdsRef.current = null; setClusterMembers(null);
      pathResultRef.current = null; setPathResult(null);
      pathStartRef.current = null; setPathStart(null);
      refresh();
    } else {
      // 点空白：清除所有焦点
      setSelectedNode(null); selectedNodeRef.current = null;
      clusterMemberIdsRef.current = null; setClusterMembers(null);
      pathResultRef.current = null; setPathResult(null);
      pathStartRef.current = null; setPathStart(null);
      setPathMode(false); pathModeRef.current = false;
      refresh();
    }
  };
  const handleWheel = (e: React.WheelEvent) => {
    e.preventDefault(); zoomRef.current = Math.max(0.2, Math.min(8, zoomRef.current - e.deltaY * 0.002)); setZoom(zoomRef.current); refresh();
  };
  const resetView = () => {
    zoomRef.current = 1; offsetRef.current = { x: 0, y: 0 }; selectedClusterRef.current = null; selectedNodeRef.current = null;
    clusterMemberIdsRef.current = null; pathResultRef.current = null; pathStartRef.current = null;
    pathModeRef.current = false;
    setZoom(1); setOffset({ x: 0, y: 0 }); setSelectedCluster(null); setSelectedNode(null);
    setClusterMembers(null); setPathResult(null); setPathStart(null); setPathMode(false);
  };

  if (loading) return (
    <DashboardLayout>
      <DashboardLoading />
    </DashboardLayout>
  );

  // 玻璃态浮动面板基础样式
  const glassPanel: React.CSSProperties = {
    background: 'rgba(6, 6, 20, 0.72)',
    backdropFilter: 'blur(20px) saturate(160%)',
    WebkitBackdropFilter: 'blur(20px) saturate(160%)',
    border: '1px solid rgba(62, 207, 174, 0.18)',
    borderRadius: 14,
    boxShadow: '0 8px 32px rgba(0, 0, 0, 0.5), 0 0 40px rgba(62, 207, 174, 0.06), inset 0 1px 0 rgba(62, 207, 174, 0.08)',
  };

  return (
    <DashboardLayout>
      {/* 全屏图谱容器 */}
      <div style={{
        position: 'relative',
        height: 'calc(100vh - 40px)',
        minHeight: 500,
        borderRadius: 16,
        overflow: 'hidden',
        background: 'transparent', // 让 SacredBackground 透出
      }}>
        {error ? (
          <div style={{ position: 'absolute', inset: 0, display: 'flex', alignItems: 'center', justifyContent: 'center' }}>
            <div style={{ ...glassPanel, textAlign: 'center', padding: 40, color: '#f87171', maxWidth: 400 }}>
              <p style={{ marginBottom: 12 }}>{error}</p>
              <button onClick={() => { setError(''); setLoading(true); refresh(); }}
                style={{ background: 'rgba(62,207,174,0.12)', color: 'var(--accent-cyan-bright)', border: '1px solid rgba(62,207,174,0.3)', padding: '8px 20px', borderRadius: 8, cursor: 'pointer', fontSize: 13, fontFamily: 'var(--font-heading)' }}>
                {t('dash.graph.retry')}
              </button>
            </div>
          </div>
        ) : (
          <>
            {/* canvas 撑满 */}
            <canvas ref={canvasRef} style={{ width: '100%', height: '100%', display: 'block', cursor: 'grab' }}
              onWheel={handleWheel} onMouseDown={handleMouseDown} onMouseMove={handleMouseMove}
              onMouseUp={() => setDragging(false)} onMouseLeave={() => { setDragging(false); if (hoverRef.current) { hoverRef.current = null; setHover(null); } }}
              onClick={handleClick} />

            {/* ── 顶部浮动工具栏（左上角玻璃卡）── */}
            <div style={{
              ...glassPanel,
              position: 'absolute', top: 16, left: 16, zIndex: 10,
              padding: '10px 12px',
              maxWidth: 'calc(100vw - 32px)',
            }}>
              {/* 第一行：标题 + 统计 */}
              <div style={{ display: 'flex', alignItems: 'center', gap: 10, marginBottom: 8, flexWrap: 'wrap' }}>
                <span style={{ color: 'var(--text-primary)', fontFamily: 'var(--font-display)', fontSize: 13, fontWeight: 700, letterSpacing: '0.02em' }}>
                  {t('dash.graph.title')}
                </span>
                <span style={{ color: 'var(--accent-cyan-bright)', fontSize: 10, fontFamily: 'var(--font-mono)', letterSpacing: '0.05em', opacity: 0.8 }}>
                  {stats.nodes} {t('dash.graph.stats.nodes')} · {stats.edges} {t('dash.graph.stats.edges')} · {stats.clusters} {t('dash.graph.stats.clusters')}
                  {graphMeta.truncated && (
                    <span style={{ color: '#3ecfae', fontSize: 11, marginLeft: 8 }} title="top by mass">
                      ⦿ 显示前 {stats.nodes} / 共 {graphMeta.total} 节点(按质量)
                    </span>
                  )}
                </span>
              </div>
              {/* 第二行：搜索 + 缩放 + 重置 */}
              <div style={{ display: 'flex', gap: 6, alignItems: 'center', flexWrap: 'wrap' }}>
                <div style={{ position: 'relative', flex: '0 1 160px' }}>
                  <Search size={13} style={{ position: 'absolute', left: 10, top: '50%', transform: 'translateY(-50%)', color: 'var(--text-tertiary)' }} />
                  <input type="text" value={searchQ} onChange={e => { searchQRef.current = e.target.value; setSearchQ(e.target.value); refresh(); }} placeholder={t('dash.graph.filter.placeholder')}
                    style={{ width: '100%', background: 'rgba(62,207,174,0.04)', color: 'var(--text-primary)', border: '1px solid rgba(62,207,174,0.12)', borderRadius: 8, padding: '5px 8px 5px 28px', fontSize: 12, boxSizing: 'border-box', outline: 'none' }} />
                </div>
                <button onClick={() => { zoomRef.current = Math.min(8, zoomRef.current * 1.25); setZoom(zoomRef.current); refresh(); }} style={tb}><ZoomIn size={14} /></button>
                <button onClick={() => { zoomRef.current = Math.max(0.2, zoomRef.current * 0.8); setZoom(zoomRef.current); refresh(); }} style={tb}><ZoomOut size={14} /></button>
                <button onClick={resetView} style={tb}><RotateCcw size={14} /></button>
                <button
                  onClick={async () => {
                    if (kgQuality) { setKgQuality(null); return; }
                    setKgLoading(true);
                    try { const q = await getKgQuality(100); setKgQuality(q); } catch { /* 静默 */ }
                    setKgLoading(false);
                  }}
                  style={{ ...tb, color: kgQuality ? '#3ecfae' : 'var(--accent-cyan-bright)', borderColor: kgQuality ? 'rgba(52,211,153,0.3)' : 'rgba(62,207,174,0.12)' }}
                  title="图谱健康评估"
                >
                  {kgLoading ? <Activity size={14} className="animate-spin" /> : <HeartPulse size={14} />}
                </button>
              </div>
              {/* 第三行：边类型图例 */}
              <div style={{ display: 'flex', gap: 8, marginTop: 6, flexWrap: 'wrap' }}>
                {Object.entries(EDGE_COLORS).map(([type, color]) => (
                  <div key={type} style={{ display: 'flex', alignItems: 'center', gap: 3 }}>
                    <div style={{ width: 12, height: 2, background: color, borderRadius: 1 }} />
                    <span style={{ color: 'var(--text-tertiary)', fontSize: 10 }}>{EDGE_LABEL_KEYS[type] ? t(EDGE_LABEL_KEYS[type] as TranslationKey) : type} ({edgeTypeCounts[type] || 0})</span>
                  </div>
                ))}
              </div>
            </div>

            {/* ── 选中节点详情（右侧浮动卡：关系分布 + 路径按钮 + 内容）── */}
            {selectedNode && (
              <div style={{
                ...glassPanel,
                position: 'absolute', top: 16, right: 16, zIndex: 10,
                width: 320, maxWidth: 'calc(100vw - 32px)',
                maxHeight: 'calc(100vh - 140px)', overflowY: 'auto',
                padding: 16,
              }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
                  <h3 style={{ color: 'var(--text-primary)', fontSize: 13, fontWeight: 700, fontFamily: 'var(--font-heading)', letterSpacing: '0.03em', margin: 0 }}>
                    {t('dash.graph.node.title')} <span style={{ color: 'var(--accent-cyan-bright)', fontFamily: 'var(--font-mono)' }}>#{selectedNode.id}</span>
                  </h3>
                  <button onClick={() => { setSelectedNode(null); selectedNodeRef.current = null; }} style={{ color: 'var(--text-tertiary)', background: 'none', border: 'none', cursor: 'pointer', padding: 4 }}><X size={15} /></button>
                </div>
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 6, marginBottom: 12 }}>
                  <Metric label={t('dash.graph.node.relations')} value={String(nodeRelationsDetail?.degree ?? nodeRelations)} color="#3ecfae" />
                  <Metric label={t('dash.graph.node.mass')} value={selectedNode.mass.toFixed(2)} color="#8b7ec8" />
                </div>

                {/* 关系类型分布（能力1：调 getNodeRelations 的真实数据）*/}
                {nodeRelationsDetail && Object.keys(nodeRelationsDetail.typeDist).length > 0 && (
                  <div style={{ marginBottom: 12 }}>
                    <div style={{ color: 'var(--text-tertiary)', fontSize: 10, textTransform: 'uppercase', marginBottom: 6, display: 'flex', alignItems: 'center', gap: 4, fontFamily: 'var(--font-heading)', letterSpacing: '0.05em' }}><GitBranch size={11} /> 关系分布</div>
                    {Object.entries(nodeRelationsDetail.typeDist).sort((a, b) => b[1] - a[1]).map(([type, count]) => {
                      const total = nodeRelationsDetail.degree || 1;
                      const pct = (count / total) * 100;
                      const color = EDGE_COLORS[type] || '#3ecfae';
                      const label = EDGE_LABEL_KEYS[type] ? t(EDGE_LABEL_KEYS[type] as TranslationKey) : type;
                      return (
                        <div key={type} style={{ marginBottom: 4 }}>
                          <div style={{ display: 'flex', justifyContent: 'space-between', marginBottom: 2 }}>
                            <span style={{ color: 'var(--text-secondary)', fontSize: 10 }}>{label}</span>
                            <span style={{ color, fontSize: 10, fontFamily: 'var(--font-mono)' }}>{count}</span>
                          </div>
                          <div style={{ height: 4, background: 'rgba(62,207,174,0.06)', borderRadius: 2, overflow: 'hidden' }}>
                            <div style={{ width: `${pct}%`, height: '100%', background: color, borderRadius: 2, boxShadow: `0 0 6px ${color}` }} />
                          </div>
                        </div>
                      );
                    })}
                  </div>
                )}

                {/* 最强关系 Top 3 */}
                {nodeRelationsDetail && nodeRelationsDetail.strongest.length > 0 && (
                  <div style={{ marginBottom: 12 }}>
                    <div style={{ color: 'var(--text-tertiary)', fontSize: 10, textTransform: 'uppercase', marginBottom: 6, fontFamily: 'var(--font-heading)', letterSpacing: '0.05em' }}><Target size={11} /> 最强关联</div>
                    {nodeRelationsDetail.strongest.slice(0, 3).map((s, i) => {
                      const color = EDGE_COLORS[s.type] || '#3ecfae';
                      return (
                        <button key={i} onClick={() => {
                          // 点击最强关联 → 定位到目标节点
                          const targetNode = nodesRef.current.find(n => n.id === s.target);
                          if (targetNode) focusNode(targetNode.idx);
                        }} style={{ display: 'flex', alignItems: 'center', gap: 6, width: '100%', marginBottom: 3, padding: '4px 6px', background: 'rgba(62,207,174,0.03)', border: '1px solid rgba(62,207,174,0.08)', borderRadius: 6, cursor: 'pointer', textAlign: 'left' }}>
                          <span style={{ color, fontSize: 9, padding: '1px 4px', borderRadius: 3, background: `${color}15` }}>{EDGE_LABEL_KEYS[s.type] ? t(EDGE_LABEL_KEYS[s.type] as TranslationKey) : s.type}</span>
                          <span style={{ color: 'var(--text-secondary)', fontSize: 10, fontFamily: 'var(--font-mono)', flex: 1 }}>#{s.target}</span>
                          <span style={{ color: 'var(--accent-cyan-bright)', fontSize: 10, fontFamily: 'var(--font-mono)' }}>{s.strength.toFixed(2)}</span>
                        </button>
                      );
                    })}
                  </div>
                )}

                {/* 路径查找按钮（能力3）*/}
                <button
                  onClick={() => {
                    if (pathMode) {
                      setPathMode(false); pathModeRef.current = false;
                    } else {
                      pathStartRef.current = selectedNode.idx; setPathStart(selectedNode.idx);
                      setPathMode(true); pathModeRef.current = true;
                      // 清除选中态进入路径模式
                      setSelectedNode(null); selectedNodeRef.current = null;
                      clusterMemberIdsRef.current = null;
                      pathResultRef.current = null; setPathResult(null);
                      refresh();
                    }
                  }}
                  style={{
                    width: '100%', marginBottom: 12, padding: '8px 12px',
                    background: pathMode ? 'rgba(255,215,0,0.12)' : 'rgba(62,207,174,0.06)',
                    border: `1px solid ${pathMode ? 'rgba(255,215,0,0.4)' : 'rgba(62,207,174,0.2)'}`,
                    borderRadius: 8, cursor: 'pointer',
                    color: pathMode ? '#FFD700' : 'var(--accent-cyan-bright)',
                    fontSize: 11, fontFamily: 'var(--font-heading)', fontWeight: 600,
                    letterSpacing: '0.05em', display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 6,
                  }}
                >
                  <Route size={13} />
                  {pathMode ? '点击目标节点找路径…' : '查找关联路径'}
                </button>

                {selectedNode.labels.length > 0 && (
                  <div style={{ marginBottom: 12 }}>
                    <div style={{ color: 'var(--text-tertiary)', fontSize: 10, textTransform: 'uppercase', marginBottom: 4, display: 'flex', alignItems: 'center', gap: 4, fontFamily: 'var(--font-heading)', letterSpacing: '0.05em' }}><Tag size={11} /> {t('dash.graph.node.labels')}</div>
                    <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>{selectedNode.labels.map(l => <span key={l} style={{ background: 'rgba(62,207,174,0.08)', color: 'var(--accent-cyan-bright)', fontSize: 11, padding: '2px 8px', borderRadius: 4, border: '1px solid rgba(62,207,174,0.15)' }}>{l}</span>)}</div>
                  </div>
                )}
                <div>
                  <div style={{ color: 'var(--text-tertiary)', fontSize: 10, textTransform: 'uppercase', marginBottom: 4, display: 'flex', alignItems: 'center', gap: 4, fontFamily: 'var(--font-heading)', letterSpacing: '0.05em' }}><Activity size={11} /> {t('dash.graph.node.content')}</div>
                  <p style={{ color: 'var(--text-secondary)', fontSize: 12, lineHeight: 1.6, whiteSpace: 'pre-wrap', maxHeight: 200, overflow: 'auto', background: 'rgba(0,0,0,0.25)', padding: 10, borderRadius: 8, margin: 0, border: '1px solid rgba(62,207,174,0.06)' }}>{selectedNode.content.slice(0, 500)}{selectedNode.content.length > 500 ? '...' : ''}</p>
                </div>
              </div>
            )}

            {/* ── 路径模式提示 + 路径结果 + 无路径提示（能力3）── */}
            {(pathMode || pathResult || pathNotFound) && (
              <div style={{
                ...glassPanel,
                position: 'absolute', top: 16, left: '50%', transform: 'translateX(-50%)', zIndex: 11,
                padding: '8px 16px', display: 'flex', alignItems: 'center', gap: 10,
              }}>
                {pathMode ? (
                  <>
                    <Navigation size={14} color="#FFD700" />
                    <span style={{ color: '#FFD700', fontSize: 11, fontFamily: 'var(--font-heading)', letterSpacing: '0.05em' }}>路径模式：点击目标节点</span>
                    <button onClick={() => { setPathMode(false); pathModeRef.current = false; pathStartRef.current = null; setPathStart(null); refresh(); }} style={{ color: 'var(--text-tertiary)', background: 'none', border: 'none', cursor: 'pointer' }}><X size={13} /></button>
                  </>
                ) : pathResult ? (
                  <>
                    <Route size={14} color="#FFD700" />
                    <span style={{ color: 'var(--text-primary)', fontSize: 11, fontFamily: 'var(--font-heading)' }}>
                      路径长度：<span style={{ color: '#FFD700', fontFamily: 'var(--font-mono)' }}>{pathResult.length - 1}</span> 跳 · 经 <span style={{ color: '#FFD700', fontFamily: 'var(--font-mono)' }}>{pathResult.length}</span> 节点
                    </span>
                    <button onClick={() => { pathResultRef.current = null; setPathResult(null); pathStartRef.current = null; setPathStart(null); refresh(); }} style={{ color: 'var(--text-tertiary)', background: 'none', border: 'none', cursor: 'pointer' }}><X size={13} /></button>
                  </>
                ) : pathNotFound ? (
                  <>
                    <Route size={14} color="#f87171" />
                    <span style={{ color: '#f87171', fontSize: 11, fontFamily: 'var(--font-heading)' }}>无关联路径（超过 6 跳或不连通）</span>
                  </>
                ) : null}
              </div>
            )}

            {/* ── 聚类成员列表（能力2：点聚类后显示成员）── */}
            {clusterMembers && (
              <div style={{
                ...glassPanel,
                position: 'absolute', top: 16, right: 16, zIndex: 10,
                width: 320, maxWidth: 'calc(100vw - 32px)',
                maxHeight: 'calc(100vh - 140px)', overflowY: 'auto',
                padding: 16,
              }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
                  <h3 style={{ color: 'var(--text-primary)', fontSize: 13, fontWeight: 700, fontFamily: 'var(--font-heading)', margin: 0 }}>
                    聚类成员 <span style={{ color: 'var(--accent-cyan-bright)', fontFamily: 'var(--font-mono)' }}>{clusterMembers.length}</span>
                  </h3>
                  <button onClick={() => { setClusterMembers(null); clusterMemberIdsRef.current = null; refresh(); }} style={{ color: 'var(--text-tertiary)', background: 'none', border: 'none', cursor: 'pointer', padding: 4 }}><X size={15} /></button>
                </div>
                <div style={{ display: 'flex', flexDirection: 'column', gap: 4 }}>
                  {clusterMembers.slice(0, 50).map(m => (
                    <button key={m.id} onClick={() => {
                      const node = nodesRef.current.find(n => n.id === m.id);
                      if (node) focusNode(node.idx);
                    }} style={{ display: 'flex', flexDirection: 'column', gap: 3, padding: '6px 8px', background: 'rgba(62,207,174,0.03)', border: '1px solid rgba(62,207,174,0.08)', borderRadius: 6, cursor: 'pointer', textAlign: 'left' }}>
                      <div style={{ display: 'flex', alignItems: 'center', gap: 6 }}>
                        <span style={{ color: 'var(--accent-cyan-bright)', fontSize: 10, fontFamily: 'var(--font-mono)' }}>#{m.id}</span>
                        {m.labels.slice(0, 2).map(l => <span key={l} style={{ color: 'var(--text-tertiary)', fontSize: 9, padding: '0 4px', borderRadius: 3, background: 'rgba(62,207,174,0.06)' }}>{l}</span>)}
                      </div>
                      <span style={{ color: 'var(--text-secondary)', fontSize: 10, lineHeight: 1.4, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{m.content.slice(0, 60)}</span>
                    </button>
                  ))}
                  {clusterMembers.length > 50 && (
                    <div style={{ textAlign: 'center', color: 'var(--text-tertiary)', fontSize: 10, padding: 8 }}>还有 {clusterMembers.length - 50} 条…</div>
                  )}
                </div>
              </div>
            )}

            {/* ── 图谱健康评估面板（能力4，左下角浮动卡）── */}
            {kgQuality && (
              <div style={{
                ...glassPanel,
                position: 'absolute', bottom: 16, left: 16, zIndex: 10,
                width: 260, padding: 14,
              }}>
                <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 10 }}>
                  <h3 style={{ color: 'var(--text-primary)', fontSize: 12, fontWeight: 700, fontFamily: 'var(--font-heading)', letterSpacing: '0.05em', margin: 0, display: 'flex', alignItems: 'center', gap: 6 }}>
                    <HeartPulse size={13} color="#3ecfae" /> 图谱健康
                  </h3>
                  <button onClick={() => setKgQuality(null)} style={{ color: 'var(--text-tertiary)', background: 'none', border: 'none', cursor: 'pointer', padding: 0 }}><X size={13} /></button>
                </div>
                {/* 综合评分 */}
                <div style={{ textAlign: 'center', marginBottom: 12 }}>
                  <div style={{ fontSize: 36, fontWeight: 800, fontFamily: 'var(--font-display)', color: kgQuality.density_score >= 50 ? '#3ecfae' : (kgQuality.density_score >= 25 ? '#3ecfae' : '#f87171') }}>
                    {kgQuality.density_score}
                  </div>
                  <div style={{ fontSize: 10, color: 'var(--text-tertiary)', fontFamily: 'var(--font-heading)', letterSpacing: '0.1em', textTransform: 'uppercase' }}>
                    {kgQuality.assessment}
                  </div>
                </div>
                {/* 指标网格 */}
                <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 6, marginBottom: 10 }}>
                  <div style={{ background: 'rgba(62,207,174,0.04)', borderRadius: 6, padding: '5px 7px' }}>
                    <div style={{ color: 'var(--text-tertiary)', fontSize: 9 }}>孤立率</div>
                    <div style={{ color: kgQuality.orphan_rate_pct > 30 ? '#f87171' : 'var(--text-primary)', fontSize: 13, fontFamily: 'var(--font-mono)' }}>{kgQuality.orphan_rate_pct.toFixed(1)}%</div>
                  </div>
                  <div style={{ background: 'rgba(62,207,174,0.04)', borderRadius: 6, padding: '5px 7px' }}>
                    <div style={{ color: 'var(--text-tertiary)', fontSize: 9 }}>平均关系</div>
                    <div style={{ color: 'var(--text-primary)', fontSize: 13, fontFamily: 'var(--font-mono)' }}>{kgQuality.relation_density.avg_per_memory.toFixed(1)}</div>
                  </div>
                  <div style={{ background: 'rgba(62,207,174,0.04)', borderRadius: 6, padding: '5px 7px' }}>
                    <div style={{ color: 'var(--text-tertiary)', fontSize: 9 }}>聚类数</div>
                    <div style={{ color: 'var(--text-primary)', fontSize: 13, fontFamily: 'var(--font-mono)' }}>{kgQuality.total_clusters}</div>
                  </div>
                  <div style={{ background: 'rgba(62,207,174,0.04)', borderRadius: 6, padding: '5px 7px' }}>
                    <div style={{ color: 'var(--text-tertiary)', fontSize: 9 }}>平均强度</div>
                    <div style={{ color: 'var(--text-primary)', fontSize: 13, fontFamily: 'var(--font-mono)' }}>{kgQuality.strength_distribution.avg_strength.toFixed(2)}</div>
                  </div>
                </div>
                {/* 强度分布条 */}
                <div>
                  <div style={{ color: 'var(--text-tertiary)', fontSize: 9, marginBottom: 4 }}>关系强度分布</div>
                  <div style={{ display: 'flex', height: 6, borderRadius: 3, overflow: 'hidden' }}>
                    <div style={{ width: `${kgQuality.strength_distribution.strong_ge_0_5}%`, background: '#3ecfae' }} title={`强 ${kgQuality.strength_distribution.strong_ge_0_5}%`} />
                    <div style={{ width: `${kgQuality.strength_distribution.medium}%`, background: '#3ecfae' }} title={`中 ${kgQuality.strength_distribution.medium}%`} />
                    <div style={{ width: `${kgQuality.strength_distribution.weak_lt_0_2}%`, background: '#f87171' }} title={`弱 ${kgQuality.strength_distribution.weak_lt_0_2}%`} />
                  </div>
                  <div style={{ display: 'flex', justifyContent: 'space-between', marginTop: 3, fontSize: 8, color: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }}>
                    <span>强 {kgQuality.strength_distribution.strong_ge_0_5}%</span>
                    <span>中 {kgQuality.strength_distribution.medium}%</span>
                    <span>弱 {kgQuality.strength_distribution.weak_lt_0_2}%</span>
                  </div>
                </div>
              </div>
            )}

            {/* ── 右下角聚类选择器（保留浮动，玻璃态升级）── */}
            <div style={{
              ...glassPanel,
              position: 'absolute', bottom: 16, right: 16, zIndex: 10,
              padding: '6px 10px',
            }}>
              <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', maxWidth: 240 }}>
                {Array.from({ length: Math.min(stats.clusters, 15) }, (_, i) => (
                  <button key={i} onClick={() => {
                    const v = selectedCluster === i ? null : i;
                    selectedClusterRef.current = v; setSelectedCluster(v);
                    setSelectedNode(null); selectedNodeRef.current = null;
                    pathResultRef.current = null; setPathResult(null);
                    if (v !== null && clustersDataRef.current[v]) {
                      // 能力2：加载聚类成员，高亮 + 显示列表
                      const memberIds = clustersDataRef.current[v].member_ids || [];
                      const idToIdx = new Map<number, number>();
                      nodesRef.current.forEach((n, idx) => idToIdx.set(n.id, idx));
                      const memberIdxSet = new Set<number>();
                      const members = memberIds.map(id => {
                        const idx = idToIdx.get(id);
                        if (idx !== undefined) { memberIdxSet.add(idx); return nodesRef.current[idx]; }
                        return null;
                      }).filter((n): n is SNode => n !== null).map(n => ({ id: n.id, content: n.content, labels: n.labels }));
                      clusterMemberIdsRef.current = memberIdxSet;
                      setClusterMembers(members);
                    } else {
                      clusterMemberIdsRef.current = null;
                      setClusterMembers(null);
                    }
                    refresh();
                  }}
                    style={{ display: 'flex', alignItems: 'center', gap: 3, cursor: 'pointer', background: 'none', border: 'none', padding: 0, opacity: selectedCluster !== null && selectedCluster !== i ? 0.3 : 1 }}>
                    <div style={{ width: 9, height: 9, borderRadius: 2, background: CLUSTER_COLORS[i % CLUSTER_COLORS.length], boxShadow: `0 0 6px ${CLUSTER_COLORS[i % CLUSTER_COLORS.length]}` }} />
                    <span style={{ color: 'var(--text-secondary)', fontSize: 10, fontFamily: 'var(--font-mono)' }}>{i + 1}</span>
                  </button>
                ))}
              </div>
            </div>

            {/* ── 底部可折叠统计面板（默认收起）── */}
            {(topConcepts.length > 0 || clusterInfo.length > 0) && (
              <>
                {/* 展开/收起按钮 */}
                <button
                  onClick={() => setStatsPanelOpen(!statsPanelOpen)}
                  style={{
                    ...glassPanel,
                    position: 'absolute', bottom: 16, left: '50%', transform: 'translateX(-50%)', zIndex: 10,
                    padding: '7px 18px', cursor: 'pointer',
                    color: 'var(--accent-cyan-bright)', fontSize: 11, fontFamily: 'var(--font-heading)',
                    letterSpacing: '0.08em', textTransform: 'uppercase', fontWeight: 600,
                    display: 'flex', alignItems: 'center', gap: 8,
                  }}
                >
                  {statsPanelOpen ? <ChevronDown size={14} /> : <ChevronUp size={14} />}
                  {statsPanelOpen ? '收起统计' : '概念 / 聚类统计'}
                </button>

                {/* 展开后的统计面板 */}
                {statsPanelOpen && (
                  <div style={{
                    ...glassPanel,
                    position: 'absolute', bottom: 56, left: '50%', transform: 'translateX(-50%)', zIndex: 10,
                    width: 'min(720px, 90vw)', padding: 16,
                    display: 'grid', gridTemplateColumns: topConcepts.length > 0 && clusterInfo.length > 0 ? '1fr 1fr' : '1fr', gap: 16,
                  }}>
                    {topConcepts.length > 0 && (
                      <div>
                        <h3 style={{ color: 'var(--text-primary)', fontSize: 12, fontWeight: 700, marginBottom: 10, display: 'flex', alignItems: 'center', gap: 6, fontFamily: 'var(--font-heading)', letterSpacing: '0.05em' }}><Tag size={14} color="#3ecfae" /> {t('dash.graph.concepts')}</h3>
                        <div>
                          {topConcepts.map((c, i) => {
                            const mx = topConcepts[0]?.member_count || 1;
                            return (
                              <div key={i} style={{ display: 'flex', alignItems: 'center', gap: 8, marginBottom: 5 }}>
                                <span style={{ color: 'var(--text-secondary)', fontSize: 11, minWidth: 80, textAlign: 'right', overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{c.label}</span>
                                <div style={{ flex: 1, height: 5, background: 'rgba(62,207,174,0.06)', borderRadius: 3, overflow: 'hidden' }}><div style={{ width: `${(c.member_count / mx) * 100}%`, height: '100%', background: CLUSTER_COLORS[i % CLUSTER_COLORS.length], borderRadius: 3, boxShadow: `0 0 8px ${CLUSTER_COLORS[i % CLUSTER_COLORS.length]}` }} /></div>
                                <span style={{ color: 'var(--text-tertiary)', fontSize: 10, minWidth: 40, fontFamily: 'var(--font-mono)' }}>{c.member_count}</span>
                              </div>
                            );
                          })}
                        </div>
                      </div>
                    )}
                    {clusterInfo.length > 0 && (
                      <div>
                        <h3 style={{ color: 'var(--text-primary)', fontSize: 12, fontWeight: 700, marginBottom: 10, display: 'flex', alignItems: 'center', gap: 6, fontFamily: 'var(--font-heading)', letterSpacing: '0.05em' }}><GitBranch size={14} color="#8b7ec8" /> {t('dash.graph.clusters')}</h3>
                        <div style={{ display: 'grid', gridTemplateColumns: '1fr 1fr', gap: 6 }}>
                          {clusterInfo.slice(0, 10).map((c, i) => (
                            <button key={i} onClick={() => { const v = selectedCluster === i ? null : i; selectedClusterRef.current = v; setSelectedCluster(v); setSelectedNode(null); refresh(); setStatsPanelOpen(false); }}
                              style={{ background: selectedCluster === i ? `${CLUSTER_COLORS[i % CLUSTER_COLORS.length]}12` : 'rgba(62,207,174,0.02)', border: `1px solid ${selectedCluster === i ? `${CLUSTER_COLORS[i % CLUSTER_COLORS.length]}40` : 'rgba(62,207,174,0.08)'}`, borderRadius: 8, padding: 7, cursor: 'pointer', textAlign: 'left', color: 'inherit' }}>
                              <div style={{ display: 'flex', alignItems: 'center', gap: 4, marginBottom: 4 }}>
                                <div style={{ width: 8, height: 8, borderRadius: 2, background: CLUSTER_COLORS[i % CLUSTER_COLORS.length] }} />
                                <span style={{ color: 'var(--text-primary)', fontSize: 11, fontFamily: 'var(--font-mono)' }}>C{i + 1}</span>
                                <span style={{ color: 'var(--text-tertiary)', fontSize: 10, marginLeft: 'auto' }}>{c.size}</span>
                              </div>
                              <div style={{ display: 'flex', gap: 2, flexWrap: 'wrap' }}>{c.top_labels.slice(0, 2).map((tl: { label: string }) => <span key={tl.label} style={{ color: CLUSTER_COLORS[i % CLUSTER_COLORS.length], fontSize: 9, background: `${CLUSTER_COLORS[i % CLUSTER_COLORS.length]}10`, padding: '1px 4px', borderRadius: 3 }}>{tl.label}</span>)}</div>
                            </button>
                          ))}
                        </div>
                      </div>
                    )}
                  </div>
                )}
              </>
            )}

            {/* ── hover 轻量 tooltip（保留）── */}
            {hover && !dragging && (
              <div style={{ position: 'fixed', left: Math.min(hover.x + 12, window.innerWidth - 280), top: hover.y - 8, ...glassPanel, padding: '8px 12px', maxWidth: 260, pointerEvents: 'none', zIndex: 100 }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 4 }}>
                  <span style={{ color: 'var(--accent-cyan-bright)', fontSize: 11, fontWeight: 700, fontFamily: 'var(--font-mono)' }}>#{hover.node.id}</span>
                  <span style={{ color: 'var(--text-tertiary)', fontSize: 10 }}>mass {hover.node.mass.toFixed(1)}</span>
                  {hover.node.cluster >= 0 && <span style={{ color: CLUSTER_COLORS[hover.node.cluster % CLUSTER_COLORS.length], fontSize: 10, fontFamily: 'var(--font-mono)' }}>C{hover.node.cluster + 1}</span>}
                </div>
                <p style={{ color: 'var(--text-secondary)', fontSize: 11, lineHeight: 1.5, margin: 0, display: '-webkit-box', WebkitLineClamp: 3, WebkitBoxOrient: 'vertical', overflow: 'hidden' }}>{hover.node.content.slice(0, 200)}</p>
                {hover.node.labels.length > 0 && (
                  <div style={{ display: 'flex', gap: 3, flexWrap: 'wrap', marginTop: 4 }}>
                    {hover.node.labels.slice(0, 4).map(l => <span key={l} style={{ color: 'var(--accent-cyan-bright)', fontSize: 9, background: 'rgba(62,207,174,0.08)', padding: '1px 4px', borderRadius: 3 }}>{l}</span>)}
                  </div>
                )}
              </div>
            )}
          </>
        )}
      </div>
    </DashboardLayout>
  );
}

const tb: React.CSSProperties = { background: 'rgba(62,207,174,0.05)', border: '1px solid rgba(62,207,174,0.12)', color: 'var(--accent-cyan-bright)', padding: 5, borderRadius: 8, cursor: 'pointer', display: 'flex', alignItems: 'center', justifyContent: 'center' };
function Metric({ label, value, color }: { label: string; value: string; color: string }) {
  return <div style={{ background: `${color}0d`, border: `1px solid ${color}22`, borderRadius: 8, padding: '6px 8px', textAlign: 'center' }}><div style={{ color: '#6b7280', fontSize: 10, marginBottom: 2 }}>{label}</div><div style={{ color: '#f0f0f5', fontSize: 14, fontWeight: 600 }}>{value}</div></div>;
}
