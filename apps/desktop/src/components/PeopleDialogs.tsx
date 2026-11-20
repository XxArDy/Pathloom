import { useEffect, useRef, useState, type ReactNode } from 'react';
import { useTranslation } from 'react-i18next';
import { IconX } from '@tabler/icons-react';
import { open, save } from '@tauri-apps/plugin-dialog';
import { api, errorText, type ExportFormat, type HistoryRow, type ImportFormat, type ImportPreview, type QuickDraft } from '../api';
import { useApp } from '../store';
import { localDate } from '../reminders';

type Msg = { bad: boolean; text: string } | null;

/** Reload boards and the open board after files changed outside the canvas (import, revert). */
async function refreshBoards() {
  const st = useApp.getState();
  st.set({ boards: await api.boardsList() });
  const now = useApp.getState();
  const b = now.activeBoard ?? now.boards[0];
  if (b) await now.openBoard(b);
}

function Modal({ title, onClose, children }: { title: string; onClose: () => void; children: ReactNode }) {
  const { t } = useTranslation();
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => { if (e.code === 'Escape') onClose(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });
  return (
    <div className="modal-back" onMouseDown={(e) => { if (e.target === e.currentTarget) onClose(); }}>
      <div className="modal" style={{ width: 'min(560px, 94vw)' }} role="dialog" aria-modal="true" aria-label={title}>
        <div className="modal-head">
          <h2>{title}</h2>
          <button className="icon-btn push" aria-label={t('inspector.close')} onClick={onClose}><IconX size={16} /></button>
        </div>
        <div className="people-body">{children}</div>
      </div>
    </div>
  );
}

const Note = ({ msg }: { msg: Msg }) => msg && <p className={msg.bad ? 'err' : 'muted'} role="alert">{msg.text}</p>;

function QuickAdd({ close }: { close: () => void }) {
  const { t } = useTranslation();
  const { stages, setStages } = useApp();
  const [text, setText] = useState('');
  const [draft, setDraft] = useState<QuickDraft | null>(null);
  const [title, setTitle] = useState('');
  const [stage, setStage] = useState('');
  const [due, setDue] = useState('');
  const [msg, setMsg] = useState<Msg>(null);
  const seq = useRef(0);

  useEffect(() => {
    const n = ++seq.current;
    if (!text.trim()) { setDraft(null); return; }
    const timer = setTimeout(() => {
      api.quickAddParse(text, localDate()).then((d) => {
        if (n !== seq.current) return;
        setDraft(d); setTitle(d.title); setStage(d.stage ?? ''); setDue(d.due ?? ''); setMsg(null);
      }).catch((e) => setMsg({ bad: true, text: errorText(e) }));
    }, 150);
    return () => clearTimeout(timer);
  }, [text]);

  async function add() {
    try {
      const r = await api.quickAddApply(title.trim(), stage, due || null);
      setStages(r.stages);
      if (r.note) { setMsg({ bad: false, text: r.note }); setText(''); } else close();
    } catch (e) {
      setMsg({ bad: true, text: errorText(e) });
    }
  }

  return (
    <Modal title={t('people.quick.title')} onClose={close}>
      <form className="tpl-form" onSubmit={(e) => { e.preventDefault(); if (title.trim() && stage) void add(); }}>
        <input autoFocus value={text} placeholder={t('people.quick.placeholder')} onChange={(e) => setText(e.target.value)} />
        <p className="muted">{t('people.quick.hint')}</p>
        {draft && (
          <dl className="people-fields">
            <dt>{t('people.quick.fTitle')}</dt>
            <dd><input value={title} onChange={(e) => setTitle(e.target.value)} /></dd>
            <dt>{t('people.quick.fStage')}</dt>
            <dd>
              <select value={stage} onChange={(e) => setStage(e.target.value)}>
                <option value="">{t('people.quick.noStage')}</option>
                {stages.map((s) => <option key={s.key} value={s.key}>{s.key} · {s.title}</option>)}
              </select>
            </dd>
            <dt>{t('people.quick.fDue')}</dt>
            <dd><input type="date" value={due} onChange={(e) => setDue(e.target.value)} /></dd>
            {draft.who && <><dt>{t('people.quick.fWho')}</dt><dd>{draft.who} <span className="muted">({t('people.quick.whoNote')})</span></dd></>}
          </dl>
        )}
        {draft && draft.unmatched.length > 0 && <p className="warn">{t('people.quick.unmatched', { items: draft.unmatched.join(', ') })}</p>}
        <Note msg={msg} />
        <button className="btn pri" type="submit" disabled={!draft || !title.trim() || !stage}>{t('people.quick.add')}</button>
      </form>
    </Modal>
  );
}

const EXPORTS: { id: ExportFormat; ext: string }[] = [
  { id: 'html', ext: 'html' }, { id: 'md', ext: 'md' }, { id: 'csv', ext: 'csv' }, { id: 'github', ext: 'json' },
];

function ExportDialog({ close }: { close: () => void }) {
  const { t } = useTranslation();
  const { project } = useApp();
  const [msg, setMsg] = useState<Msg>(null);

  async function run(f: { id: ExportFormat; ext: string }) {
    try {
      const path = await save({ defaultPath: `${project?.name ?? 'plan'}.${f.ext}`, filters: [{ name: f.id, extensions: [f.ext] }] });
      if (!path) return;
      await api.exportWrite(f.id, path);
      setMsg({ bad: false, text: t('people.export.saved', { path }) });
    } catch (e) {
      setMsg({ bad: true, text: errorText(e) });
    }
  }

  return (
    <Modal title={t('people.export.title')} onClose={close}>
      <ul className="tpl-list">
        {EXPORTS.map((f) => <li key={f.id}><button onClick={() => void run(f)}>{t(`people.export.${f.id}`)}</button></li>)}
      </ul>
      <Note msg={msg} />
    </Modal>
  );
}

const guessFormat = (path: string): ImportFormat => (/\.json$/i.test(path) ? 'trello' : /\.csv$/i.test(path) ? 'csv' : 'md');

function ImportDialog({ close }: { close: () => void }) {
  const { t } = useTranslation();
  const { setStages } = useApp();
  const [path, setPath] = useState<string | null>(null);
  const [format, setFormat] = useState<ImportFormat>('md');
  const [prev, setPrev] = useState<ImportPreview | null>(null);
  const [msg, setMsg] = useState<Msg>(null);

  async function choose() {
    const p = await open({ filters: [{ name: 'plan', extensions: ['md', 'markdown', 'csv', 'json'] }] });
    if (typeof p === 'string') { setPath(p); setFormat(guessFormat(p)); }
  }

  useEffect(() => {
    setPrev(null);
    if (!path) return;
    let alive = true;
    api.importPreview(format, path)
      .then((r) => { if (alive) { setPrev(r); setMsg(null); } })
      .catch((e) => { if (alive) setMsg({ bad: true, text: errorText(e) }); });
    return () => { alive = false; };
  }, [path, format]);

  async function apply() {
    if (!path) return;
    try {
      setStages(await api.importApply(format, path));
      await refreshBoards();
      close();
    } catch (e) {
      setMsg({ bad: true, text: errorText(e) });
    }
  }

  return (
    <Modal title={t('people.import.title')} onClose={close}>
      <div className="tpl-form">
        <button className="btn" onClick={() => void choose().catch((e) => setMsg({ bad: true, text: errorText(e) }))}>{t('people.import.choose')}</button>
        {path && <p className="muted mono">{path}</p>}
        {path && (
          <label>{t('people.import.format')}{' '}
            <select value={format} onChange={(e) => setFormat(e.target.value as ImportFormat)}>
              {(['md', 'csv', 'trello', 'notion', 'spec-kit'] as const).map((f) => <option key={f} value={f}>{t(`people.import.${f}`)}</option>)}
            </select>
          </label>
        )}
        {prev && (
          <div>
            <p><b>{t('people.import.counts', { stages: prev.stages, tasks: prev.tasks })}</b></p>
            {prev.created.length > 0 && <p className="muted">{t('people.import.created', { list: prev.created.join(', ') })}</p>}
            {prev.updated.length > 0 && <p className="muted">{t('people.import.updated', { list: prev.updated.join(', ') })}</p>}
            {prev.warnings.map((w) => <p key={w} className="warn">{w}</p>)}
          </div>
        )}
        <Note msg={msg} />
        <button className="btn pri" disabled={!prev} onClick={() => void apply()}>{t('people.import.apply')}</button>
      </div>
    </Modal>
  );
}

function HistoryDialog({ close }: { close: () => void }) {
  const { t } = useTranslation();
  const { setStages } = useApp();
  const [rows, setRows] = useState<HistoryRow[] | null>(null);
  const [msg, setMsg] = useState<Msg>(null);
  const load = () => api.historyList(50).then(setRows).catch((e) => setMsg({ bad: true, text: errorText(e) }));
  useEffect(() => { void load(); }, []);

  async function revert(id: string) {
    try {
      setStages(await api.historyRevert(id));
      await refreshBoards();
      setMsg({ bad: false, text: t('people.history.reverted') });
    } catch (e) {
      setMsg({ bad: true, text: errorText(e) });
    }
    await load();
  }

  return (
    <Modal title={t('people.history.title')} onClose={close}>
      {rows?.length === 0 && <p className="muted">{t('people.history.empty')}</p>}
      <ul className="hist-list">
        {rows?.map((r) => (
          <li key={r.id}>
            <div>
              <b>{r.action}</b> <span className="ag-meta">{new Date(r.at).toLocaleString()}</span>
              <div className="muted mono hist-files">{r.files.join(', ')}</div>
            </div>
            <button className="btn sm" onClick={() => void revert(r.id)}>{t('people.history.revert')}</button>
          </li>
        ))}
      </ul>
      <Note msg={msg} />
    </Modal>
  );
}

export function PeopleDialogs() {
  const { peopleDialog, set } = useApp();
  const close = () => set({ peopleDialog: null });
  if (peopleDialog === 'quickAdd') return <QuickAdd close={close} />;
  if (peopleDialog === 'export') return <ExportDialog close={close} />;
  if (peopleDialog === 'import') return <ImportDialog close={close} />;
  if (peopleDialog === 'history') return <HistoryDialog close={close} />;
  return null;
}
