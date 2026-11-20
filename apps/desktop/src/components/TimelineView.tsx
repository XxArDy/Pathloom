import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { api, errorText, type Schedule } from '../api';
import { useApp } from '../store';
import { localDate } from '../reminders';

const day = (iso: string) => Date.UTC(+iso.slice(0, 4), +iso.slice(5, 7) - 1, +iso.slice(8, 10)) / 86_400_000;

/** Stage bars on a day axis with today line and critical path (docs/m10 §А5). Plain DOM, no chart library. */
export function TimelineView() {
  const { t } = useTranslation();
  const { stages, selectedStage, set } = useApp();
  const [sch, setSch] = useState<Schedule | null>(null);

  useEffect(() => {
    api.schedule(localDate()).then(setSch).catch((e) => set({ error: errorText(e) }));
  }, [stages, set]);

  if (!sch) return <div className="timeline" />;
  const t0 = day(sch.today);
  const span = Math.max(day(sch.finish) - t0, 7);
  const pct = (d: number) => `${(d / span) * 100}%`;
  const ticks = Array.from({ length: span + 1 }, (_, i) => i).filter((i) => span <= 14 || i % 7 === 0);
  const dateAt = (i: number) => new Date((t0 + i) * 86_400_000).toISOString().slice(5, 10);

  return (
    <div className="timeline">
      <div className="ag-head">
        <h2>{t('people.timeline.title')}</h2>
        <span className="ag-meta">{t('people.timeline.finish', { date: sch.finish })}</span>
        <span className="ag-meta"><span className="tl-key" />{t('people.timeline.legend')}</span>
      </div>
      {sch.items.length === 0 && <p className="muted">{t('people.timeline.empty')}</p>}
      <div className="tl-grid">
        <div className="tl-row tl-axis">
          <span />
          <div className="tl-track">
            {ticks.map((i) => <span key={i} className="tl-tick" style={{ left: pct(i) }}>{dateAt(i)}</span>)}
          </div>
        </div>
        {sch.items.map((i) => {
          const s = day(i.start) - t0;
          const len = day(i.finish) - day(i.start);
          const tip = i.critical ? t('people.timeline.tipCritical') : t('people.timeline.tipFloat', { count: i.float });
          const slack = i.slack == null ? null : i.slack >= 0 ? t('people.timeline.slack', { count: i.slack }) : t('people.timeline.late', { count: -i.slack });
          return (
            <div key={i.key} className={`tl-row ${selectedStage === i.key ? 'sel' : ''}`}>
              <button className="tl-label" onClick={() => set({ selectedStage: i.key, selectedTask: null })} title={i.title}>
                <span className="tid">{i.key}</span> {i.title}
              </button>
              <div className="tl-track">
                <span className="tl-today" style={{ left: pct(0) }} title={t('people.timeline.today')} />
                <button
                  className={`tl-bar ${i.critical ? 'crit' : ''} ${len === 0 ? 'zero' : ''}`}
                  style={{ left: pct(s), width: len === 0 ? 4 : pct(len) }}
                  title={tip}
                  aria-label={`${i.key}: ${tip}`}
                  onClick={() => set({ selectedStage: i.key, selectedTask: null })}
                />
                <span className={`tl-note ${i.slack != null && i.slack < 0 ? 'bad' : ''}`} style={{ left: `calc(${pct(s + len)} + 8px)` }}>
                  {t('people.timeline.days', { count: i.estimate })}{slack ? ` · ${slack}` : ''}
                </span>
              </div>
            </div>
          );
        })}
      </div>
    </div>
  );
}
