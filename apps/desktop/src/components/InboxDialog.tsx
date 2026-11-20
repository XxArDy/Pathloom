import { useEffect } from 'react';
import { useTranslation } from 'react-i18next';
import { IconX } from '@tabler/icons-react';
import { useApp } from '../store';
import type { InboxItem } from '../api';

const KINDS: InboxItem['kind'][] = ['approval', 'decision', 'owner'];

/** "Waiting for me": approvals, decisions and owner tasks grouped by kind; a click opens the task. */
export function InboxDialog() {
  const { t } = useTranslation();
  const { inbox, inboxOpen, set } = useApp();
  const close = () => set({ inboxOpen: false });
  useEffect(() => { if (inboxOpen && inbox.length === 0) close(); });
  useEffect(() => {
    if (!inboxOpen) return;
    const onKey = (e: KeyboardEvent) => { if (e.code === 'Escape') close(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });
  if (!inboxOpen) return null;
  const open = (i: InboxItem) => set({ inboxOpen: false, selectedStage: i.stage, selectedTask: i.task_id });
  return (
    <div className="modal-back" onMouseDown={(e) => { if (e.target === e.currentTarget) close(); }}>
      <div className="modal" style={{ width: 'min(560px, 94vw)' }} role="dialog" aria-modal="true" aria-label={t('inbox.title')}>
        <div className="modal-head">
          <h2>{t('inbox.title')}</h2>
          <button className="icon-btn push" aria-label={t('inspector.close')} onClick={close}><IconX size={16} /></button>
        </div>
        <div className="inbox-body">
          {KINDS.map((k) => {
            const items = inbox.filter((i) => i.kind === k);
            if (!items.length) return null;
            return (
              <section key={k}>
                <div className="i-h"><span>{t(`inbox.kinds.${k}`)}</span><span>{items.length}</span></div>
                <ul className="inbox-list">
                  {items.map((i) => (
                    <li key={i.task_id}><button onClick={() => open(i)}><span className="tid">{i.task_id}</span> {i.title}</button></li>
                  ))}
                </ul>
              </section>
            );
          })}
        </div>
      </div>
    </div>
  );
}
