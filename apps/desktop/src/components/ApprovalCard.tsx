import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { ask } from '@tauri-apps/plugin-dialog';
import { IconCheck, IconX, IconRefresh } from '@tabler/icons-react';
import type { StageView } from '@pathloom/canvas';
import { alreadyDecided, api, errorText, fileUrl, isImageFile } from '../api';
import { refreshInbox } from '../agentSync';
import { agentText, ago } from '../agents';
import { useApp } from '../store';
import i18n from '../i18n';

const REASONS = ['off-theme', 'duplicate', 'quality', 'scale', 'wrong-place'] as const;

export const taskKind = (stage: StageView, id: string): 'approval' | 'decision' | null =>
  stage.taskInfo?.[id]?.kind ?? (stage.key === 'APPR' ? 'approval' : stage.key === 'DECIDE' ? 'decision' : null);

/** Runs an owner action, stores the refreshed stages and the inbox; errors go to the toast. */
function useOwnerCall() {
  const { setStages, set } = useApp();
  const [busy, setBusy] = useState(false);
  const run = async (p: Promise<StageView[]>) => {
    setBusy(true);
    try { setStages(await p); await refreshInbox(); } catch (e) {
      const d = alreadyDecided(e);
      if (!d) { set({ error: errorText(e) }); return; }
      // First answer wins: show who decided and refresh the view instead of a raw error.
      set({ error: i18n.t('approval.alreadyDecided', { by: d.by, at: new Date(d.at).toLocaleString() }) });
      try { setStages(await api.stagesList()); await refreshInbox(); } catch { /* the watcher refreshes later */ }
    } finally { setBusy(false); }
  };
  return { busy, run };
}

/** Full-size image over the window; Esc or a click closes it. */
export function Lightbox({ src, onClose }: { src: string; onClose: () => void }) {
  const { t } = useTranslation();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.key === 'Escape') { e.stopPropagation(); onClose(); } };
    window.addEventListener('keydown', onKey, true);
    return () => window.removeEventListener('keydown', onKey, true);
  }, [onClose]);
  return (
    <div className="lightbox" role="dialog" aria-modal="true" aria-label={t('approval.preview')} onClick={onClose}>
      <img src={src} alt="" />
    </div>
  );
}

/** Two images: before (left of the handle) and after, with a range input as the handle. */
function BeforeAfter({ before, after, onOpen }: { before: string; after: string; onOpen: (s: string) => void }) {
  const { t } = useTranslation();
  const [pos, setPos] = useState(50);
  return (
    <div className="ba">
      <div className="ba-stage">
        <img src={fileUrl(after)} alt={t('approval.after')} onClick={() => onOpen(fileUrl(after))} />
        <img className="ba-top" src={fileUrl(before)} alt={t('approval.before')} style={{ clipPath: `inset(0 ${100 - pos}% 0 0)` }} onClick={() => onOpen(fileUrl(before))} />
        <span className="ba-line" style={{ left: `${pos}%` }} />
      </div>
      <input type="range" min={0} max={100} value={pos} aria-label={t('approval.slider')} onChange={(e) => setPos(Number(e.target.value))} />
      <div className="ba-cap"><span>{t('approval.before')}</span><span>{t('approval.after')}</span></div>
    </div>
  );
}

/** Approval of a task: state, image previews, Approve / Reject (reason chips + comment), optional re-check. */
export function ApprovalBox({ stage, taskId }: { stage: StageView; taskId: string }) {
  const { t } = useTranslation();
  const { agents, set } = useApp();
  const { busy, run } = useOwnerCall();
  const info = stage.taskInfo?.[taskId] ?? {};
  const ap = info.approval;
  const images = (info.files ?? []).filter(isImageFile);
  const ev = stage.evidence?.[taskId];
  const [rejecting, setRejecting] = useState(false);
  const [reasons, setReasons] = useState<string[]>([]);
  const [comment, setComment] = useState('');
  const [big, setBig] = useState<string | null>(null);
  const [exit, setExit] = useState<number | null>(null);
  const [checking, setChecking] = useState(false);
  useEffect(() => { setRejecting(false); setReasons([]); setComment(''); setExit(null); }, [taskId]);

  const recheck = async () => {
    if (!ev?.command) return;
    if (!(await ask(t('approval.recheckConfirm', { cmd: ev.command }), { title: t('approval.recheck'), kind: 'warning' }))) return;
    setChecking(true);
    try { setExit(await api.evidenceRecheck(taskId)); } catch (e) { set({ error: errorText(e) }); } finally { setChecking(false); }
  };
  const reject = async () => { await run(api.approvalsDecide(taskId, false, reasons, comment.trim())); setRejecting(false); };

  return (
    <div className="appr">
      {ap && (
        <div className={`appr-state s-${ap.state}`}>
          <b>{t(`approval.state.${ap.state}`)}</b>
          {ap.requested_by && ap.state === 'pending' && <span className="muted"> · {t('approval.requestedBy', { who: agentText(ap.requested_by, agents), ago: ago(ap.requested_at ?? '', t) })}</span>}
          {ap.state !== 'pending' && ap.by && <span className="muted"> · {agentText(ap.by, agents)}{ap.at ? `, ${ago(ap.at, t)}` : ''}</span>}
          {(ap.reasons ?? []).length > 0 && (
            <div className="appr-chips">{(ap.reasons ?? []).map((r) => <span key={r} className="chip on">{t(`approval.reasons.${r}`, { defaultValue: r })}</span>)}</div>
          )}
          {ap.comment && <p className="appr-comment">{ap.comment}</p>}
        </div>
      )}
      {images.length === 2 ? (
        <BeforeAfter before={images[0]} after={images[1]} onOpen={setBig} />
      ) : images.length > 0 && (
        <div className="appr-thumbs">
          {images.map((f) => <img key={f} src={fileUrl(f)} alt={f} title={f} onClick={() => setBig(fileUrl(f))} />)}
        </div>
      )}
      {ap?.state === 'pending' && (
        <>
          {rejecting ? (
            <div className="appr-reject">
              <div className="appr-chips">
                {REASONS.map((r) => (
                  <button key={r} type="button" className={`chip ${reasons.includes(r) ? 'on' : ''}`} aria-pressed={reasons.includes(r)}
                    onClick={() => setReasons((x) => (x.includes(r) ? x.filter((y) => y !== r) : [...x, r]))}>{t(`approval.reasons.${r}`)}</button>
                ))}
              </div>
              <textarea className="prop-comment" rows={2} value={comment} placeholder={t('approval.comment')} maxLength={2000} onChange={(e) => setComment(e.target.value)} />
              <div className="prop-actions">
                <button className="btn danger-solid" disabled={busy} onClick={() => void reject()}>{t('approval.confirmReject')}</button>
                <button className="btn" onClick={() => setRejecting(false)}>{t('proposals.cancel')}</button>
              </div>
            </div>
          ) : (
            <div className="prop-actions">
              <button className="btn pri" disabled={busy} onClick={() => void run(api.approvalsDecide(taskId, true, []))}><IconCheck size={14} />{t('approval.approve')}</button>
              <button className="btn" disabled={busy} onClick={() => setRejecting(true)}><IconX size={14} />{t('approval.reject')}</button>
            </div>
          )}
        </>
      )}
      {ev?.command && (
        <div className="appr-recheck">
          <button className="btn sm" disabled={checking} onClick={() => void recheck()}><IconRefresh size={13} />{checking ? t('gate.running') : t('approval.recheck')}</button>
          {exit != null && <span className={exit === 0 ? 'ok' : 'bad'}>{t('approval.exit', { code: exit })}</span>}
        </div>
      )}
      {big && <Lightbox src={big} onClose={() => setBig(null)} />}
    </div>
  );
}

/** Question to the owner: option buttons (recommended highlighted) and a free answer. */
export function DecisionBox({ stage, taskId }: { stage: StageView; taskId: string }) {
  const { t } = useTranslation();
  const { agents } = useApp();
  const { busy, run } = useOwnerCall();
  const d = stage.taskInfo?.[taskId]?.decision;
  const [own, setOwn] = useState('');
  useEffect(() => setOwn(''), [taskId]);
  if (!d) return null;
  const answer = (c: string) => void run(api.decisionAnswer(taskId, c));
  return (
    <div className="appr">
      <p className="dec-q">{d.question}</p>
      {d.asked_by && <p className="muted small">{t('decision.askedBy', { who: agentText(d.asked_by, agents) })}</p>}
      {d.choice != null ? (
        <div className="appr-state s-approved">
          <b>{t('decision.answered')}: {d.choice}</b>
          <span className="muted"> · {agentText(d.by ?? 'me', agents)}{d.at ? `, ${ago(d.at, t)}` : ''}</span>
        </div>
      ) : null}
      <div className="dec-opts">
        {d.options.map((o, i) => (
          <button key={o.label} className={`dec-opt ${i === d.recommended ? 'rec' : ''} ${d.choice === o.label ? 'chosen' : ''}`} disabled={busy} onClick={() => answer(o.label)}>
            <b>{o.label}</b>{i === d.recommended && <span className="rec-tag">{t('decision.recommended')}</span>}
            {o.description && <span className="muted small">{o.description}</span>}
          </button>
        ))}
      </div>
      <div className="td-attach">
        <input className="inline-edit" value={own} maxLength={2000} placeholder={t('decision.own')} onChange={(e) => setOwn(e.target.value)}
          onKeyDown={(e) => { if (e.key === 'Enter' && own.trim()) { answer(own.trim()); setOwn(''); } }} />
        <button className="btn" disabled={busy || !own.trim()} onClick={() => { answer(own.trim()); setOwn(''); }}>{t('decision.submit')}</button>
      </div>
    </div>
  );
}
