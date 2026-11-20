import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { api, errorText, type Agenda, type AgendaItem } from '../api';
import { useApp } from '../store';
import { localDate } from '../reminders';

/** Today / Week: overdue, due, blocked and "waiting for me" (docs/m10 §А3). */
export function AgendaView() {
  const { t } = useTranslation();
  const { stages, inbox, set } = useApp();
  const [week, setWeek] = useState(false);
  const [ag, setAg] = useState<Agenda | null>(null);

  // Refetch when the plan or the inbox changes (live reload keeps both fresh).
  useEffect(() => {
    api.agenda(localDate(), week ? 7 : 1).then(setAg).catch((e) => set({ error: errorText(e) }));
  }, [week, stages, inbox, set]);

  const open = (key: string, task?: string) => set({ selectedStage: key, selectedTask: task ?? null });
  const stageSection = (id: 'overdue' | 'due' | 'blocked', items: AgendaItem[]) => items.length > 0 && (
    <section>
      <div className="i-h"><span>{t(`agenda.${id}`)}</span><span>{items.length}</span></div>
      <ul className="inbox-list">
        {items.map((i) => (
          <li key={i.key}>
            <button onClick={() => open(i.key)}>
              <span className="tid">{i.key}</span> {i.title}
              {i.due && <span className="ag-meta">{t('agenda.due_on', { date: i.due.slice(0, 10) })}</span>}
            </button>
          </li>
        ))}
      </ul>
    </section>
  );
  const empty = ag && !ag.overdue.length && !ag.due.length && !ag.blocked.length && !ag.waiting.length;

  return (
    <div className="agenda">
      <div className="ag-head">
        <h2>{t('views.agenda')}</h2>
        <div className="views" role="tablist">
          <button role="tab" aria-selected={!week} className={`btn sm ${!week ? 'on' : ''}`} onClick={() => setWeek(false)}>{t('agenda.today')}</button>
          <button role="tab" aria-selected={week} className={`btn sm ${week ? 'on' : ''}`} onClick={() => setWeek(true)}>{t('agenda.week')}</button>
        </div>
      </div>
      {ag && stageSection('overdue', ag.overdue)}
      {ag && stageSection('due', ag.due)}
      {ag && stageSection('blocked', ag.blocked)}
      {ag && ag.waiting.length > 0 && (
        <section>
          <div className="i-h"><span>{t('agenda.waiting')}</span><span>{ag.waiting.length}</span></div>
          <ul className="inbox-list">
            {ag.waiting.map((i) => (
              <li key={i.task_id}>
                <button onClick={() => open(i.stage, i.task_id)}>
                  <span className="tid">{i.task_id}</span> {i.title}<span className="ag-meta">{t(`inbox.kinds.${i.kind}`)}</span>
                </button>
              </li>
            ))}
          </ul>
        </section>
      )}
      {empty && <p className="muted">{t('agenda.empty')}</p>}
    </div>
  );
}
