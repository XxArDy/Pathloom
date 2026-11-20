import { useEffect, useState } from 'react';
import { useTranslation } from 'react-i18next';
import { IconX } from '@tabler/icons-react';
import { open } from '@tauri-apps/plugin-dialog';
import { api, errorText, type TemplateInfo } from '../api';
import { useApp } from '../store';

const slug = (s: string) => s.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-+|-+$/g, '').slice(0, 40);
const folderName = (p: string) => p.split(/[\\/]/).filter(Boolean).pop() ?? p;

/** Template picker ("new" on the start screen, "add" in a project) and "save plan as template" (docs/m10 §А1). */
export function TemplateDialog({ mode, onClose }: { mode?: 'new'; onClose?: () => void }) {
  const { t, i18n } = useTranslation();
  const { templateDialog, enterProject, setStages, set } = useApp();
  const kind = mode ?? templateDialog;
  const close = () => (onClose ? onClose() : set({ templateDialog: null }));
  const [list, setList] = useState<TemplateInfo[]>([]);
  const [error, setError] = useState<string | null>(null);
  const [name, setName] = useState('');
  const [title, setTitle] = useState('');
  const lang = i18n.language.startsWith('uk') ? 'uk' : 'en';

  useEffect(() => {
    if (kind && kind !== 'save') api.templatesList(lang).then(setList).catch((e) => setError(errorText(e)));
  }, [kind, lang]);
  useEffect(() => {
    if (!kind) return;
    const onKey = (e: KeyboardEvent) => { if (e.code === 'Escape') close(); };
    window.addEventListener('keydown', onKey);
    return () => window.removeEventListener('keydown', onKey);
  });
  if (!kind) return null;

  async function pick(tpl: TemplateInfo) {
    setError(null);
    try {
      if (kind === 'new') {
        const dir = await open({ directory: true, title: t('tpl.pickFolder') });
        if (typeof dir !== 'string') return;
        close();
        await enterProject(await api.templateNew(dir, folderName(dir), tpl.id, lang));
      } else {
        setStages(await api.templateApply(tpl.id, lang));
        set({ boards: await api.boardsList() });
        const st = useApp.getState();
        const b = st.activeBoard ?? st.boards[0];
        if (b) await st.openBoard(b);
        close();
      }
    } catch (e) {
      setError(errorText(e));
    }
  }

  async function save() {
    setError(null);
    try {
      await api.templateSave(name, title || name);
      close();
    } catch (e) {
      setError(errorText(e));
    }
  }

  return (
    <div className="modal-back" onMouseDown={(e) => { if (e.target === e.currentTarget) close(); }}>
      <div className="modal" style={{ width: 'min(520px, 94vw)' }} role="dialog" aria-modal="true" aria-label={t('tpl.title')}>
        <div className="modal-head">
          <h2>{kind === 'save' ? t('tpl.saveTitle') : t('tpl.title')}</h2>
          <button className="icon-btn push" aria-label={t('inspector.close')} onClick={close}><IconX size={16} /></button>
        </div>
        {kind === 'save' ? (
          <form className="tpl-form" onSubmit={(e) => { e.preventDefault(); void save(); }}>
            <input autoFocus placeholder={t('tpl.titleField')} value={title} onChange={(e) => { setTitle(e.target.value); if (!name || name === slug(title)) setName(slug(e.target.value)); }} />
            <input placeholder={t('tpl.name')} value={name} onChange={(e) => setName(e.target.value)} />
            <button className="btn pri" type="submit" disabled={!name}>{t('tpl.ok')}</button>
          </form>
        ) : (
          <div className="inbox-body">
            {list.length === 0 && !error && <p className="muted">{t('tpl.empty')}</p>}
            <ul className="tpl-list">
              {list.map((x) => (
                <li key={x.id}>
                  <button onClick={() => void pick(x)}>
                    <b>{x.title}</b> <span className="ag-meta">{x.builtin ? t('tpl.builtin') : t('tpl.mine')}</span>
                    <span className="tpl-d">{x.description}</span>
                  </button>
                </li>
              ))}
            </ul>
          </div>
        )}
        {error && <p className="err" role="alert" style={{ padding: '0 18px 14px' }}>{error}</p>}
      </div>
    </div>
  );
}
