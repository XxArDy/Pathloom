// System notifications (docs/m10 §А4). Off by default; checked every minute while the app is open.
import { isPermissionGranted, requestPermission, sendNotification } from '@tauri-apps/plugin-notification';
import i18n from './i18n';
import { api } from './api';
import { useApp } from './store';

export interface ReminderSettings { enabled: boolean; due: boolean; waiting: boolean; summary: boolean; time: string }
interface State { dueSent: string[]; waiting: string[] | null; summaryDay: string; summaryAt: string }

const SETTINGS_KEY = 'pathloom.reminders';
const STATE_KEY = 'pathloom.remindersState';
const DEFAULTS: ReminderSettings = { enabled: false, due: true, waiting: true, summary: true, time: '09:00' };

export function loadReminders(): ReminderSettings {
  try {
    return { ...DEFAULTS, ...JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? '{}') };
  } catch {
    return DEFAULTS;
  }
}

export function saveReminders(s: ReminderSettings): void {
  try { localStorage.setItem(SETTINGS_KEY, JSON.stringify(s)); } catch { /* keep in-memory only */ }
}

function loadState(root: string): State {
  const empty: State = { dueSent: [], waiting: null, summaryDay: '', summaryAt: '' };
  try {
    return { ...empty, ...(JSON.parse(localStorage.getItem(STATE_KEY) ?? '{}')[root] ?? {}) };
  } catch {
    return empty;
  }
}

function saveState(root: string, st: State): void {
  try {
    const all = JSON.parse(localStorage.getItem(STATE_KEY) ?? '{}');
    all[root] = st;
    localStorage.setItem(STATE_KEY, JSON.stringify(all));
  } catch { /* worst case: a reminder repeats */ }
}

const pad = (n: number) => String(n).padStart(2, '0');
/** Local calendar date, YYYY-MM-DD. */
export function localDate(d = new Date()): string {
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}`;
}

async function allowed(): Promise<boolean> {
  try {
    return (await isPermissionGranted()) || (await requestPermission()) === 'granted';
  } catch {
    return false;
  }
}

/** One check. Does nothing unless reminders are enabled. */
export async function checkReminders(now = new Date()): Promise<void> {
  const s = loadReminders();
  const root = useApp.getState().project?.root;
  if (!s.enabled || !root || !(await allowed())) return;
  const t = i18n.t.bind(i18n);
  const today = localDate(now);
  const st = loadState(root);
  try {
    const ag = await api.agenda(today, 2);
    if (s.due) {
      const tomorrow = localDate(new Date(now.getFullYear(), now.getMonth(), now.getDate() + 1));
      const fresh = ag.due.filter((i) => !st.dueSent.includes(`${i.key}|${today}`));
      if (fresh.length) {
        const body = fresh.map((i) => `${i.title} (${i.due?.slice(0, 10) === tomorrow ? t('reminders.dueTomorrow') : t('reminders.dueToday')})`).join('\n');
        sendNotification({ title: t('reminders.dueTitle'), body });
      }
      st.dueSent = [...st.dueSent.filter((k) => k.endsWith(`|${today}`)), ...fresh.map((i) => `${i.key}|${today}`)];
    }
    if (s.waiting && st.waiting) {
      const known = st.waiting;
      const fresh = ag.waiting.filter((i) => !known.includes(i.task_id));
      if (fresh.length) sendNotification({ title: t('reminders.waitingTitle'), body: fresh.map((i) => i.title).join('\n') });
    }
    st.waiting = ag.waiting.map((i) => i.task_id); // the first run only records the current items
    if (s.summary && st.summaryDay !== today && `${pad(now.getHours())}:${pad(now.getMinutes())}` >= s.time) {
      const since = st.summaryAt || new Date(now.getTime() - 86_400_000).toISOString();
      const d = await api.digest(since, today);
      const counts: [number, string][] = [
        [d.done.length, 'sumDone'], [d.stuck.overdue.length, 'sumOverdue'], [d.stuck.blocked.length, 'sumBlocked'], [d.waiting.length, 'sumWaiting'],
      ];
      const parts = counts.filter(([n]) => n > 0).map(([n, k]) => t(`reminders.${k}`, { count: n }));
      if (parts.length) sendNotification({ title: t('reminders.summaryTitle'), body: parts.join(', ') });
      st.summaryDay = today;
      st.summaryAt = now.toISOString();
    }
    saveState(root, st);
  } catch { /* project closed or busy: try again next minute */ }
}

/** Starts the minute timer; returns the stop function. */
export function startReminders(): () => void {
  const id = setInterval(() => void checkReminders(), 60_000);
  void checkReminders();
  return () => clearInterval(id);
}
