// Live agent status pieces: task pipeline, commits and cost of a stage, active agents and usage meter (docs/m9 §4-6).
import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { resolveAgent, type StageView } from '@pathloom/canvas';
import { api, fileUrl, isImageFile, type CommitRow, type UsageSummary } from '../api';
import { agentText, ago, type ClaimView } from '../agents';
import { useApp } from '../store';
import { findTheme } from '../theme';
import { Lightbox } from './ApprovalCard';

const STEPS = ['claimed', 'pushed', 'merged', 'verified', 'accepted'] as const;

/** HEAD of the project's git repo, refreshed with the activity log (null without git). */
export function useHead(): string | null {
  const { project, activity } = useApp();
  const [head, setHead] = useState<string | null>(null);
  const root = project?.root;
  useEffect(() => { if (root) api.gitHead().then(setHead).catch(() => setHead(null)); else setHead(null); }, [root, activity]);
  return head;
}

/** Evidence was made on another commit than HEAD (prefix compare either way). */
export const evidenceStale = (commit: string | undefined, head: string | null) =>
  !!commit && !!head && !(head.startsWith(commit) || commit.startsWith(head));

/** "parent → by (branch)", `branch@sha7`, 5-step pipeline and evidence thumbnails of a task. */
export function TaskLive({ stage, taskId, claim }: { stage: StageView; taskId: string; claim?: ClaimView }) {
  const { t } = useTranslation();
  const { agents } = useApp();
  const [big, setBig] = useState<string | null>(null);
  const st = stage.taskInfo?.[taskId]?.status;
  const step = st?.step ?? (claim ? 'claimed' : undefined);
  const branch = st?.branch ?? claim?.branch;
  const sha = st?.sha;
  const chain = claim && (claim.parent || claim.by)
    ? [claim.parent && agentText(claim.parent, agents), (claim.by ? agentText(claim.by, agents) : agentText(claim.agent, agents)) + (claim.branch ? ` (${claim.branch})` : '')].filter(Boolean).join(' → ')
    : '';
  if (!step && !chain) return null;
  const imgs = (st?.evidence ?? []).filter(isImageFile);
  return (
    <div className="td-live">
      {chain && <p className="muted small mono">{chain}</p>}
      {(branch || sha) && <p className="muted small mono">{[branch, sha && sha.slice(0, 7)].filter(Boolean).join('@')}</p>}
      {step && (
        <ol className="pipe" aria-label={t('live.pipeline')}>
          {STEPS.map((s, i) => (
            <li key={s} className={i < STEPS.indexOf(step) ? 'past' : s === step ? 'now' : ''} aria-current={s === step ? 'step' : undefined}>
              {t(`live.step.${s}`)}
            </li>
          ))}
        </ol>
      )}
      {imgs.length > 0 && (
        <div className="thumbs">
          {imgs.map((f) => <img key={f} src={fileUrl(f)} alt={f} title={f} onClick={() => setBig(fileUrl(f))} />)}
        </div>
      )}
      {big && <Lightbox src={big} onClose={() => setBig(null)} />}
    </div>
  );
}

const cache = new Map<string, CommitRow[]>();

/** Commits that mention the stage; loads when opened, cached per project and HEAD. */
export function StageCommits({ stageKey }: { stageKey: string }) {
  const { t } = useTranslation();
  const { project } = useApp();
  const head = useHead();
  const [open, setOpen] = useState(false);
  const [rows, setRows] = useState<CommitRow[] | null>(null);
  const ck = `${project?.root}|${head}|${stageKey}`;
  useEffect(() => {
    if (!open) return;
    const hit = cache.get(ck);
    if (hit) { setRows(hit); return; }
    setRows(null);
    let alive = true;
    api.stageCommits(stageKey).then((r) => { cache.set(ck, r); if (alive) setRows(r); }).catch(() => { if (alive) setRows([]); });
    return () => { alive = false; };
  }, [open, ck, stageKey]);
  return (
    <details className="commits" onToggle={(e) => setOpen(e.currentTarget.open)}>
      <summary className="i-h"><span>{t('live.commits')}</span>{rows && <span>{rows.length}</span>}</summary>
      {open && rows && rows.length === 0 && <p className="muted small">{t('live.noCommits')}</p>}
      <ul className="sessions">
        {(rows ?? []).map((c) => (
          <li key={c.sha} className="commit">
            <span className="mono muted">{c.sha.slice(0, 7)}</span>
            <span className="grow" title={c.subject}>{c.subject}</span>
            {c.tasks.map((k) => <span key={k} className="dep">{k}</span>)}
            <span className="muted small" title={c.author}>{c.author} · {ago(c.at, t)}</span>
          </li>
        ))}
      </ul>
    </details>
  );
}

const fmtTokens = (n: number) => (n >= 1e6 ? `${(n / 1e6).toFixed(1)}M` : n >= 1e3 ? `${Math.round(n / 1e3)}k` : String(n));
const fmtUsage = (u: { tokens_in: number; tokens_out: number; cost_usd?: number }) =>
  `${fmtTokens(u.tokens_in + u.tokens_out)}${u.cost_usd != null ? ` · $${u.cost_usd.toFixed(2)}` : ''}`;

/** Usage summary of the project, refreshed with the activity log. */
function useUsage(): UsageSummary | null {
  const { activity, project } = useApp();
  const [u, setU] = useState<UsageSummary | null>(null);
  useEffect(() => { if (project) api.usageSummary().then(setU).catch(() => setU(null)); }, [activity, project]);
  return u;
}

/** Tokens and cost of a stage with a per-model breakdown (hidden when nothing was reported). */
export function StageCost({ stageKey }: { stageKey: string }) {
  const { t } = useTranslation();
  const u = useUsage();
  const total = u?.by_stage[stageKey];
  if (!total) return null;
  return (
    <>
      <div className="i-h"><span>{t('live.cost')}</span><span>{fmtUsage(total)}</span></div>
      {Object.entries(u?.by_stage_model?.[stageKey] ?? {}).map(([m, v]) => (
        <div key={m} className="ov-row"><span className="grow mono">{m}</span><span className="muted">{fmtUsage(v)}</span></div>
      ))}
    </>
  );
}

/** Header: who is working on which task right now (click selects the task). */
export function ActiveAgents() {
  const { t } = useTranslation();
  const { stages, agents, theme, customThemes, set } = useApp();
  const tokens = findTheme(theme, customThemes).tokens;
  const live = stages.flatMap((s) => (s.claims ?? []).map((c) => ({ s, c })));
  if (live.length === 0) return null;
  return (
    <div className="active-agents" aria-label={t('live.active')}>
      {live.map(({ s, c }) => {
        const r = resolveAgent(c.agent, agents, tokens);
        const color = c.stale ? 'var(--bad)' : c.silent ? 'var(--warn)' : r.color;
        return (
          <button key={`${c.agent}:${c.taskId}`} className="agent-pill" style={{ color, borderColor: color }}
            title={`${r.name} · ${c.taskId}${c.stale ? ` · ${t('live.stale')}` : ''}`}
            onClick={() => set({ selectedStage: s.key, selectedTask: c.taskId })}>
            {r.avatar && r.avatar.length <= 4 ? r.avatar : <span className="dot" style={{ background: color }} />}
            {r.name} <b className="mono">{c.taskId}</b>
          </button>
        );
      })}
    </div>
  );
}

/** Header: highest weekly limit percent among agents; yellow from the threshold. Hidden without quota. */
export function UsageMeter() {
  const { t } = useTranslation();
  const { quotaWarn, agents } = useApp();
  const q = Object.entries(useUsage()?.quota ?? {});
  if (q.length === 0) return null;
  const max = Math.max(...q.map(([, v]) => v.percent_weekly));
  const tip = q.map(([a, v]) => `${agentText(a, agents)}: ${Math.round(v.percent_weekly)}%${v.resets_at ? ` · ${t('live.resets', { at: new Date(v.resets_at).toLocaleString() })}` : ''}`).join('\n');
  return <span className={`usage-meter ${max >= quotaWarn ? 'warn' : ''}`} title={tip}>{t('live.limit', { percent: Math.round(max) })}</span>;
}

/** Header: "origin/main @abc1234" when the board is read from a git ref, plus the owner-action queue size. */
export function GitBadge() {
  const { t } = useTranslation();
  const git = useApp((s) => s.git);
  if (!git?.readRef) return null;
  const tip = [t('git.hint', { ref: git.readRef }), git.error ? t('git.error', { error: git.error }) : ''].filter(Boolean).join(String.fromCharCode(10));
  return (
    <>
      <span className={`usage-meter ${git.error ? 'warn' : ''}`} title={tip}>{git.readRef}{git.sha ? ` @${git.sha.slice(0, 7)}` : ''}</span>
      {git.queued > 0 && <span className="usage-meter warn" title={t('git.queuedHint')}>{t('git.queued', { count: git.queued })}</span>}
    </>
  );
}
