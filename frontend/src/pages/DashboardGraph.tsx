import { useEffect, useRef, useState, useCallback } from 'react';
import DashboardLayout from '@/components/DashboardLayout';
import { DashboardLoading } from '@/components/DashboardUI';
import { errMsg, getGraphExport, getGraphAnalysis, getNodeRelations, getKgQuality } from '@/lib/api';
import type { KgQuality } from '@/lib/api';
import { Search, ZoomIn, ZoomOut, RotateCcw, X, GitBranch, Tag, Activity, ChevronDown, ChevronUp, Route, Navigation, HeartPulse, Target, Orbit, Eye, ChevronsDownUp } from 'lucide-react';
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
interface SEdge { s: number; t: number; type: string; strength: number; hits: number; }
interface ClusterInfo { size: number; top_labels: { label: string; count: number }[]; }
interface HoverInfo { x: number; y: number; node: SNode; }
// ── 总览模式(渐进披露): 聚类折叠为超节点星域 ──
// 借鉴星座式语义放射: 任何时刻屏幕只讲一个故事(刘启航方向: 选择性/筛选性展示)
type ViewMode = 'overview' | 'observe';
interface SuperNode {
  ci: number;                 // 聚类索引(-1 = 未分组伪超节点)
  members: number[];          // 节点 idx, 按 mass 降序
  visible: Set<number>;       // 展开时实际渲染的成员(top-K, 渐进披露: 数字徽章代替渲染其余)
  memberCount: number;
  totalMass: number;
  topLabels: string[];
  intraEdges: SEdge[];        // 簇内边(展开时画)
}
interface SuperEdge { a: number; b: number; count: number; strength: number; }

export default function DashboardGraph() {
  const { t } = useI18nContext();
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [graphMeta, setGraphMeta] = useState<{ truncated: boolean; total: number; totalEdges: number }>({ truncated: false, total: 0, totalEdges: 0 });
  const [searchQ, setSearchQ] = useState('');
  const [, setZoom] = useState(1);
  const [, setOffset] = useState({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const [stats, setStats] = useState({ nodes: 0, edges: 0, clusters: 0, interCluster: 0, highways: 0 });
  // 边强度LOD下限: 大图客户端剔除弱边(渲染循环读ref, 滑杆读写双份=zoom/offset同款模式)
  const [edgeLod, setEdgeLod] = useState(0);
  const edgeLodRef = useRef(0);
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
  // ── 总览模式 state（渐进披露三态: L0超节点/L1展开/L2节点焦点）──
  const [viewMode, setViewMode] = useState<ViewMode>(() =>
    (typeof localStorage !== 'undefined' && localStorage.getItem('epicode.graph.view') === 'observe') ? 'observe' : 'overview');
  const viewModeRef = useRef<ViewMode>(viewMode);
  const [expandedClusters, setExpandedClusters] = useState<Set<number>>(new Set());
  const expandedRef = useRef<Set<number>>(new Set());
  const [superHover, setSuperHover] = useState<{ x: number; y: number; sp: SuperNode } | null>(null);
  const superHoverRef = useRef<number | null>(null);

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
  // 总览模式数据(refs, draw循环读)
  const superNodesRef = useRef<SuperNode[]>([]);
  const superEdgesRef = useRef<SuperEdge[]>([]);
  const superByCiRef = useRef<Map<number, SuperNode>>(new Map()); // ci → 超节点(draw/hit-test查)
  const centsRef = useRef<Map<number, { x: number; y: number }>>(new Map()); // 活体质心(超节点位置, 点击命中用)
  const anchorsRef = useRef<Map<number, { x: number; y: number }>>(new Map()); // ci → 总览锚点(网格; 防settled后漂移叠死)

  useEffect(() => {
    let mounted = true;
    async function load() {
      try {
        const [data, analysis] = await Promise.all([getGraphExport(), getGraphAnalysis()]);
        if (!mounted) return;
        setGraphMeta({ truncated: !!data.truncated, total: data.total_nodes || (data.nodes || []).length, totalEdges: data.total_edges || 0 });
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
            es.push({ s: si, t: ti, type: rt, strength: re.strength, hits: re.hits || 0 });
            tc[rt] = (tc[rt] || 0) + 1;
          }
        }
        const ies: SEdge[] = [];
        for (const re of (data.inter_cluster_edges || [])) {
          const si = idToIdx.get(re.source), ti = idToIdx.get(re.target);
          if (si !== undefined && ti !== undefined)
            ies.push({ s: si, t: ti, type: (re.relation_type || 'related').toLowerCase(), strength: re.strength, hits: 0 });
        }
        nodesRef.current = ns; edgesRef.current = es; interEdgesRef.current = ies;
        setStats({ nodes: ns.length, edges: es.length, clusters: (data.clusters || []).length, interCluster: ies.length, highways: es.filter(e => e.hits > 0).length });
        setClusterInfo(analysis?.cluster_analysis || []);
        setEdgeTypeCounts(tc);
        setTopConcepts((data.concepts || []).slice(0, 12));
        // 缓存 clusters 的 member_ids（能力2：聚类下钻看成员）
        clustersDataRef.current = (data.clusters || []) as { member_ids: number[]; top_labels: unknown[] }[];
        // ── 总览模式数据构建: 聚类→超节点(成员按mass降序, 簇内边预过滤) + 跨簇边聚合为超边 ──
        {
          const byCluster = new Map<number, number[]>();
          for (const n of ns) {
            const arr = byCluster.get(n.cluster); if (arr) arr.push(n.idx); else byCluster.set(n.cluster, [n.idx]);
          }
          const intraByCluster = new Map<number, SEdge[]>();
          for (const e of es) {
            const ca = ns[e.s]?.cluster, cb = ns[e.t]?.cluster;
            if (ca === undefined || cb === undefined || ca !== cb) continue;
            const arr = intraByCluster.get(ca); if (arr) arr.push(e); else intraByCluster.set(ca, [e]);
          }
          const supers: SuperNode[] = [];
          const EXPAND_K = 48; // 展开时按mass取top-K(选择性展示: 其余用数字徽章代言)
          for (const [ci, memberIdxs] of byCluster) {
            const members = memberIdxs.slice().sort((a, b) => ns[b].mass - ns[a].mass);
            let totalMass = 0; for (const mi of members) totalMass += ns[mi].mass;
            const labels = (analysis?.cluster_analysis?.[ci]?.top_labels || []).map((tl: { label: string }) => tl.label);
            supers.push({
              ci, members, visible: new Set(members.slice(0, EXPAND_K)),
              memberCount: members.length, totalMass,
              topLabels: labels.length > 0 ? labels : (members[0] !== undefined ? ns[members[0]].labels.slice(0, 2) : []),
              intraEdges: intraByCluster.get(ci) || [],
            });
          }
          // 未分组(-1)也构建超节点, 但空簇跳过
          superNodesRef.current = supers;
          superByCiRef.current = new Map(supers.map(sp => [sp.ci, sp]));
          // 超边: 跨簇边按聚类对聚合(count+总强度, 绘制弧线时用)
          const seMap = new Map<string, SuperEdge>();
          for (const e of ies) {
            const ca = ns[e.s]?.cluster, cb = ns[e.t]?.cluster;
            if (ca === undefined || cb === undefined || ca === cb) continue;
            const key = `${Math.min(ca, cb)}|${Math.max(ca, cb)}`;
            let se = seMap.get(key);
            if (!se) { se = { a: Math.min(ca, cb), b: Math.max(ca, cb), count: 0, strength: 0 }; seMap.set(key, se); }
            se.count++; se.strength += e.strength;
          }
          superEdgesRef.current = Array.from(seMap.values()).sort((x, y) => y.count - x.count);
        }
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

    // 总览锚点: 聚类网格按实际画布尺寸计算(与load()同一公式, 但load用的是1200x600虚拟尺寸)。
    // 必要性: settled后簇间O(n²)斥力永久关闭, 只剩活体质心弱弹簧+布朗抖动,
    // 而圆形力场(fieldR≈313px)装不下20簇网格跨度 → 外圈簇被持续压向中心,
    // 十几分钟后全部簇叠死在画布中央(实测)。锚点让总览星域永远稳定。
    {
      const numC = superNodesRef.current.filter(sp => sp.ci >= 0).length || 1;
      const anchors = new Map<number, { x: number; y: number }>();
      for (const sp of superNodesRef.current) {
        if (sp.ci < 0) continue;
        const layer = sp.ci % 6;
        const layerH = (H - 80) / 6;
        const inLayer = Math.ceil(numC / 6);
        const pos = Math.floor(sp.ci / 6);
        anchors.set(sp.ci, { x: W * (0.15 + 0.7 * (pos + 0.5) / inLayer), y: 40 + layer * layerH + layerH / 2 });
      }
      anchorsRef.current = anchors;
    }

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
      // 总览模式: settled后成员拉向固定网格锚点(替代活体质心) — 斥力已关,
      // 质心弹簧+圆形力场会让簇漂移叠死(实测)。观测模式保持原样(活体质心+力场)。
      const ovMode = viewModeRef.current === 'overview';
      const breathe = settled ? (ovMode ? 0.0012 : 0.0003) : 0.0008;
      const damping = settled ? 0.95 : 0.82;
      // 展开簇形态收紧: settled后仍保留簇内局部斥力(O(k²)仅展开簇成员, ≤48²很便宜) —
      // 否则锚点拉力会把可见成员拉叠成一坨; 局部斥力×加强拉力平衡 = 紧凑晕圈
      if (settled && ovMode && expandedRef.current.size > 0) {
        for (const ci of expandedRef.current) {
          const arr: typeof ns = [];
          for (const n of ns) if (n.cluster === ci) arr.push(n);
          for (let i = 0; i < arr.length; i++) {
            for (let j = i + 1; j < arr.length; j++) {
              const dx = arr[j].x - arr[i].x, dy = arr[j].y - arr[i].y;
              const d2 = dx * dx + dy * dy; if (d2 < 1) continue;
              const d = Math.sqrt(d2);
              const rep = Math.min(400 / d2, 0.32); // 斥力cap压到0.32: 平衡点≈0.32/0.007≈46px → 紧凑晕圈
              const fx = (dx / d) * rep, fy = (dy / d) * rep;
              arr[i].vx -= fx; arr[i].vy -= fy; arr[j].vx += fx; arr[j].vy += fy;
            }
          }
        }
      }
      for (const n of ns) {
        const anchor: { x: number; y: number } | null = (settled && ovMode && n.cluster >= 0)
          ? anchorsRef.current.get(n.cluster) ?? null : null;
        const cent = centers.get(n.cluster);
        const inExpanded = n.cluster >= 0 && expandedRef.current.has(n.cluster);
        const pull = anchor && inExpanded ? 0.007 : breathe; // ×2.5: 压住局部斥力cap → 晕圈收紧
        if (anchor) { n.vx += (anchor.x - n.x) * pull; n.vy += (anchor.y - n.y) * pull; }
        else if (cent) { n.vx += (cent.x / cent.n - n.x) * breathe; n.vy += (cent.y / cent.n - n.y) * breathe; }
        else { n.vx += (cx - n.x) * 0.0004; n.vy += (cy - n.y) * 0.0004; }
        // 收敛后加随机微扰（布朗运动，模拟神经活动）— 展开簇成员减半防扩散晕
        const jit = settled ? (inExpanded ? 0.004 : 0.01) : 0;
        if (jit) { n.vx += (Math.random() - 0.5) * jit; n.vy += (Math.random() - 0.5) * jit; }
        n.vx *= damping; n.vy *= damping; n.x += n.vx; n.y += n.vy;
        // 关键修复：用圆形力场代替矩形 clamp，避免网络被矩形边框"压成长方形"。
        // 节点离中心超过 fieldR 时，施加向心推力（弹性边界），形成自然的圆形/有机团块。
        // 总览豁免: 成簇节点锚在网格上(网格跨度大于力场直径), 力场只会把外圈簇往中心挤。
        if (!(ovMode && n.cluster >= 0)) {
          const ddx = n.x - cx, ddy = n.y - cy;
          const distC = Math.sqrt(ddx * ddx + ddy * ddy);
          if (distC > fieldR) {
            const k = (distC - fieldR) * 0.18; // 向心弹性
            n.vx -= (ddx / distC) * k;
            n.vy -= (ddy / distC) * k;
          }
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

      // ── 总览模式(渐进披露): 超节点星域 + 选择性展示 ──
      // ov=总览开关; eff=有效展开集合(用户展开 ∪ 搜索命中自动展开=查询即过滤)
      const ov = viewModeRef.current === 'overview';
      let eff: Set<number> | null = null;
      const cents = new Map<number, { x: number; y: number }>();
      if (ov) {
        eff = expandedRef.current;
        if (sq && vis) {
          // 搜索命中所在簇自动展开 — 借鉴PixVision"查询即过滤"
          eff = new Set(expandedRef.current);
          vis.forEach(i => { const ci = ns[i]?.cluster; if (ci !== undefined) eff!.add(ci); });
        }
        // 活体质心(跟踪模拟中的节点, 超节点随物理呼吸)
        const acc = new Map<number, { x: number; y: number; n: number }>();
        for (const n of ns) {
          const c = acc.get(n.cluster);
          if (c) { c.x += n.x; c.y += n.y; c.n++; } else acc.set(n.cluster, { x: n.x, y: n.y, n: 1 });
        }
        acc.forEach((v, k) => cents.set(k, { x: v.x / v.n, y: v.y / v.n }));
        centsRef.current = cents;
      }

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

      // ── 超边弧线(总览)已移至超节点绘制之后(z-order上层) — 画在下层时20个球体辉光(半径rr*2.6)
      //    会把弧线整体淹没(实测像素扫描弧线在画但视觉0条), 上层+发光才能"浮"出来 ──

      // 跨簇边（暗，流动效果）— LOD: 弱于下限的边直接不画
      for (const e of ies) {
        if (e.strength < edgeLodRef.current) continue;
        const a = ns[e.s], b = ns[e.t]; if (!a || !b) continue;
        // 总览: 跨簇真实边只在双展开簇间显示(其余由超边弧线代言)
        if (ov && eff && (!eff.has(a.cluster) || !eff.has(b.cluster))) continue;
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
      // 簇内边（亮，流动粒子 — 突触放电系统）— LOD剔除 + 主干道高亮
      // 主干道 = 检索强化命中的边(hits>0, 后端PPR/multi_hop走过): 更亮更粗
      for (const e of es) {
        if (e.strength < edgeLodRef.current) continue;
        const a = ns[e.s], b = ns[e.t]; if (!a || !b) continue;
        // 总览: 只画双展开簇内的可见成员(top-K)边
        if (ov && eff) {
          const sa2 = superByCiRef.current.get(a.cluster), sb2 = superByCiRef.current.get(b.cluster);
          if (!sa2 || !sb2 || !eff.has(a.cluster) || !eff.has(b.cluster) || !sa2.visible.has(e.s) || !sb2.visible.has(e.t)) continue;
        }
        if (vis && !vis.has(e.s) && !vis.has(e.t)) continue;
        if (sc !== null && a.cluster !== sc && b.cluster !== sc) continue;
        const isHv = hvNode && (hvNode.idx === e.s || hvNode.idx === e.t);
        const isSelEdge = sel && (sel.idx === e.s || sel.idx === e.t);
        const isHwy = e.hits > 0;
        const ec = EDGE_COLORS[e.type] || '#3ecfae';
        // 焦点模式 dim 逻辑
        const edgeKey = `${Math.min(e.s, e.t)}-${Math.max(e.s, e.t)}`;
        const isPathEdge = pathEdges && pathEdges.has(edgeKey);
        if (hasFocus && !isPathEdge && !highlightSet!.has(e.s)) {
          // 非高亮边在焦点模式下极暗(主干道略可见——检索巩固过的路不熄灭)
          ctx.globalAlpha = isHwy ? 0.1 : 0.03;
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
          ctx.globalAlpha = isHv ? 0.6 : (isHwy ? 0.4 : 0.12);
        }
        ctx.beginPath(); ctx.moveTo(a.x, a.y); ctx.lineTo(b.x, b.y);
        ctx.strokeStyle = ec;
        ctx.lineWidth = isHv || isSelEdge ? 1.5 : (isHwy ? 1.1 : 0.5);
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
        if (ov && eff && (!eff.has(a.cluster) || !eff.has(b.cluster))) continue;
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
        // 总览: 折叠簇成员不画(超节点球体代言); 展开簇只画top-K可见成员
        if (ov && eff) {
          const sp = superByCiRef.current.get(n.cluster);
          if (!sp || !eff.has(n.cluster) || !sp.visible.has(n.idx)) continue;
        }
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
            ctx.font = `600 ${Math.min(11, 8 + n.mass / 8)}px JetBrains Mono, var(--font-mono), monospace`;
            ctx.textAlign = 'left';
            // 半透明底pill — 提升交叉重叠时的可读性
            const tw = ctx.measureText(labelText).width;
            const ty = n.y + 3;
            ctx.globalAlpha = isHv ? 0.85 : 0.62;
            ctx.beginPath(); ctx.roundRect(n.x + r + 2, ty - 9, tw + 8, 13, 4);
            ctx.fillStyle = 'rgba(8,10,18,0.8)'; ctx.fill();
            ctx.globalAlpha = isHv ? 1 : 0.88;
            ctx.fillStyle = isHv ? '#f0f0f5' : color;
            ctx.shadowColor = color;
            ctx.shadowBlur = isHv ? 8 : 4;
            ctx.fillText(labelText, n.x + r + 6, ty);
            ctx.shadowBlur = 0;
          }
        }
      }
      ctx.globalAlpha = 1;

      // ── 超节点绘制(总览) ──
      // L0 折叠态: 星域球体(呼吸辉光+核心球+粒子环+标签+成员数徽章) — "数字代替渲染"
      // L1 展开态: 中心枢纽小节点(光环+top-K徽章) — 点击收起
      if (ov && eff) {
        for (const sp of superNodesRef.current) {
          if (sp.memberCount === 0) continue;
          const c = cents.get(sp.ci); if (!c) continue;
          const color = sp.ci >= 0 ? CLUSTER_COLORS[sp.ci % CLUSTER_COLORS.length] : '#6b7280';
          const isExp = eff.has(sp.ci);
          const isHov = superHoverRef.current === sp.ci;
          if (!isExp) {
            const r = Math.max(15, Math.min(42, 12 + Math.sqrt(sp.memberCount) * 3.4));
            const rr = r * (1 + Math.sin(t * 1.4 + sp.ci * 1.7) * 0.05); // 呼吸
            // 外层大辉光
            const glow = ctx.createRadialGradient(c.x, c.y, 0, c.x, c.y, rr * 2.6);
            glow.addColorStop(0, color + (isHov ? '55' : '26'));
            glow.addColorStop(0.5, color + '10');
            glow.addColorStop(1, color + '00');
            ctx.globalAlpha = isHov ? 0.95 : 0.8;
            ctx.beginPath(); ctx.arc(c.x, c.y, rr * 2.6, 0, Math.PI * 2);
            ctx.fillStyle = glow; ctx.fill();
            // 核心球(径向渐变=球体感)
            const core = ctx.createRadialGradient(c.x - rr * 0.3, c.y - rr * 0.3, rr * 0.1, c.x, c.y, rr);
            core.addColorStop(0, '#ffffff');
            core.addColorStop(0.28, color);
            core.addColorStop(1, color + '40');
            ctx.globalAlpha = isHov ? 1 : 0.9;
            ctx.beginPath(); ctx.arc(c.x, c.y, rr, 0, Math.PI * 2);
            ctx.fillStyle = core; ctx.fill();
            // 环绕粒子(椭圆轨道)
            ctx.globalAlpha = 0.55;
            for (let k = 0; k < 3; k++) {
              const ang = t * 0.5 + (k * Math.PI * 2) / 3 + sp.ci;
              ctx.beginPath();
              ctx.arc(c.x + Math.cos(ang) * (rr + 7), c.y + Math.sin(ang) * (rr + 7) * 0.35, 1.6, 0, Math.PI * 2);
              ctx.fillStyle = '#ffffff'; ctx.fill();
            }
            // 标签 + 数字徽章(半透明底pill, 遮挡时仍可读)
            ctx.globalAlpha = 1; ctx.textAlign = 'center';
            ctx.font = '700 12px JetBrains Mono, monospace';
            const labelText = sp.topLabels.length > 0 ? sp.topLabels[0] : (sp.ci >= 0 ? `C${sp.ci + 1}` : 'Ungrouped');
            const ltw = ctx.measureText(labelText).width;
            ctx.globalAlpha = 0.85;
            ctx.beginPath(); ctx.roundRect(c.x - ltw / 2 - 7, c.y - rr - 12 - 10, ltw + 14, 15, 5);
            ctx.fillStyle = 'rgba(8,10,18,0.82)'; ctx.fill();
            ctx.globalAlpha = 1;
            ctx.fillStyle = '#f0f0f5';
            ctx.shadowColor = color; ctx.shadowBlur = isHov ? 12 : 6;
            ctx.fillText(labelText, c.x, c.y - rr - 12);
            ctx.shadowBlur = 0;
            const badge = `${sp.memberCount} · ${sp.totalMass.toFixed(0)}m`;
            ctx.font = '500 10px JetBrains Mono, monospace';
            const btw = ctx.measureText(badge).width;
            ctx.globalAlpha = 0.72;
            ctx.beginPath(); ctx.roundRect(c.x - btw / 2 - 6, c.y + rr + 16 - 9, btw + 12, 13, 4);
            ctx.fillStyle = 'rgba(8,10,18,0.82)'; ctx.fill();
            ctx.globalAlpha = 0.95;
            ctx.fillStyle = color;
            ctx.fillText(badge, c.x, c.y + rr + 16);
          } else {
            // 展开簇枢纽: 小核 + 脉冲环 + top-K徽章
            const hr = isHov ? 8 : 6;
            const ring = ctx.createRadialGradient(c.x, c.y, 0, c.x, c.y, hr * 3);
            ring.addColorStop(0, color + '50'); ring.addColorStop(1, color + '00');
            ctx.globalAlpha = 0.7;
            ctx.beginPath(); ctx.arc(c.x, c.y, hr * 3, 0, Math.PI * 2);
            ctx.fillStyle = ring; ctx.fill();
            ctx.globalAlpha = 0.92;
            ctx.beginPath(); ctx.arc(c.x, c.y, hr, 0, Math.PI * 2);
            ctx.fillStyle = color; ctx.fill();
            ctx.globalAlpha = 0.5;
            ctx.beginPath(); ctx.arc(c.x, c.y, hr + 4 + Math.sin(t * 2 + sp.ci) * 2, 0, Math.PI * 2);
            ctx.strokeStyle = color; ctx.lineWidth = 1; ctx.stroke();
            // top-K徽章: 渐进披露数字
            const shown = Math.min(sp.visible.size, sp.memberCount);
            ctx.globalAlpha = 0.85; ctx.textAlign = 'center';
            ctx.font = '500 9px JetBrains Mono, monospace';
            ctx.fillStyle = color;
            ctx.fillText(sp.memberCount > shown ? `${shown}/${sp.memberCount}` : `${sp.memberCount}`, c.x, c.y - hr - 7);
          }
        }
        ctx.globalAlpha = 1;
      }

      // ── 超边弧线(总览, z-order上层): 聚类对聚合为发光曲线 + 流动光点(PixVision式光弧) ──
      // 画在超节点之后: 下层时被球体辉光(半径rr*2.6)整体淹没(实测在画但视觉0条)
      if (ov && eff) {
        const arr = superEdgesRef.current;
        const maxCnt = arr.length ? arr[0].count : 1;
        for (const se of arr) {
          const pa = cents.get(se.a), pb = cents.get(se.b);
          if (!pa || !pb) continue;
          const dx = pb.x - pa.x, dy = pb.y - pa.y;
          const d = Math.sqrt(dx * dx + dy * dy) || 1;
          const bow = Math.min(d * 0.22, 60) * ((se.a * 31 + se.b * 17) % 2 === 0 ? 1 : -1);
          const cxp = (pa.x + pb.x) / 2 - (dy / d) * bow, cyp = (pa.y + pb.y) / 2 + (dx / d) * bow;
          const hov = superHoverRef.current === se.a || superHoverRef.current === se.b;
          const strengthN = se.count / maxCnt;
          const ca = se.a >= 0 ? CLUSTER_COLORS[se.a % CLUSTER_COLORS.length] : '#6b7280';
          const cb2 = se.b >= 0 ? CLUSTER_COLORS[se.b % CLUSTER_COLORS.length] : '#6b7280';
          const grad = ctx.createLinearGradient(pa.x, pa.y, pb.x, pb.y);
          grad.addColorStop(0, ca + '80'); grad.addColorStop(0.5, '#3ecfaeaa'); grad.addColorStop(1, cb2 + '80');
          ctx.globalAlpha = Math.min((0.16 + 0.45 * strengthN) * (hov ? 2.2 : 1), 0.92);
          ctx.beginPath(); ctx.moveTo(pa.x, pa.y); ctx.quadraticCurveTo(cxp, cyp, pb.x, pb.y);
          ctx.strokeStyle = grad; ctx.lineWidth = hov ? 2.8 : 1.1 + strengthN * 2.4;
          // 发光描边 — 让弧线从球体辉光里"浮"出来
          ctx.shadowColor = '#3ecfae'; ctx.shadowBlur = hov ? 14 : 6 + strengthN * 6;
          ctx.stroke();
          ctx.shadowBlur = 0;
          // 流动光点(较强弧常驻, 弱弧hover时)
          if (strengthN > 0.35 || hov) {
            const seed = ((se.a * 73856093) ^ (se.b * 19349663)) >>> 0;
            const phase = (seed % 1000) / 1000;
            const p = (t * 0.25 + phase) % 1;
            const qpx = (1 - p) * (1 - p) * pa.x + 2 * (1 - p) * p * cxp + p * p * pb.x;
            const qpy = (1 - p) * (1 - p) * pa.y + 2 * (1 - p) * p * cyp + p * p * pb.y;
            ctx.globalAlpha = 0.5;
            ctx.beginPath(); ctx.arc(qpx, qpy, 6.5, 0, Math.PI * 2); ctx.fillStyle = '#3ecfae'; ctx.fill();
            ctx.globalAlpha = 0.95;
            ctx.shadowColor = '#ffffff'; ctx.shadowBlur = 8;
            ctx.beginPath(); ctx.arc(qpx, qpy, 2.6, 0, Math.PI * 2); ctx.fillStyle = '#ffffff'; ctx.fill();
            ctx.shadowBlur = 0;
          }
        }
        ctx.globalAlpha = 1;
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
  // ── 总览: 超节点命中检测(折叠球体大半径 / 展开枢纽小半径) ──
  const findSuperAt = useCallback((mx: number, my: number): number | null => {
    for (const sp of superNodesRef.current) {
      if (sp.memberCount === 0) continue;
      const c = centsRef.current.get(sp.ci); if (!c) continue;
      const expNow = expandedRef.current.has(sp.ci);
      const r = expNow ? 12 : Math.max(15, Math.min(42, 12 + Math.sqrt(sp.memberCount) * 3.4)) + 6;
      const d = Math.sqrt((c.x - mx) ** 2 + (c.y - my) ** 2);
      if (d < r) return sp.ci;
    }
    return null;
  }, []);
  // 节点在总览下是否可见(用户展开的簇 + top-K成员)
  const nodeVisibleInOverview = useCallback((n: SNode): boolean => {
    const sp = superByCiRef.current.get(n.cluster);
    return !!sp && expandedRef.current.has(n.cluster) && sp.visible.has(n.idx);
  }, []);
  const toggleExpand = useCallback((ci: number) => {
    const next = new Set(expandedRef.current);
    if (next.has(ci)) next.delete(ci); else next.add(ci);
    expandedRef.current = next; setExpandedClusters(next);
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
    const gx = (sx - offsetRef.current.x) / zoomRef.current, gy = (sy - offsetRef.current.y) / zoomRef.current;
    let node = findNodeAt(gx, gy);
    // 总览: 折叠簇/不可见成员不响应hover
    if (node && viewModeRef.current === 'overview' && !nodeVisibleInOverview(node)) node = null;
    if (node) {
      superHoverRef.current = null; setSuperHover(null);
      hoverRef.current = { x: e.clientX, y: e.clientY, node }; setHover(hoverRef.current); canvas.style.cursor = 'pointer';
      } else if (viewModeRef.current === 'overview') {
        const sci = findSuperAt(gx, gy);
        if (sci !== null) {
          hoverRef.current = null; setHover(null);
          const sp = superByCiRef.current.get(sci) || null;
          superHoverRef.current = sci;
          setSuperHover(sp ? { x: e.clientX, y: e.clientY, sp } : null);
          canvas.style.cursor = 'pointer';
        } else {
        superHoverRef.current = null; setSuperHover(null);
        if (hoverRef.current) { hoverRef.current = null; setHover(null); }
        canvas.style.cursor = 'grab';
      }
    } else {
      superHoverRef.current = null; setSuperHover(null);
      if (hoverRef.current) { hoverRef.current = null; setHover(null); }
      canvas.style.cursor = 'grab'; /* else分支dragging恒false(CodeQL) */
    }
    refresh();
  };
  const handleClick = (e: React.MouseEvent) => {
    const canvas = canvasRef.current; if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const { w: W, h: H } = dimsRef.current;
    const sx = (e.clientX - rect.left) / rect.width * W, sy = (e.clientY - rect.top) / rect.height * H;
    const gx = (sx - offsetRef.current.x) / zoomRef.current, gy = (sy - offsetRef.current.y) / zoomRef.current;
    let node = findNodeAt(gx, gy);
    if (node && viewModeRef.current === 'overview' && !nodeVisibleInOverview(node)) node = null;
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
      // 总览"答案画布": 选中节点 → 自动展开最强邻居所在簇(一次只讲一个故事)
      if (viewModeRef.current === 'overview') {
        const neighborClusters = new Map<number, number>();
        for (const ed of edgesRef.current) {
          let oIdx = -1;
          if (ed.s === node.idx) oIdx = ed.t; else if (ed.t === node.idx) oIdx = ed.s; else continue;
          const oci = nodesRef.current[oIdx]?.cluster;
          if (oci === undefined || oci === node.cluster) continue;
          if (ed.strength > (neighborClusters.get(oci) || 0)) neighborClusters.set(oci, ed.strength);
        }
        const topCis = Array.from(neighborClusters.entries()).sort((a, b) => b[1] - a[1]).slice(0, 3).map(x => x[0]);
        if (topCis.length > 0 || !expandedRef.current.has(node.cluster)) {
          const next = new Set(expandedRef.current);
          next.add(node.cluster); topCis.forEach(ci => next.add(ci));
          expandedRef.current = next; setExpandedClusters(next);
        }
      }
      refresh();
    } else if (viewModeRef.current === 'overview' && findSuperAt(gx, gy) !== null) {
      // 总览: 点超节点 = 展开(L0→L1) / 收起(L1→L0)
      toggleExpand(findSuperAt(gx, gy)!);
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
    // 总览: 重置=回到L0星域(全部收起)
    if (viewModeRef.current === 'overview') { expandedRef.current = new Set(); setExpandedClusters(new Set()); }
  };
  // 模式切换(总览↔观测), localStorage持久化
  const switchMode = (m: ViewMode) => {
    viewModeRef.current = m; setViewMode(m);
    try { localStorage.setItem('epicode.graph.view', m); } catch { /* 私密模式等 */ }
    refresh();
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
                  {stats.nodes} {t('dash.graph.stats.nodes')} · {stats.edges} {t('dash.graph.stats.edges')} · {stats.clusters} {t('dash.graph.stats.clusters')}{stats.highways > 0 && <> · {stats.highways} {t('dash.graph.stats.highways')}</>}
                  {graphMeta.truncated && (
                    <span style={{ color: '#3ecfae', fontSize: 11, marginLeft: 8 }} title="top by mass">
                      ⦿ 显示前 {stats.nodes} / 共 {graphMeta.total} 节点(按质量)
                    </span>
                  )}
                </span>
              </div>
              {/* 第二行：模式切换 + 搜索 + 缩放 + 重置 */}
              <div style={{ display: 'flex', gap: 6, alignItems: 'center', flexWrap: 'wrap' }}>
                {/* 总览/观测 双模式(渐进披露 vs 全量仪器观) */}
                <div style={{ display: 'flex', borderRadius: 8, overflow: 'hidden', border: '1px solid rgba(62,207,174,0.15)' }} title={t('dash.graph.mode.hint')}>
                  <button onClick={() => switchMode('overview')} style={{
                    display: 'flex', alignItems: 'center', gap: 4, padding: '5px 10px', cursor: 'pointer',
                    background: viewMode === 'overview' ? 'rgba(62,207,174,0.16)' : 'rgba(62,207,174,0.03)',
                    border: 'none', color: viewMode === 'overview' ? 'var(--accent-cyan-bright)' : 'var(--text-tertiary)',
                    fontSize: 11, fontFamily: 'var(--font-heading)', fontWeight: 600, letterSpacing: '0.05em',
                  }}><Orbit size={12} />{t('dash.graph.mode.overview')}</button>
                  <button onClick={() => switchMode('observe')} style={{
                    display: 'flex', alignItems: 'center', gap: 4, padding: '5px 10px', cursor: 'pointer',
                    background: viewMode === 'observe' ? 'rgba(62,207,174,0.16)' : 'rgba(62,207,174,0.03)',
                    border: 'none', borderLeft: '1px solid rgba(62,207,174,0.12)',
                    color: viewMode === 'observe' ? 'var(--accent-cyan-bright)' : 'var(--text-tertiary)',
                    fontSize: 11, fontFamily: 'var(--font-heading)', fontWeight: 600, letterSpacing: '0.05em',
                  }}><Eye size={12} />{t('dash.graph.mode.observe')}</button>
                </div>
                <div style={{ position: 'relative', flex: '0 1 160px' }}>
                  <Search size={13} style={{ position: 'absolute', left: 10, top: '50%', transform: 'translateY(-50%)', color: 'var(--text-tertiary)' }} />
                  <input type="text" value={searchQ} onChange={e => { searchQRef.current = e.target.value; setSearchQ(e.target.value); refresh(); }} placeholder={t('dash.graph.filter.placeholder')}
                    style={{ width: '100%', background: 'rgba(62,207,174,0.04)', color: 'var(--text-primary)', border: '1px solid rgba(62,207,174,0.12)', borderRadius: 8, padding: '5px 8px 5px 28px', fontSize: 12, boxSizing: 'border-box', outline: 'none' }} />
                </div>
                <button onClick={() => { zoomRef.current = Math.min(8, zoomRef.current * 1.25); setZoom(zoomRef.current); refresh(); }} style={tb}><ZoomIn size={14} /></button>
                <button onClick={() => { zoomRef.current = Math.max(0.2, zoomRef.current * 0.8); setZoom(zoomRef.current); refresh(); }} style={tb}><ZoomOut size={14} /></button>
                <button onClick={resetView} style={tb}><RotateCcw size={14} /></button>
                <div style={{ display: 'flex', alignItems: 'center', gap: 4 }} title={t('dash.graph.lod.label')}>
                  <span style={{ fontSize: 9, color: 'var(--text-tertiary)', fontFamily: 'var(--font-mono)' }}>LOD</span>
                  <input type="range" min={0} max={1} step={0.05} value={edgeLod}
                    onChange={e => { const v = parseFloat(e.target.value); edgeLodRef.current = v; setEdgeLod(v); }}
                    style={{ width: 64, accentColor: 'var(--accent-cyan-bright)' }} />
                  <span style={{ fontSize: 9, fontFamily: 'var(--font-mono)', color: 'var(--text-tertiary)' }}>{edgeLod.toFixed(2)}</span>
                </div>
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
                          // 点击最强关联 → 定位到目标节点(总览下先展开其所在簇)
                          const targetNode = nodesRef.current.find(n => n.id === s.target);
                          if (targetNode) {
                            if (viewModeRef.current === 'overview' && !expandedRef.current.has(targetNode.cluster)) toggleExpand(targetNode.cluster);
                            focusNode(targetNode.idx);
                          }
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

            {/* ── 右下角聚类芯片（总览=展开/收起下钻, 观测=过滤高亮）── */}
            <div style={{
              ...glassPanel,
              position: 'absolute', bottom: 16, right: 16, zIndex: 10,
              padding: '6px 10px',
            }}>
              <div style={{ display: 'flex', gap: 6, flexWrap: 'wrap', maxWidth: 240 }}>
                {Array.from({ length: Math.min(stats.clusters, 15) }, (_, i) => {
                  const isExp = viewMode === 'overview' && expandedClusters.has(i);
                  return (
                  <button key={i} onClick={() => {
                    if (viewMode === 'overview') { toggleExpand(i); refresh(); return; }
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
                    style={{ display: 'flex', alignItems: 'center', gap: 3, cursor: 'pointer', background: 'none', border: 'none', padding: 0, opacity: viewMode === 'observe' && selectedCluster !== null && selectedCluster !== i ? 0.3 : (isExp ? 1 : 0.75) }}>
                    <div style={{ width: isExp ? 11 : 9, height: isExp ? 11 : 9, borderRadius: isExp ? '50%' : 2, background: CLUSTER_COLORS[i % CLUSTER_COLORS.length], boxShadow: isExp ? `0 0 8px ${CLUSTER_COLORS[i % CLUSTER_COLORS.length]}` : 'none' }} />
                    <span style={{ color: isExp ? 'var(--text-primary)' : 'var(--text-secondary)', fontSize: 10, fontFamily: 'var(--font-mono)' }}>{i + 1}</span>
                  </button>
                  );
                })}
              </div>
            </div>

            {/* ── 总览KPI缎带(底部): 真实数据徽章 — 借鉴PixVision"数字代替渲染" ── */}
            {viewMode === 'overview' && (
              <div style={{
                ...glassPanel,
                position: 'absolute', bottom: 16, left: '50%', transform: 'translateX(-50%)', zIndex: 10,
                padding: '7px 16px', display: 'flex', alignItems: 'center', gap: 14, maxWidth: 'calc(100vw - 80px)',
              }}>
                <Kpi label={t('dash.graph.stats.nodes')} value={graphMeta.truncated ? `${stats.nodes}/${graphMeta.total}` : String(stats.nodes)} />
                <Kpi label={t('dash.graph.stats.clusters')} value={String(stats.clusters)} />
                <Kpi label={t('dash.graph.stats.edges')} value={String(graphMeta.totalEdges || stats.edges)} />
                {stats.highways > 0 && <Kpi label={t('dash.graph.stats.highways')} value={String(stats.highways)} />}
                <Kpi label={t('dash.graph.kpi.expanded')} value={`${expandedClusters.size}/${stats.clusters}`} />
                <button onClick={() => { expandedRef.current = new Set(); setExpandedClusters(new Set()); refresh(); }}
                  style={{ display: 'flex', alignItems: 'center', gap: 4, background: 'rgba(62,207,174,0.06)', border: '1px solid rgba(62,207,174,0.18)', color: 'var(--accent-cyan-bright)', padding: '4px 10px', borderRadius: 8, cursor: 'pointer', fontSize: 10, fontFamily: 'var(--font-heading)', letterSpacing: '0.05em' }}>
                  <ChevronsDownUp size={12} />{t('dash.graph.kpi.collapseAll')}
                </button>
              </div>
            )}

            {/* ── 底部可折叠统计面板（默认收起, 观测模式专属）── */}
            {viewMode === 'observe' && (topConcepts.length > 0 || clusterInfo.length > 0) && (
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

            {/* ── 总览: 超节点hover tooltip(标签+成员数+操作提示) ── */}
            {superHover && !dragging && (() => {
              const sp = superHover.sp;
              const color = sp.ci >= 0 ? CLUSTER_COLORS[sp.ci % CLUSTER_COLORS.length] : '#6b7280';
              const isExp = expandedClusters.has(sp.ci);
              return (
                <div style={{ position: 'fixed', left: Math.min(superHover.x + 12, window.innerWidth - 280), top: superHover.y - 8, ...glassPanel, padding: '8px 12px', maxWidth: 260, pointerEvents: 'none', zIndex: 100 }}>
                  <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 4 }}>
                    <div style={{ width: 8, height: 8, borderRadius: '50%', background: color, boxShadow: `0 0 6px ${color}` }} />
                    <span style={{ color: 'var(--text-primary)', fontSize: 11, fontWeight: 700, fontFamily: 'var(--font-heading)' }}>
                      {sp.topLabels.length > 0 ? sp.topLabels.slice(0, 2).join(' · ') : (sp.ci >= 0 ? `C${sp.ci + 1}` : t('dash.graph.super.ungrouped'))}
                    </span>
                  </div>
                  <div style={{ color: 'var(--text-tertiary)', fontSize: 10, fontFamily: 'var(--font-mono)' }}>
                    {sp.memberCount} {t('dash.graph.stats.nodes')} · {sp.totalMass.toFixed(0)} mass
                  </div>
                  <div style={{ color, fontSize: 10, marginTop: 3 }}>
                    {isExp ? t('dash.graph.super.clickToCollapse') : t('dash.graph.super.clickToExpand')}
                  </div>
                </div>
              );
            })()}

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
// 总览KPI缎带项(数字徽章 — "数字代替渲染")
function Kpi({ label, value }: { label: string; value: string }) {
  return (
    <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', minWidth: 44 }}>
      <span style={{ color: 'var(--accent-cyan-bright)', fontSize: 13, fontFamily: 'var(--font-mono)', fontWeight: 700, lineHeight: 1.2 }}>{value}</span>
      <span style={{ color: 'var(--text-tertiary)', fontSize: 9, fontFamily: 'var(--font-heading)', letterSpacing: '0.08em' }}>{label}</span>
    </div>
  );
}
