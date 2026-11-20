import { useTranslation } from 'react-i18next';
import { IconCopy, IconExternalLink, IconLink } from '@tabler/icons-react';
import type { StageView } from '@pathloom/canvas';
import { api, errorText } from '../api';
import { agentText, ago } from '../agents';
import { useApp } from '../store';

type Session = NonNullable<StageView['sessions']>[number];

/** Colored dot + label of the session's live state; the tooltip says how long ago it changed. */
export function SessionState({ s }: { s: Session }) {
  const { t } = useTranslation();
  if (!s.state) return null;
  const when = s.state_at ? ago(s.state_at, t) : '';
  return (
    <span className={`sess-state st-${s.state}`} title={when ? t('sessions.since', { ago: when }) : undefined}>
      <i />{t(`sessions.state.${s.state}`)}
    </span>
  );
}

/** One session of a stage: client, title, live state, copy id, open (url). */
export function SessionRow({ s, onOpen }: { s: Session; onOpen: (url: string) => void }) {
  const { t } = useTranslation();
  const agents = useApp((st) => st.agents);
  return (
    <li>
      <span className="who">{agentText(s.client, agents)}</span>
      <span className="grow" title={s.id}>{s.title || s.id}</span>
      <SessionState s={s} />
      <button className="icon-btn" title={t('sessions.copy')} aria-label={t('sessions.copy')} onClick={() => void navigator.clipboard.writeText(s.id)}><IconCopy size={13} /></button>
      {s.url && <button className="icon-btn" title={t('sessions.open')} aria-label={t('sessions.open')} onClick={() => onOpen(s.url as string)}><IconExternalLink size={13} /></button>}
    </li>
  );
}

/** Copies a `pathloom://` link to the project, a stage key or a task id. */
export function CopyLink({ id }: { id?: string }) {
  const { t } = useTranslation();
  const copy = async () => {
    try { await navigator.clipboard.writeText(await api.linkMake(id)); } catch (e) { useApp.getState().set({ error: errorText(e) }); }
  };
  return <button className="icon-btn" title={t('link.copy')} aria-label={t('link.copy')} onClick={() => void copy()}><IconLink size={14} /></button>;
}
