import { useEffect, useRef, useState } from 'react';
import DashboardLayout from '@/components/DashboardLayout';
import { getGraphExport, getGraphAnalysis } from '@/lib/api';
import { Search, ZoomIn, ZoomOut, RotateCcw, X } from 'lucide-react'
;

const COLORS = ['#a855f7', '#d946ef', '#6366f1', '#ec4899', '#34d399', '#f59e0b', '#60a5fa', '#f87171', '#a3e635', '#22d3ee', '#fb923c', '#818cf8'];

interface SimNode {
  id: number; x: number; y: number; vx: number; vy: number;
  r: number; mass: number; labels: string[]; content: string; cluster: number; idx: number;
}

interface ClusterInfo {
  size: number; top_labels: { label: string; count: number }[];
}

export default function DashboardGraph() {
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const rafRef = useRef(0);
  const [nodes, setNodes] = useState<SimNode[]>([]);
  const [edgePairs, setEdgePairs] = useState<[number, number][]>([]);
  const [clusterColors, setClusterColors] = useState<Map<number, number>>(new Map());
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [searchQ, setSearchQ] = useState('');
  const [zoom, setZoom] = useState(1);
  const [offset, setOffset] = useState({ x: 0, y: 0 });
  const [dragging, setDragging] = useState(false);
  const [stats, setStats] = useState({ nodes: 0, edges: 0, clusters: 0 });
  const [selectedCluster, setSelectedCluster] = useState<number | null>(null);
  const [clusterInfo, setClusterInfo] = useState<ClusterInfo[]>([]);
  const [selectedNode, setSelectedNode] = useState<SimNode | null>(null);
  const [nodeRelations, setNodeRelations] = useState<number>(0);
  const dragRef = useRef({ x: 0, y: 0 });
  const nodesRef = useRef<SimNode[]>([]);
  const edgesRef = useRef<[number, number][]>([]);
  const zoomRef = useRef(1);
  const offsetRef = useRef({ x: 0, y: 0 });
  const searchQRef = useRef('');
  const selectedClusterRef = useRef<number | null>(null);
  const needsRedraw = useRef(false);

  const W = 1200, H = 700;

  useEffect(() => {
    let mounted = true;
    async function load() {
      try {
        const [data, analysis] = await Promise.all([getGraphExport(), getGraphAnalysis() as any]);
        if (!mounted) return;

        const rawNodes = data.nodes || [];
        const rawEdges = data.edges || [];

        const cMap = new Map<number, number>();
        (data.clusters || []).forEach((c: { member_ids: number[] }, ci: number) => {
          (c.member_ids || []).forEach(id => cMap.set(id, ci));
        });

        const idToIdx = new Map<number, number>();
        const ns: SimNode[] = rawNodes.map((rn, i) => {
          idToIdx.set(rn.id, i);
          return {
            id: rn.id, idx: i,
            x: W / 2 + (Math.random() - 0.5) * 400,
            y: H / 2 + (Math.random() - 0.5) * 300,
            vx: 0, vy: 0,
            r: Math.max(3, 3 + Math.min((rn.mass || 1) * 1.5, 12)),
            mass: rn.mass || 1,
            labels: rn.labels || [],
            content: rn.content || '',
            cluster: cMap.get(rn.id) ?? -1,
          };
        });

        const es: [number, number][] = [];
        for (const re of rawEdges) {
          const si = idToIdx.get(re.source);
          const ti = idToIdx.get(re.target);
          if (si !== undefined && ti !== undefined) es.push([si, ti]);
        }

        setClusterColors(cMap);
        setNodes(ns);
        setEdgePairs(es);
        setStats({ nodes: ns.length, edges: es.length, clusters: (data.clusters || []).length });
        setClusterInfo(analysis?.cluster_analysis || []);
        nodesRef.current = ns;
        edgesRef.current = es;
      } catch (e: any) {
        if (mounted) setError(e.message);
      }
      if (mounted) setLoading(false);
    }
    load();
    return () => { mounted = false; };
  }, []);

  useEffect(() => {
    if (!nodes.length) return;
    const canvas = canvasRef.current;
    if (!canvas) return;
    const context = canvas.getContext('2d')!;

    const dpr = Math.min(window.devicePixelRatio || 1, 2);
    canvas.width = W * dpr;
    canvas.height = H * dpr;
    context.setTransform(dpr, 0, 0, dpr, 0, 0);

    let frame = 0;
    const maxFrames = 300;

    function simulate() {
      const ns = nodesRef.current;
      const es = edgesRef.current;
      if (!ns.length) return;

      if (frame < maxFrames) {
        for (let i = 0; i < ns.length; i++) {
          for (let j = i + 1; j < ns.length; j++) {
            const dx = ns[j].x - ns[i].x;
            const dy = ns[j].y - ns[i].y;
            const d2 = dx * dx + dy * dy;
            if (d2 < 1) continue;
            const d = Math.sqrt(d2);
            const f = Math.min(150 / d2, 0.5);
            const fx = (dx / d) * f;
            const fy = (dy / d) * f;
            ns[i].vx -= fx; ns[i].vy -= fy;
            ns[j].vx += fx; ns[j].vy += fy;
          }
        }

        for (const [si, ti] of es) {
          const a = ns[si], b = ns[ti];
          const dx = b.x - a.x, dy = b.y - a.y;
          const d = Math.sqrt(dx * dx + dy * dy) || 1;
          const f = d * 0.0003;
          a.vx += (dx / d) * f; a.vy += (dy / d) * f;
          b.vx -= (dx / d) * f; b.vy -= (dy / d) * f;
        }

        for (const n of ns) {
          n.vx += (W / 2 - n.x) * 0.0002;
          n.vy += (H / 2 - n.y) * 0.0002;
          n.vx *= 0.85; n.vy *= 0.85;
          n.x += n.vx; n.y += n.vy;
          n.x = Math.max(20, Math.min(W - 20, n.x));
          n.y = Math.max(20, Math.min(H - 20, n.y));
        }
        frame++;
      }
    }

    function draw() {
      context.clearRect(0, 0, W, H);

      const ns = nodesRef.current;
      const es = edgesRef.current;
      const z = zoomRef.current;
      const o = offsetRef.current;
      const sq = searchQRef.current;
      const sc = selectedClusterRef.current;

      context.save();
      context.translate(o.x, o.y);
      context.scale(z, z);

      const sqLower = sq ? sq.toLowerCase() : '';
      const visibleSet = sq
        ? new Set(ns.filter(n =>
            n.content.toLowerCase().includes(sqLower) ||
            n.labels.some(l => l.toLowerCase().includes(sqLower))
          ).map(n => n.idx))
        : null;

      context.globalAlpha = 0.15;
      for (const [si, ti] of es) {
        if (visibleSet && !visibleSet.has(si) && !visibleSet.has(ti)) continue;
        if (sc !== null) {
          const a = ns[si], b = ns[ti];
          if (a.cluster !== sc && b.cluster !== sc) continue;
        }
        const a = ns[si], b = ns[ti];
        context.beginPath();
        context.moveTo(a.x, a.y);
        context.lineTo(b.x, b.y);
        context.strokeStyle = '#a855f7';
        context.lineWidth = 0.3;
        context.stroke();
      }

      context.globalAlpha = 1;
      for (const n of ns) {
        const dimmed = (visibleSet && !visibleSet.has(n.idx)) || (sc !== null && n.cluster !== sc);
        context.globalAlpha = dimmed ? 0.12 : 1;
        const ci = clusterColors.get(n.id) ?? -1;
        const color = ci >= 0 ? COLORS[ci % COLORS.length] : '#6b7280';

        context.beginPath();
        context.arc(n.x, n.y, n.r, 0, Math.PI * 2);
        context.fillStyle = color + 'cc';
        context.fill();

        if (!dimmed) {
          context.beginPath();
          context.arc(n.x, n.y, n.r * 0.3, 0, Math.PI * 2);
          context.fillStyle = 'rgba(255,255,255,0.6)';
          context.fill();
        }
      }
      context.globalAlpha = 1;
      context.restore();
    }

    function loop() {
      simulate();
      draw();
      if (frame < maxFrames || needsRedraw.current) {
        needsRedraw.current = false;
        rafRef.current = requestAnimationFrame(loop);
      }
    }
    loop();

    return () => cancelAnimationFrame(rafRef.current);
  }, [nodes, edgePairs, clusterColors]);

  const handleCanvasClick = (e: React.MouseEvent) => {
    const canvas = canvasRef.current;
    if (!canvas) return;
    const rect = canvas.getBoundingClientRect();
    const sx = (e.clientX - rect.left) / rect.width * W;
    const sy = (e.clientY - rect.top) / rect.height * H;
    const mx = (sx - offset.x) / zoom;
    const my = (sy - offset.y) / zoom;

    let closest: SimNode | null = null;
    let closestD = Infinity;
    for (const n of nodesRef.current) {
      const d = Math.sqrt((n.x - mx) ** 2 + (n.y - my) ** 2);
      if (d < n.r + 5 && d < closestD) {
        closest = n;
        closestD = d;
      }
    }

    if (closest) {
      setSelectedNode(closest);
      const relCount = edgesRef.current.filter(([a, b]) => a === closest.idx || b === closest.idx).length;
      setNodeRelations(relCount);
    } else {
      setSelectedNode(null);
    }
  };

  const handleWheel = (e: React.WheelEvent) => {
    e.preventDefault();
    const newZ = Math.max(0.2, Math.min(4, zoomRef.current - e.deltaY * 0.001));
    zoomRef.current = newZ;
    setZoom(newZ);
    needsRedraw.current = true;
    if (rafRef.current === 0) rafRef.current = requestAnimationFrame(function redraw() { });
  };

  const handleMouseDown = (e: React.MouseEvent) => {
    setDragging(true);
    dragRef.current = { x: e.clientX - offsetRef.current.x, y: e.clientY - offsetRef.current.y };
  };

  const handleMouseMove = (e: React.MouseEvent) => {
    if (!dragging) return;
    const newO = { x: e.clientX - dragRef.current.x, y: e.clientY - dragRef.current.y };
    offsetRef.current = newO;
    setOffset(newO);
    needsRedraw.current = true;
  };

  const resetView = () => {
    zoomRef.current = 1; offsetRef.current = { x: 0, y: 0 };
    selectedClusterRef.current = null;
    setZoom(1); setOffset({ x: 0, y: 0 }); setSelectedCluster(null); setSelectedNode(null);
    needsRedraw.current = true;
  };

  if (loading) {
    return (
      <DashboardLayout>
        <div style={{ display: 'flex', alignItems: 'center', justifyContent: 'center', height: '60vh' }}>
          <div style={{ width: 32, height: 32, border: '3px solid #a855f7', borderTopColor: 'transparent', borderRadius: '50%', animation: 'spin 1s linear infinite' }} />
        </div>
      </DashboardLayout>
    );
  }

  return (
    <DashboardLayout>
      <div style={{ marginBottom: 24 }}>
        <h1 style={{ color: '#f0f0f5', fontSize: 26, fontWeight: 700, letterSpacing: '-0.02em', marginBottom: 4 }}>知识图谱</h1>
        <p style={{ color: '#9ca3af', fontSize: 14 }}>{stats.nodes} nodes · {stats.edges} 条关系 · {stats.clusters} clusters</p>
      </div>

      <div style={{ display: 'grid', gridTemplateColumns: selectedNode ? '1fr 320px' : '1fr', gap: 16 }}>
        {/* Main Canvas */}
        <div style={{ position: 'relative' }}>
          {/* Toolbar */}
          <div style={{ display: 'flex', gap: 8, marginBottom: 12, flexWrap: 'wrap', alignItems: 'center' }}>
            <div style={{ position: 'relative', flex: '0 1 200px' }}>
              <Search size={14} style={{ position: 'absolute', left: 12, top: '50%', transform: 'translateY(-50%)', color: '#6b7280' }} />
              <input type="text" value={searchQ} onChange={e => { searchQRef.current = e.target.value; setSearchQ(e.target.value); needsRedraw.current = true; }} placeholder="Filter nodes..."
                style={{ width: '100%', background: 'rgba(255,255,255,0.04)', color: '#f0f0f5', border: '1px solid rgba(255,255,255,0.08)', borderRadius: 8, padding: '7px 10px 7px 34px', fontSize: 13, boxSizing: 'border-box' }} />
            </div>
            <button onClick={() => setZoom(z => z * 1.2)} style={{ background: 'rgba(255,255,255,0.04)', border: '1px solid rgba(255,255,255,0.08)', color: '#9ca3af', padding: 7, borderRadius: 8, cursor: 'pointer' }}><ZoomIn size={16} /></button>
            <button onClick={() => setZoom(z => z * 0.8)} style={{ background: 'rgba(255,255,255,0.04)', border: '1px solid rgba(255,255,255,0.08)', color: '#9ca3af', padding: 7, borderRadius: 8, cursor: 'pointer' }}><ZoomOut size={16} /></button>
            <button onClick={resetView} style={{ background: 'rgba(255,255,255,0.04)', border: '1px solid rgba(255,255,255,0.08)', color: '#9ca3af', padding: 7, borderRadius: 8, cursor: 'pointer' }}><RotateCcw size={16} /></button>
            <button onClick={() => { selectedClusterRef.current = null; setSelectedCluster(null); setSelectedNode(null); needsRedraw.current = true; }}
              style={{ background: selectedCluster !== null ? 'rgba(168,85,247,0.15)' : 'rgba(255,255,255,0.04)', border: `1px solid ${selectedCluster !== null ? 'rgba(168,85,247,0.3)' : 'rgba(255,255,255,0.08)'}`, color: selectedCluster !== null ? '#a855f7' : '#9ca3af', padding: '7px 12px', borderRadius: 8, cursor: 'pointer', fontSize: 12 }}>
              {selectedCluster !== null ? `聚类 ${selectedCluster + 1}` : '所有聚类'}
            </button>
          </div>

          {error ? (
            <div style={{ textAlign: 'center', padding: 48, color: '#f87171', background: 'rgba(255,255,255,0.03)', borderRadius: 14, border: '1px solid rgba(255,255,255,0.06)' }}>{error}</div>
          ) : (
            <div style={{ position: 'relative', borderRadius: 14, overflow: 'hidden', background: 'rgba(0,0,0,0.3)', border: '1px solid rgba(255,255,255,0.06)' }}>
              <canvas ref={canvasRef} style={{ width: '100%', height: '60vh', minHeight: 400, cursor: dragging ? 'grabbing' : 'grab', display: 'block' }}
                onWheel={handleWheel} onMouseDown={handleMouseDown} onMouseMove={handleMouseMove}
                onMouseUp={() => setDragging(false)} onMouseLeave={() => setDragging(false)}
                onClick={handleCanvasClick} />

              {/* Legend */}
              <div style={{ position: 'absolute', bottom: 12, left: 12, background: 'rgba(10,10,15,0.92)', border: '1px solid rgba(255,255,255,0.08)', borderRadius: 10, padding: '8px 12px' }}>
                <div style={{ display: 'flex', gap: 8, flexWrap: 'wrap' }}>
                  {Array.from({ length: Math.min(stats.clusters, 12) }, (_, i) => (
                    <button key={i} onClick={() => { const v = selectedCluster === i ? null : i; selectedClusterRef.current = v; setSelectedCluster(v); needsRedraw.current = true; }}
                      style={{ display: 'flex', alignItems: 'center', gap: 4, cursor: 'pointer', background: 'none', border: 'none', padding: 0,
                        opacity: selectedCluster !== null && selectedCluster !== i ? 0.4 : 1 }}>
                      <div style={{ width: 8, height: 8, borderRadius: 2, background: COLORS[i % COLORS.length] }} />
                      <span style={{ color: '#9ca3af', fontSize: 10 }}>C{i + 1}</span>
                    </button>
                  ))}
                </div>
              </div>
            </div>
          )}
        </div>

        {/* Detail Panel */}
        {selectedNode && (
          <div style={{ background: 'rgba(255,255,255,0.03)', border: '1px solid rgba(255,255,255,0.06)', borderRadius: 14, padding: 16, height: 'fit-content', position: 'sticky', top: 16 }}>
            <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 12 }}>
              <h3 style={{ color: '#f0f0f5', fontSize: 14, fontWeight: 600 }}>节点 #{selectedNode.id}</h3>
              <button onClick={() => setSelectedNode(null)} style={{ color: '#6b7280', background: 'none', border: 'none', cursor: 'pointer' }}><X size={16} /></button>
            </div>

            <div style={{ display: 'flex', gap: 8, marginBottom: 12 }}>
              <div style={{ background: 'rgba(168,85,247,0.08)', border: '1px solid rgba(168,85,247,0.15)', borderRadius: 8, padding: '6px 10px', flex: 1 }}>
                <div style={{ color: '#6b7280', fontSize: 10, marginBottom: 2 }}>质量</div>
                <div style={{ color: '#f0f0f5', fontSize: 15, fontWeight: 600 }}>{selectedNode.mass.toFixed(2)}</div>
              </div>
              <div style={{ background: 'rgba(96,165,250,0.08)', border: '1px solid rgba(96,165,250,0.15)', borderRadius: 8, padding: '6px 10px', flex: 1 }}>
                <div style={{ color: '#6b7280', fontSize: 10, marginBottom: 2 }}>关系数</div>
                <div style={{ color: '#f0f0f5', fontSize: 15, fontWeight: 600 }}>{nodeRelations}</div>
              </div>
              <div style={{ background: 'rgba(52,211,153,0.08)', border: '1px solid rgba(52,211,153,0.15)', borderRadius: 8, padding: '6px 10px', flex: 1 }}>
                <div style={{ color: '#6b7280', fontSize: 10, marginBottom: 2 }}>聚类</div>
                <div style={{ color: '#f0f0f5', fontSize: 15, fontWeight: 600 }}>C{(selectedNode.cluster + 1)}</div>
              </div>
            </div>

            <div style={{ marginBottom: 12 }}>
              <div style={{ color: '#6b7280', fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 4 }}>标签</div>
              <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                {selectedNode.labels.map(l => (
                  <span key={l} style={{ background: 'rgba(168,85,247,0.1)', color: '#a855f7', fontSize: 11, padding: '2px 8px', borderRadius: 4 }}>{l}</span>
                ))}
              </div>
            </div>

            <div>
              <div style={{ color: '#6b7280', fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 4 }}>内容</div>
              <p style={{ color: '#9ca3af', fontSize: 12, lineHeight: 1.6, whiteSpace: 'pre-wrap', maxHeight: 200, overflow: 'auto', background: 'rgba(0,0,0,0.2)', padding: 10, borderRadius: 8 }}>
                {selectedNode.content.slice(0, 500)}{selectedNode.content.length > 500 ? '...' : ''}
              </p>
            </div>

            {selectedNode.cluster >= 0 && clusterInfo[selectedNode.cluster] && (
              <div style={{ marginTop: 12, paddingTop: 12, borderTop: '1px solid rgba(255,255,255,0.06)' }}>
                <div style={{ color: '#6b7280', fontSize: 10, textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 6 }}>聚类 {(selectedNode.cluster + 1)} 信息</div>
                <div style={{ color: '#9ca3af', fontSize: 12, marginBottom: 6 }}>{clusterInfo[selectedNode.cluster].size} 个节点在此聚类中</div>
                <div style={{ display: 'flex', gap: 4, flexWrap: 'wrap' }}>
                  {clusterInfo[selectedNode.cluster].top_labels.slice(0, 4).map((tl: { label: string; count: number }) => (
                    <span key={tl.label} style={{ background: 'rgba(255,255,255,0.04)', color: '#9ca3af', fontSize: 10, padding: '2px 6px', borderRadius: 4 }}>
                      {tl.label} ({tl.count})
                    </span>
                  ))}
                </div>
              </div>
            )}
          </div>
        )}
      </div>

      {/* 聚类 Summary Bar */}
      {clusterInfo.length > 0 && (
        <div style={{ marginTop: 16 }}>
          <h3 style={{ color: '#f0f0f5', fontSize: 14, fontWeight: 600, marginBottom: 12 }}>聚类 Details</h3>
          <div style={{ display: 'grid', gridTemplateColumns: 'repeat(auto-fill, minmax(220px, 1fr))', gap: 8 }}>
            {clusterInfo.slice(0, 12).map((c, i) => (
              <button key={i} onClick={() => { const v = selectedCluster === i ? null : i; selectedClusterRef.current = v; setSelectedCluster(v); setSelectedNode(null); needsRedraw.current = true; }}
                style={{ background: selectedCluster === i ? `${COLORS[i % COLORS.length]}12` : 'rgba(255,255,255,0.02)',
                  border: `1px solid ${selectedCluster === i ? `${COLORS[i % COLORS.length]}30` : 'rgba(255,255,255,0.04)'}`,
                  borderRadius: 10, padding: 12, cursor: 'pointer', textAlign: 'left', color: 'inherit' }}>
                <div style={{ display: 'flex', alignItems: 'center', gap: 6, marginBottom: 6 }}>
                  <div style={{ width: 10, height: 10, borderRadius: 3, background: COLORS[i % COLORS.length] }} />
                  <span style={{ color: '#f0f0f5', fontSize: 12, fontWeight: 500 }}>聚类 {i + 1}</span>
                  <span style={{ color: '#6b7280', fontSize: 11, marginLeft: 'auto' }}>{c.size} nodes</span>
                </div>
                <div style={{ display: 'flex', gap: 3, flexWrap: 'wrap' }}>
                  {c.top_labels.slice(0, 3).map((tl: { label: string; count: number }) => (
                    <span key={tl.label} style={{ background: `${COLORS[i % COLORS.length]}10`, color: COLORS[i % COLORS.length], fontSize: 10, padding: '2px 5px', borderRadius: 3 }}>
                      {tl.label}
                    </span>
                  ))}
                </div>
              </button>
            ))}
          </div>
        </div>
      )}

    </DashboardLayout>
  );
}
