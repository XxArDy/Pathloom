import { useEffect, useMemo, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { IconX, IconCheck } from '@tabler/icons-react';
import { useApp } from '../store';
import { errorText } from '../api';
import { ago } from '../agents';
import { decideProposal } from '../agentSync';
import { lineDiff } from '../lineDiff';

/** Pending agent proposals: list, per-file line diff, approve / reject with optional comment. */
export function ProposalsDialog() {
  const { t } = useTranslation();
  const { proposals, proposalsOpen, set } = useApp();
  const [sel, setSel] = useState<string | null>(null);
  const [rejecting, setRejecting] = useState(false);
  const [comment, setComment] = useState('');
  const [busy, setBusy] = useState(false);
  const close = () => set({ proposalsOpen: false });

  const cur = proposals.find((p) => p.id === sel) ?? proposals[0];
  const curId = cur?.id;
  useEffect(() => { setRejecting(false); setComment(''); }, [curId]);
  useEffect(() => { if (proposalsOpen && proposals.length === 0) close(); });
  useEffect(() => {
    if (!proposalsOpen) return;
    const onKey = (e: KeyboardEvent) => { if (e.code === 'Escape') close(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });

  const diffs = useMemo(
    () => (cur?.changes ?? []).map((c) => ({ path: c.path, isNew: c.before == null, lines: lineDiff(c.before ?? '', c.after ?? '') })),
    [cur],
  );

  if (!proposalsOpen || !cur) return null;
  const decide = async (approve: boolean) => {
    setBusy(true);
    try {
      await decideProposal(cur.id, approve, comment.trim() || undefined);
    } catch (e) {
      set({ error: errorText(e) });
    } finally {
      setBusy(false);
    }
  };
  return (
    <div className="modal-back" onMouseDown={(e) => { if (e.target === e.currentTarget) close(); }}>
      <div className="modal prop-modal" role="dialog" aria-modal="true" aria-label={t('proposals.title')}>
        <div className="modal-head">
          <h2>{t('proposals.title')}</h2>
          <button className="icon-btn push" aria-label={t('inspector.close')} onClick={close}><IconX size={16} /></button>
        </div>
        <div className="prop-body">
          <ul className="prop-list">
            {proposals.map((p) => (
              <li key={p.id}>
                <button className={p.id === cur.id ? 'on' : ''} onClick={() => setSel(p.id)}>
                  <b>{p.author}</b> <span className="muted">{ago(p.created_at, t)}</span>
                </button>
              </li>
            ))}
          </ul>
          <div className="prop-detail">
            <div className="prop-keys">
              {cur.preview.created.length > 0 && <div>{t('proposals.creates')}: <code>{cur.preview.created.join(', ')}</code></div>}
              {cur.preview.updated.length > 0 && <div>{t('proposals.updates')}: <code>{cur.preview.updated.join(', ')}</code></div>}
            </div>
            {diffs.length === 0 && <p className="hint">{t('proposals.noChanges')}</p>}
            {diffs.map((d) => (
              <div key={d.path} className="diff">
                <div className="diff-path">{d.path}{d.isNew && <span className="muted"> · {t('proposals.newFile')}</span>}</div>
                <pre>{d.lines.map((l, i) => <div key={i} className={`dl ${l.op === '+' ? 'add' : l.op === '-' ? 'del' : ''}`}>{l.op} {l.text}</div>)}</pre>
              </div>
            ))}
            {rejecting && (
              <textarea className="prop-comment" rows={2} autoFocus value={comment} placeholder={t('proposals.comment')} onChange={(e) => setComment(e.target.value)} />
            )}
            <div className="prop-actions">
              <button className="btn pri" disabled={busy} onClick={() => decide(true)}><IconCheck size={14} />{t('proposals.approve')}</button>
              {rejecting
                ? <><button className="btn" disabled={busy} onClick={() => decide(false)}>{t('proposals.reject')}</button>
                    <button className="btn" onClick={() => setRejecting(false)}>{t('proposals.cancel')}</button></>
                : <button className="btn" disabled={busy} onClick={() => setRejecting(true)}>{t('proposals.reject')}</button>}
            </div>
          </div>
        </div>
      </div>
    </div>
  );
}
