import { useState, useEffect, useMemo, useCallback } from 'react';
import { errMsg,
  getArchiveTree,
  getArchiveNode,
  createArchiveNode,
  editArchiveNode,
  deleteArchiveNode,
  importArchive,
  type ArchiveNode,
} from '@/lib/api';
import DashboardLayout from '@/components/DashboardLayout';
import { DashboardLoading } from '@/components/DashboardUI';
import { useI18nContext } from '@/i18n/I18nContext';
import type { TranslationKey } from '@/i18n/translations';
import {
  Archive as ArchiveIcon,
  Folder, FileText, Code2, ChevronRight, ChevronDown,
  Plus, Search, Upload, Pencil, Trash2, X, Layers,
} from 'lucide-react';

// ── 节点类型 → 图标 / 颜色 ──
const TYPE_META: Record<string, { icon: typeof Folder; color: string }> = {
  root: { icon: Layers, color: '#8b7ec8' },
  project: { icon: Folder, color: '#3ecfae' },
  doc: { icon: FileText, color: '#3ecfae' },
  code: { icon: Code2, color: '#3ecfae' },
};

function metaFor(type: string) {
  return TYPE_META[type] || { icon: FileText, color: '#9ca3af' };
}

// 节点类型的本地化标签
function typeLabel(type: string, t: (k: TranslationKey) => string): string {
  const map: Record<string, TranslationKey> = {
    root: 'dash.arc.typeRoot',
    project: 'dash.arc.typeProject',
    doc: 'dash.arc.typeDoc',
    code: 'dash.arc.typeCode',
  };
  return map[type] ? t(map[type]) : type;
}

// 递归查找节点
function findNode(nodes: ArchiveNode[], id: number): ArchiveNode | null {
  for (const n of nodes) {
    if (n.id === id) return n;
    if (n.children?.length) {
      const f = findNode(n.children, id);
      if (f) return f;
    }
  }
  return null;
}

// 递归过滤（用于搜索）
function filterTree(nodes: ArchiveNode[], q: string): ArchiveNode[] {
  const out: ArchiveNode[] = [];
  for (const n of nodes) {
    const hit = n.title.toLowerCase().includes(q) || (n.category || '').toLowerCase().includes(q);
    const kids = n.children?.length ? filterTree(n.children, q) : [];
    if (hit || kids.length) {
      out.push({ ...n, children: kids });
    }
  }
  return out;
}

// 统计节点总数
function countAll(nodes: ArchiveNode[]): number {
  let c = 0;
  for (const n of nodes) {
    c += 1;
    if (n.children?.length) c += countAll(n.children);
  }
  return c;
}

export default function DashboardArchive() {
  const { t } = useI18nContext();
  const [tree, setTree] = useState<ArchiveNode[]>([]);
  const [loading, setLoading] = useState(true);
  const [error, setError] = useState('');
  const [notice, setNotice] = useState('');

  const [searchQ, setSearchQ] = useState('');
  const [selectedId, setSelectedId] = useState<number | null>(null);
  const [expanded, setExpanded] = useState<Set<number>>(new Set());
  const [nodeContent, setNodeContent] = useState<string>('');
  const [loadingContent, setLoadingContent] = useState(false);

  // 弹窗
  const [showCreate, setShowCreate] = useState(false);
  const [createParentId, setCreateParentId] = useState<number | null>(null);
  const [createType, setCreateType] = useState<'project' | 'doc'>('doc');
  const [createTitle, setCreateTitle] = useState('');
  const [createCategory, setCreateCategory] = useState('');
  const [createContent, setCreateContent] = useState('');
  const [creating, setCreating] = useState(false);

  const [editingId, setEditingId] = useState<number | null>(null);
  const [editTitle, setEditTitle] = useState('');
  const [editCategory, setEditCategory] = useState('');
  const [editContent, setEditContent] = useState('');
  const [editChars, setEditChars] = useState(0);

  const [showImport, setShowImport] = useState(false);
  const [importName, setImportName] = useState('');
  const [importDocs, setImportDocs] = useState('');
  const [importing, setImporting] = useState(false);

  const [confirmDeleteId, setConfirmDeleteId] = useState<number | null>(null);

  const reload = useCallback(async () => {
    try {
      const t = await getArchiveTree();
      setTree(t);
      setError('');
    } catch (e: unknown) {
      setError(errMsg(e) || t('dash.arc.loadFail'));
    } finally {
      setLoading(false);
    }
  }, [t]);

  useEffect(() => {
    reload();
  }, [reload]);

  const totalCount = useMemo(() => countAll(tree), [tree]);

  const filteredTree = useMemo(() => {
    if (!searchQ.trim()) return tree;
    return filterTree(tree, searchQ.trim().toLowerCase());
  }, [tree, searchQ]);

  const selected = useMemo(() => (selectedId ? findNode(tree, selectedId) : null), [tree, selectedId]);

  // 自动展开选中节点的祖先路径
  const expandTo = useCallback((id: number, nodes: ArchiveNode[], acc: number[]) => {
    for (const n of nodes) {
      if (n.id === id) {
        setExpanded(prev => {
          const next = new Set(prev);
          acc.forEach(a => next.add(a));
          return next;
        });
        return true;
      }
      if (n.children?.length) {
        if (expandTo(id, n.children, [...acc, n.id])) return true;
      }
    }
    return false;
  }, []);

  function toggleExpand(id: number) {
    setExpanded(prev => {
      const next = new Set(prev);
      if (next.has(id)) next.delete(id);
      else next.add(id);
      return next;
    });
  }

  function selectNode(id: number, nodes: ArchiveNode[]) {
    setSelectedId(id);
    setNodeContent('');
    if (nodes.length) expandTo(id, nodes, []);
    // 加载节点完整内容(F3修复:取最新请求,避免旧请求覆盖新内容)
    setLoadingContent(true);
    getArchiveNode(id).then((data) => {
      setNodeContent(data?.content || '');
    }).catch(() => {
      setNodeContent('');
    }).finally(() => setLoadingContent(false));
  }

  // ── 创建 ──
  function openCreate(parentId: number | null) {
    setCreateParentId(parentId);
    setCreateType('doc');
    setCreateTitle('');
    setCreateCategory('');
    setCreateContent('');
    setShowCreate(true);
  }

  async function handleCreate() {
    if (!createTitle.trim()) return;
    setCreating(true);
    try {
      await createArchiveNode(
        createParentId ?? 0,
        createType,
        createTitle.trim(),
        createContent,
        createCategory.trim() || undefined
      );
      await reload();
      setShowCreate(false);
      setNotice(t('dash.arc.createdNotice'));
      setError('');
    } catch (e: unknown) {
      setError(errMsg(e) || t('dash.arc.createFail'));
    }
    setCreating(false);
  }

  // ── 编辑 ──
  function openEdit(node: ArchiveNode) {
    setEditingId(node.id);
    setEditTitle(node.title);
    setEditCategory(node.category || '');
    // 后端未返回 content 字段时，编辑框以空开始（避免覆盖）
    setEditContent(node.content || '');
    setEditChars(node.chars || 0);
  }

  async function handleEdit() {
    if (editingId === null) return;
    try {
      await editArchiveNode(
        editingId,
        editTitle.trim() || undefined,
        editContent || undefined,
        editCategory.trim() || undefined
      );
      await reload();
      setEditingId(null);
      setNotice(t('dash.arc.updatedNotice'));
      setError('');
    } catch (e: unknown) {
      setError(errMsg(e) || t('dash.arc.updateFail'));
    }
  }

  // ── 删除 ──
  async function handleDelete(id: number) {
    try {
      await deleteArchiveNode(id);
      if (selectedId === id) setSelectedId(null);
      await reload();
      setConfirmDeleteId(null);
      setNotice(t('dash.arc.deletedNotice'));
      setError('');
    } catch (e: unknown) {
      setError(errMsg(e) || t('dash.arc.deleteFail'));
      setConfirmDeleteId(null);
    }
  }

  // ── 导入 ──
  async function handleImport() {
    if (!importName.trim() || !importDocs.trim()) return;
    setImporting(true);
    try {
      // 简单解析：每个 "---" 分隔一份文档，首行作为 title，其余为 content；category 取文档名
      const blocks = importDocs.split(/\n-{3,}\n/).map(b => b.trim()).filter(Boolean);
      const documents = blocks.map((b, i) => {
        const lines = b.split('\n');
        const title = lines[0].trim();
        const content = lines.slice(1).join('\n').trim();
        return { title: title || `${t('dash.arc.docPrefix')}${i + 1}`, content, category: importName.trim() };
      });
      if (!documents.length) {
        setError(t('dash.arc.importParseFail'));
        setImporting(false);
        return;
      }
      await importArchive(importName.trim(), documents);
      await reload();
      setShowImport(false);
      setImportName('');
      setImportDocs('');
      setNotice(`${t('dash.arc.importedNoticePrefix')}${documents.length}${t('dash.arc.importedNoticeMiddle')}${importName.trim()}${t('dash.arc.importedNoticeSuffix')}`);
      setError('');
    } catch (e: unknown) {
      setError(errMsg(e) || t('dash.arc.importFail'));
    }
    setImporting(false);
  }

  if (loading) {
    return (
      <DashboardLayout>
        <DashboardLoading />
      </DashboardLayout>
    );
  }

  // ── 递归树节点渲染 ──
  function renderNode(node: ArchiveNode, depth: number): React.ReactElement {
    const m = metaFor(node.type);
    const Icon = m.icon;
    const isExpanded = expanded.has(node.id);
    const isSelected = selectedId === node.id;
    const hasChildren = (node.children?.length || 0) > 0;
    const paddingLeft = 8 + depth * 16;

    return (
      <div key={node.id}>
        <div
          onClick={() => selectNode(node.id, tree)}
          style={{
            display: 'flex',
            alignItems: 'center',
            gap: 6,
            padding: '6px 8px',
            paddingLeft,
            borderRadius: 8,
            cursor: 'pointer',
            background: isSelected ? 'rgba(139,126,200,0.12)' : 'transparent',
            borderLeft: isSelected ? '2px solid #8b7ec8' : '2px solid transparent',
            transition: 'background 0.15s',
          }}
          onMouseEnter={(e) => { if (!isSelected) e.currentTarget.style.background = 'rgba(255,255,255,0.03)'; }}
          onMouseLeave={(e) => { if (!isSelected) e.currentTarget.style.background = 'transparent'; }}
        >
          {hasChildren ? (
            <button
              onClick={(e) => { e.stopPropagation(); toggleExpand(node.id); }}
              style={{ background: 'none', border: 'none', cursor: 'pointer', padding: 0, color: 'var(--text-tertiary)', display: 'flex' }}
            >
              {isExpanded ? <ChevronDown size={14} /> : <ChevronRight size={14} />}
            </button>
          ) : (
            <span style={{ width: 14, display: 'inline-block' }} />
          )}
          <Icon size={14} style={{ color: m.color, flexShrink: 0 }} />
          <span
            style={{
              color: isSelected ? 'var(--text-primary)' : 'var(--text-secondary)',
              fontSize: 13,
              overflow: 'hidden',
              textOverflow: 'ellipsis',
              whiteSpace: 'nowrap',
              flex: 1,
            }}
            title={node.title}
          >
            {node.title}
          </span>
          {hasChildren && (
            <span style={{ color: 'var(--text-tertiary)', fontSize: 10, fontFamily: 'var(--font-mono)' }}>
              {node.children!.length}
            </span>
          )}
        </div>
        {hasChildren && isExpanded && (
          <div>{node.children!.map((c) => renderNode(c, depth + 1))}</div>
        )}
      </div>
    );
  }

  return (
    <DashboardLayout>
      {/* 标题 */}
      <div style={{ marginBottom: 20 }}>
        <p style={{ fontFamily: 'var(--font-mono)', fontSize: 11, color: 'var(--accent-cyan)', letterSpacing: '0.16em', marginBottom: 10 }}>ARCHIVE</p>
        <h1 style={{ color: 'var(--text-primary)', fontSize: 'clamp(26px, 3.5vw, 36px)', fontWeight: 700, fontFamily: 'var(--font-display)', letterSpacing: '-0.025em', marginBottom: 4, display: 'flex', alignItems: 'center', gap: 8 }}>
          <ArchiveIcon size={24} style={{ color: 'var(--accent-purple)' }} />
          {t('dash.arc.title')}
        </h1>
        <p style={{ color: 'var(--text-secondary)', fontSize: 14 }}>{totalCount} {t('dash.arc.nodesSuffix')}</p>
      </div>

      {error && (
        <div style={{ background: 'rgba(248,113,113,0.08)', color: 'var(--danger-red)', border: '1px solid rgba(248,113,113,0.15)', borderRadius: 10, padding: 12, marginBottom: 16, fontSize: 13, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          {error}
          <button onClick={() => setError('')} style={{ color: 'var(--danger-red)', background: 'none', border: 'none', cursor: 'pointer' }}><X size={14} /></button>
        </div>
      )}
      {notice && (
        <div style={{ background: 'rgba(52,211,153,0.06)', color: 'var(--success-green)', border: '1px solid rgba(52,211,153,0.12)', borderRadius: 10, padding: 12, marginBottom: 16, fontSize: 13, display: 'flex', justifyContent: 'space-between', alignItems: 'center' }}>
          {notice}
          <button onClick={() => setNotice('')} style={{ color: 'var(--success-green)', background: 'none', border: 'none', cursor: 'pointer' }}><X size={14} /></button>
        </div>
      )}

      {/* 顶部操作栏 */}
      <div style={{ display: 'flex', gap: 8, marginBottom: 14, flexWrap: 'wrap', alignItems: 'center' }}>
        <div style={{ position: 'relative', flex: '1 1 240px', maxWidth: 360 }}>
          <Search size={14} style={{ position: 'absolute', left: 12, top: '50%', transform: 'translateY(-50%)', color: 'var(--text-tertiary)' }} />
          <input
            type="text"
            value={searchQ}
            onChange={(e) => setSearchQ(e.target.value)}
            placeholder={t('dash.arc.searchPlaceholder')}
            style={{ width: '100%', background: 'rgba(255,255,255,0.04)', color: 'var(--text-primary)', border: '1px solid var(--border-light)', borderRadius: 8, padding: '8px 12px 8px 34px', fontSize: 13, boxSizing: 'border-box' }}
          />
        </div>
        <button onClick={() => openCreate(null)} style={{ background: 'var(--accent-purple)', color: '#fff', border: 'none', padding: '8px 14px', borderRadius: 8, cursor: 'pointer', fontSize: 13, display: 'flex', alignItems: 'center', gap: 4 }}>
          <Plus size={14} /> {t('dash.arc.newBtn')}
        </button>
        <button onClick={() => setShowImport(true)} style={{ background: 'rgba(255,255,255,0.04)', color: 'var(--text-secondary)', border: '1px solid var(--border-light)', padding: '8px 14px', borderRadius: 8, cursor: 'pointer', fontSize: 13, display: 'flex', alignItems: 'center', gap: 4 }}>
          <Upload size={14} /> {t('dash.arc.importBtn')}
        </button>
      </div>

      {/* 三栏布局 */}
      <div style={{ display: 'grid', gridTemplateColumns: '280px 1fr', gap: 12, minHeight: '60vh' }}>
        {/* 左栏：树 */}
        <div style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)', borderRadius: 14, padding: 8, overflow: 'auto', maxHeight: '75vh' }}>
          {filteredTree.length === 0 ? (
            <div style={{ textAlign: 'center', padding: 24, color: 'var(--text-tertiary)', fontSize: 13 }}>
              {searchQ ? t('dash.arc.noMatch') : t('dash.arc.empty')}
            </div>
          ) : (
            <div style={{ display: 'flex', flexDirection: 'column', gap: 1 }}>
              {filteredTree.map((n) => renderNode(n, 0))}
            </div>
          )}
        </div>

        {/* 中栏：内容 */}
        <div style={{ background: 'var(--bg-card)', border: '1px solid var(--border-light)', borderRadius: 14, padding: 20, overflow: 'auto', maxHeight: '75vh' }}>
          {!selected ? (
            <div style={{ display: 'flex', flexDirection: 'column', alignItems: 'center', justifyContent: 'center', height: '100%', minHeight: 320, color: 'var(--text-tertiary)' }}>
              <ArchiveIcon size={40} style={{ opacity: 0.3, marginBottom: 12 }} />
              <div style={{ fontSize: 14 }}>{t('dash.arc.selectHint')}</div>
            </div>
          ) : (
            <NodeDetail
              node={selected}
              content={nodeContent}
              loadingContent={loadingContent}
              t={t}
              onEdit={() => openEdit(selected)}
              onDelete={() => setConfirmDeleteId(selected.id)}
              onAddChild={() => openCreate(selected.id)}
              onSelectChild={(id) => selectNode(id, tree)}
            />
          )}
        </div>
      </div>

      {/* 新建弹窗 */}
      {showCreate && (
        <Modal title={t('dash.arc.createTitle')} onClose={() => setShowCreate(false)}>
          <Field label={t('dash.arc.fieldType')}>
            <div style={{ display: 'flex', gap: 8 }}>
              {(['project', 'doc'] as const).map((tp) => {
                const m = metaFor(tp);
                const Icon = m.icon;
                const active = createType === tp;
                return (
                  <button
                    key={tp}
                    onClick={() => setCreateType(tp)}
                    style={{
                      flex: 1, padding: '10px', borderRadius: 8, cursor: 'pointer', fontSize: 13,
                      display: 'flex', alignItems: 'center', justifyContent: 'center', gap: 6,
                      background: active ? `${m.color}1a` : 'rgba(255,255,255,0.03)',
                      border: `1px solid ${active ? `${m.color}55` : 'var(--border-light)'}`,
                      color: active ? m.color : 'var(--text-secondary)',
                    }}
                  >
                    <Icon size={14} /> {typeLabel(tp, t)}
                  </button>
                );
              })}
            </div>
          </Field>
          <Field label={t('dash.arc.fieldTitle')}>
            <input
              type="text"
              value={createTitle}
              onChange={(e) => setCreateTitle(e.target.value)}
              placeholder={createType === 'project' ? t('dash.arc.projectNamePh') : t('dash.arc.docTitlePh')}
              style={inputStyle}
            />
          </Field>
          <Field label={t('dash.arc.fieldCategoryOpt')}>
            <input
              type="text"
              value={createCategory}
              onChange={(e) => setCreateCategory(e.target.value)}
              placeholder={t('dash.arc.categoryExamplePh')}
              style={inputStyle}
            />
          </Field>
          {createType === 'doc' && (
            <Field label={t('dash.arc.fieldContentOpt')}>
              <textarea
                value={createContent}
                onChange={(e) => setCreateContent(e.target.value)}
                placeholder={t('dash.arc.docBodyPh')}
                style={{ ...inputStyle, minHeight: 120, resize: 'vertical', fontFamily: 'var(--font-mono)', fontSize: 12 }}
              />
            </Field>
          )}
          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 4 }}>
            <button onClick={() => setShowCreate(false)} style={ghostBtnStyle}>{t('dash.arc.cancelBtn')}</button>
            <button onClick={handleCreate} disabled={creating || !createTitle.trim()} style={primaryBtnStyle}>
              {creating ? t('dash.arc.creatingBtn') : t('dash.arc.createBtn')}
            </button>
          </div>
        </Modal>
      )}

      {/* 编辑弹窗 */}
      {editingId !== null && (
        <Modal title={t('dash.arc.editTitle')} onClose={() => setEditingId(null)}>
          <Field label={t('dash.arc.fieldTitle')}>
            <input type="text" value={editTitle} onChange={(e) => setEditTitle(e.target.value)} style={inputStyle} />
          </Field>
          <Field label={t('dash.arc.fieldCategory')}>
            <input type="text" value={editCategory} onChange={(e) => setEditCategory(e.target.value)} placeholder={t('dash.arc.categoryPh')} style={inputStyle} />
          </Field>
          <Field label={editChars > 0 ? `${t('dash.arc.fieldContent')}${t('dash.arc.contentCurrentPrefix')}${editChars}${t('dash.arc.contentCurrentMiddle')}${t('dash.arc.contentKeepHint')}` : t('dash.arc.fieldContent')}>
            <textarea
              value={editContent}
              onChange={(e) => setEditContent(e.target.value)}
              placeholder={t('dash.arc.editBodyPh')}
              style={{ ...inputStyle, minHeight: 140, resize: 'vertical', fontFamily: 'var(--font-mono)', fontSize: 12 }}
            />
          </Field>
          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 4 }}>
            <button onClick={() => setEditingId(null)} style={ghostBtnStyle}>{t('dash.arc.cancelBtn')}</button>
            <button onClick={handleEdit} style={primaryBtnStyle}>{t('dash.arc.saveBtn')}</button>
          </div>
        </Modal>
      )}

      {/* 导入弹窗 */}
      {showImport && (
        <Modal title={t('dash.arc.batchImportTitle')} onClose={() => setShowImport(false)}>
          <Field label={t('dash.arc.fieldProjectName')}>
            <input type="text" value={importName} onChange={(e) => setImportName(e.target.value)} placeholder={t('dash.arc.importProjectPh')} style={inputStyle} />
          </Field>
          <Field label={t('dash.arc.fieldDocs')}>
            <textarea
              value={importDocs}
              onChange={(e) => setImportDocs(e.target.value)}
              placeholder={t('dash.arc.importDocsPh')}
              style={{ ...inputStyle, minHeight: 180, resize: 'vertical', fontFamily: 'var(--font-mono)', fontSize: 12 }}
            />
          </Field>
          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 4 }}>
            <button onClick={() => setShowImport(false)} style={ghostBtnStyle}>{t('dash.arc.cancelBtn')}</button>
            <button onClick={handleImport} disabled={importing || !importName.trim() || !importDocs.trim()} style={primaryBtnStyle}>
              {importing ? t('dash.arc.importingBtn') : t('dash.arc.importBtn')}
            </button>
          </div>
        </Modal>
      )}

      {/* 删除确认 */}
      {confirmDeleteId !== null && (
        <Modal title={t('dash.arc.deleteConfirmTitle')} onClose={() => setConfirmDeleteId(null)}>
          <p style={{ color: 'var(--text-secondary)', fontSize: 13, lineHeight: 1.6 }}>
            {t('dash.arc.deleteConfirmMsg')}
            <br />
            <span style={{ color: 'var(--danger-red)' }}>{t('dash.arc.deleteIrreversible')}</span>
          </p>
          <div style={{ display: 'flex', justifyContent: 'flex-end', gap: 8, marginTop: 12 }}>
            <button onClick={() => setConfirmDeleteId(null)} style={ghostBtnStyle}>{t('dash.arc.cancelBtn')}</button>
            <button onClick={() => handleDelete(confirmDeleteId)} style={{ ...primaryBtnStyle, background: 'var(--danger-red)' }}>
              {t('dash.arc.deleteBtn')}
            </button>
          </div>
        </Modal>
      )}
    </DashboardLayout>
  );
}

// ── 中栏：节点详情 ──
function NodeDetail({ node, content, loadingContent, t, onEdit, onDelete, onAddChild, onSelectChild }: {
  node: ArchiveNode;
  content: string;
  loadingContent: boolean;
  t: (k: TranslationKey) => string;
  onEdit: () => void;
  onDelete: () => void;
  onAddChild: () => void;
  onSelectChild: (id: number) => void;
}) {
  const m = metaFor(node.type);
  const Icon = m.icon;
  const isProjectLike = node.type === 'project' || node.type === 'root';
  const kids = node.children || [];

  return (
    <div>
      {/* 头部 */}
      <div style={{ display: 'flex', alignItems: 'flex-start', justifyContent: 'space-between', gap: 12, marginBottom: 16, flexWrap: 'wrap' }}>
        <div style={{ display: 'flex', alignItems: 'center', gap: 10, flex: 1, minWidth: 0 }}>
          <div style={{ width: 36, height: 36, borderRadius: 10, display: 'flex', alignItems: 'center', justifyContent: 'center', background: `${m.color}15`, flexShrink: 0 }}>
            <Icon size={18} style={{ color: m.color }} />
          </div>
          <div style={{ minWidth: 0 }}>
            <h2 style={{ color: 'var(--text-primary)', fontSize: 18, fontWeight: 600, marginBottom: 2, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>
              {node.title}
            </h2>
            <div style={{ display: 'flex', gap: 10, alignItems: 'center', flexWrap: 'wrap', color: 'var(--text-tertiary)', fontSize: 11 }}>
              <span style={{ background: `${m.color}12`, color: m.color, padding: '1px 6px', borderRadius: 4 }}>{typeLabel(node.type, t)}</span>
              {node.category && <span>{t('dash.arc.categoryLabel')}{node.category}</span>}
              {node.chars > 0 && <span>{node.chars} {t('dash.arc.charsSuffix')}</span>}
              {node.timestamp > 0 && <span>{new Date(node.timestamp * 1000).toLocaleString('zh-CN')}</span>}
              {node.status && node.status !== 'active' && (
                <span style={{ color: node.status === 'archived' ? 'var(--text-tertiary)' : 'var(--warning-orange)' }}>{node.status}</span>
              )}
            </div>
          </div>
        </div>
        <div style={{ display: 'flex', gap: 6, flexShrink: 0 }}>
          {isProjectLike && (
            <button onClick={onAddChild} title={t('dash.arc.addChildTitle')} style={{ ...ghostBtnStyle, padding: '6px 10px' }}>
              <Plus size={13} style={{ verticalAlign: -1, marginRight: 3 }} /> {t('dash.arc.addBtn')}
            </button>
          )}
          <button onClick={onEdit} title={t('dash.arc.editBtn')} style={{ background: 'rgba(255,255,255,0.04)', border: '1px solid var(--border-light)', color: 'var(--text-secondary)', padding: 7, borderRadius: 8, cursor: 'pointer', display: 'flex' }}>
            <Pencil size={13} />
          </button>
          <button onClick={onDelete} title={t('dash.arc.deleteBtn')} style={{ background: 'rgba(248,113,113,0.06)', border: '1px solid rgba(248,113,113,0.12)', color: 'var(--danger-red)', padding: 7, borderRadius: 8, cursor: 'pointer', display: 'flex' }}>
            <Trash2 size={13} />
          </button>
        </div>
      </div>

      {/* 内容区域 */}
      {isProjectLike ? (
        // 项目/根：展示子文档列表
        kids.length === 0 ? (
          <div style={{ textAlign: 'center', padding: 40, color: 'var(--text-tertiary)', fontSize: 13, background: 'rgba(255,255,255,0.02)', borderRadius: 12, border: '1px dashed var(--border-light)' }}>
            {t('dash.arc.noChildrenHint')}
          </div>
        ) : (
          <div style={{ display: 'flex', flexDirection: 'column', gap: 6 }}>
            {kids.map((c) => {
              const cm = metaFor(c.type);
              const CIcon = cm.icon;
              return (
                <div
                  key={c.id}
                  onClick={() => onSelectChild(c.id)}
                  style={{
                    display: 'flex', alignItems: 'center', gap: 10, padding: '10px 12px',
                    background: 'rgba(255,255,255,0.02)', border: '1px solid var(--border-light)',
                    borderRadius: 10, cursor: 'pointer', transition: 'all 0.15s',
                  }}
                  onMouseEnter={(e) => { e.currentTarget.style.background = 'rgba(139,126,200,0.06)'; e.currentTarget.style.borderColor = 'rgba(139,126,200,0.2)'; }}
                  onMouseLeave={(e) => { e.currentTarget.style.background = 'rgba(255,255,255,0.02)'; e.currentTarget.style.borderColor = 'var(--border-light)'; }}
                >
                  <CIcon size={15} style={{ color: cm.color, flexShrink: 0 }} />
                  <div style={{ flex: 1, minWidth: 0 }}>
                    <div style={{ color: 'var(--text-primary)', fontSize: 13, fontWeight: 500, overflow: 'hidden', textOverflow: 'ellipsis', whiteSpace: 'nowrap' }}>{c.title}</div>
                    <div style={{ color: 'var(--text-tertiary)', fontSize: 11 }}>
                      {c.category && <span>{c.category} · </span>}
                      {c.chars > 0 ? `${c.chars} ${t('dash.arc.charsSuffix')}` : t('dash.arc.emptyDoc')}
                      {c.children_count > 0 && ` · ${c.children_count} ${t('dash.arc.childrenSuffix')}`}
                    </div>
                  </div>
                  <ChevronRight size={14} style={{ color: 'var(--text-tertiary)' }} />
                </div>
              );
            })}
          </div>
        )
      ) : (
        // 文档/代码：展示内容
        loadingContent ? (
          <div style={{ textAlign: 'center', padding: 40, color: 'var(--text-tertiary)', fontSize: 13 }}>
            {t('dash.arc.loading')}
          </div>
        ) : content ? (
          <pre style={{
            color: 'var(--text-secondary)', fontSize: 13, lineHeight: 1.7,
            whiteSpace: 'pre-wrap', wordBreak: 'break-word',
            background: 'rgba(0,0,0,0.25)', padding: 16, borderRadius: 12,
            border: '1px solid var(--border-light)',
            fontFamily: node.type === 'code' ? 'var(--font-mono)' : 'var(--font-body)',
            margin: 0,
          }}>
            {content}
          </pre>
        ) : (
          <div style={{ textAlign: 'center', padding: 40, color: 'var(--text-tertiary)', fontSize: 13, background: 'rgba(255,255,255,0.02)', borderRadius: 12, border: '1px dashed var(--border-light)' }}>
            {t('dash.arc.noContentHint')}
          </div>
        )
      )}
    </div>
  );
}

// ── 弹窗 / 表单辅助组件 ──
function Modal({ title, onClose, children }: { title: string; onClose: () => void; children: React.ReactNode }) {
  return (
    <div
      onClick={onClose}
      style={{
        position: 'fixed', inset: 0, zIndex: 100,
        background: 'rgba(0,0,0,0.6)', backdropFilter: 'blur(4px)',
        display: 'flex', alignItems: 'center', justifyContent: 'center', padding: 20,
      }}
    >
      <div
        onClick={(e) => e.stopPropagation()}
        style={{
          background: 'var(--bg-card-solid)', border: '1px solid var(--border-medium)',
          borderRadius: 16, padding: 20, width: '100%', maxWidth: 520,
          maxHeight: '85vh', overflow: 'auto',
          boxShadow: '0 20px 60px rgba(0,0,0,0.5)',
        }}
      >
        <div style={{ display: 'flex', justifyContent: 'space-between', alignItems: 'center', marginBottom: 16 }}>
          <h3 style={{ color: 'var(--text-primary)', fontSize: 16, fontWeight: 600, margin: 0 }}>{title}</h3>
          <button onClick={onClose} style={{ color: 'var(--text-tertiary)', background: 'none', border: 'none', cursor: 'pointer', padding: 0 }}>
            <X size={18} />
          </button>
        </div>
        <div style={{ display: 'flex', flexDirection: 'column', gap: 12 }}>
          {children}
        </div>
      </div>
    </div>
  );
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <div>
      <div style={{ color: 'var(--text-tertiary)', fontSize: 11, textTransform: 'uppercase', letterSpacing: '0.05em', marginBottom: 5 }}>{label}</div>
      {children}
    </div>
  );
}

const inputStyle: React.CSSProperties = {
  width: '100%',
  background: 'rgba(0,0,0,0.3)',
  color: 'var(--text-primary)',
  border: '1px solid var(--border-light)',
  borderRadius: 8,
  padding: 10,
  fontSize: 13,
  boxSizing: 'border-box',
  outline: 'none',
};

const primaryBtnStyle: React.CSSProperties = {
  background: 'var(--accent-purple)',
  color: '#fff',
  border: 'none',
  padding: '8px 16px',
  borderRadius: 8,
  cursor: 'pointer',
  fontSize: 13,
};

const ghostBtnStyle: React.CSSProperties = {
  background: 'rgba(255,255,255,0.04)',
  color: 'var(--text-secondary)',
  border: '1px solid var(--border-light)',
  padding: '8px 16px',
  borderRadius: 8,
  cursor: 'pointer',
  fontSize: 13,
};
