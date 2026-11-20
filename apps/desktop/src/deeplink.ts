// pathloom://open links: open the project, then select the stage/task (m10 §Б7).
import { listen } from '@tauri-apps/api/event';
import { ask } from '@tauri-apps/plugin-dialog';
import { api, errorText, type LinkTarget } from './api';
import i18n from './i18n';
import { openProject } from './projectActions';
import { useApp } from './store';

const norm = (p: string) => p.replace(/[\\/]+$/, '').replace(/\\/g, '/').toLowerCase();

/** A link may come from anywhere: a folder the user never opened needs their yes first. */
async function allowed(path: string): Promise<boolean> {
  const recent = await api.recentProjects().catch(() => []);
  if (recent.some((r) => norm(r.path) === norm(path))) return true;
  return ask(i18n.t('link.confirmOpen', { path }), { title: i18n.t('link.openTitle'), kind: 'warning' });
}

async function openTarget(t: LinkTarget): Promise<void> {
  try {
    if (norm(useApp.getState().project?.root ?? '') !== norm(t.project)) {
      if (!(await allowed(t.project))) return;
      await openProject(t.project);
    }
    useApp.getState().set({ view: 'board', selectedStage: t.stage, selectedTask: t.task });
  } catch (e) {
    useApp.getState().set({ error: errorText(e) });
  }
}

/** Call once after the startup project is open: handles the launch link and links that arrive later. */
export async function startDeepLinks(): Promise<() => void> {
  const un = await listen<LinkTarget>('deep-link', (e) => void openTarget(e.payload));
  const first = await api.deeplinkPending().catch(() => null);
  if (first) await openTarget(first);
  return un;
}
