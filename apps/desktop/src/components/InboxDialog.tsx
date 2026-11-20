import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { IconX, IconChevronDown, IconChevronRight, IconExternalLink } from '@tabler/icons-react';
import { useApp } from '../store';
import type { StageView } from '@pathloom/canvas';
import type { InboxItem } from '../api';
import { ApprovalBox, DecisionBox } from './ApprovalCard';

const KINDS: InboxItem['kind'][] = ['approval', 'decision', 'owner'];

/** "Waiting for me": approvals, decisions and owner tasks grouped by kind. Approvals and decisions unfold in
 * place (decide, or write to the agent); the arrow button and owner tasks open the task on the board. */
export function InboxDialog() {
  const { t } = useTranslation();
  const { inbox, inboxOpen, stages, set } = useApp();
  const [openId, setOpenId] = useState<string | null>(null);
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
                    <InboxRow key={i.task_id} item={i} expanded={openId === i.task_id}
                      onToggle={() => setOpenId((x) => (x === i.task_id ? null : i.task_id))} onOpen={() => open(i)}
                      stage={stages.find((s) => s.key === i.stage)} />
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

function InboxRow({ item, expanded, onToggle, onOpen, stage }: {
  item: InboxItem; expanded: boolean; onToggle: () => void; onOpen: () => void; stage: StageView | undefined;
}) {
  const { t } = useTranslation();
  if (item.kind === 'owner' || !stage) {
    return <li><button onClick={onOpen}><span className="tid">{item.task_id}</span> {item.title}</button></li>;
  }
  const Chevron = expanded ? IconChevronDown : IconChevronRight;
  return (
    <li className="inbox-item">
      <div className="inbox-open">
        <button aria-expanded={expanded} onClick={onToggle}><Chevron size={13} /> <span className="tid">{item.task_id}</span> {item.title}</button>
        <button className="icon-btn" style={{ width: 'auto' }} aria-label={t('inbox.openTask')} title={t('inbox.openTask')} onClick={onOpen}><IconExternalLink size={14} /></button>
      </div>
      {expanded && (item.kind === 'approval' ? <ApprovalBox stage={stage} taskId={item.task_id} /> : <DecisionBox stage={stage} taskId={item.task_id} />)}
    </li>
  );
}
