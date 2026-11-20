// pathloom://open links: open the project, then select the stage/task (m10 §Б7).
import { listen } from '@tauri-apps/api/event';
import { api, errorText, type LinkTarget } from './api';
import { openProject } from './projectActions';
import { useApp } from './store';

const norm = (p: string) => p.replace(/[\\/]+$/, '').replace(/\\/g, '/').toLowerCase();

async function openTarget(t: LinkTarget): Promise<void> {
  try {
    if (norm(useApp.getState().project?.root ?? '') !== norm(t.project)) await openProject(t.project);
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
