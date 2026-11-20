// Typed wrappers over Tauri commands (docs/m1-contracts.md §3).
import { invoke } from '@tauri-apps/api/core';
import type { Board, StageView } from '@pathloom/canvas';
import type { LogEntry } from './agents';
import type { AgentProfile } from '@pathloom/canvas';

export interface ProjectSnapshot {
  root: string;
  name: string;
  boards: string[];
  stages: StageView[];
}

/** Partial stage update; omitted fields stay unchanged, '' clears due/branch. */
export interface UsageTotals { tokens_in: number; tokens_out: number; cost_usd?: number }
export interface UsageSummary {
  total: UsageTotals; by_agent: Record<string, UsageTotals>; by_stage: Record<string, UsageTotals>;
  by_stage_model?: Record<string, Record<string, UsageTotals>>;
  quota?: Record<string, { percent_weekly: number; resets_at?: string; at: string }>;
}
export interface CommitRow { sha: string; subject: string; author: string; at: string; tasks: string[] }

export interface InboxItem { task_id: string; stage: string; title: string; kind: 'approval' | 'decision' | 'owner' }

export interface AgendaItem { key: string; title: string; due: string | null; status: StageView['status']; who: string; progress: [number, number] }
export interface Agenda { today: string; days: number; overdue: AgendaItem[]; due: AgendaItem[]; blocked: AgendaItem[]; waiting: InboxItem[] }
export interface Digest {
  since: string; today: string; done: unknown[];
  stuck: { blocked: AgendaItem[]; overdue: AgendaItem[]; silent_claims: unknown[] };
  waiting: InboxItem[];
}
export interface TemplateInfo { id: string; title: string; description: string; builtin: boolean }

export interface StageMeta {
  check?: string;
  title?: string;
  kind?: StageView['kind'];
  who?: StageView['who'];
  priority?: StageView['priority'];
  due?: string;
  branch?: string;
}

export interface RecentProject {
  path: string;
  name: string;
  openedAt: number;
}

/** Git mode: the board is read from `readRef`; owner actions that cannot be written now wait in a queue. */
export interface GitStatus { readRef: string | null; sha: string | null; error: string | null; queued: number }

/** Parsed `pathloom://open` link (from the shell). */
export interface LinkTarget { project: string; stage: string | null; task: string | null }

/** Details of an ALREADY_DECIDED error ("... is already decided by BY at AT: VALUE"), or null for any other error. */
export function alreadyDecided(e: unknown): { by: string; at: string } | null {
  const m = /is already decided by (.+?) at (\S+): /.exec(errorText(e));
  return m ? { by: m[1], at: m[2] } : null;
}

export const api = {
  linkMake: (id?: string) => invoke<string>('link_make', { id: id ?? null }),
  deeplinkPending: () => invoke<LinkTarget | null>('deeplink_pending'),
  projectCreate: (path: string, name: string) => invoke<ProjectSnapshot>('project_create', { path, name }),
  projectOpen: (path: string) => invoke<ProjectSnapshot>('project_open', { path }),
  demoOpen: (lang: string) => invoke<ProjectSnapshot>('demo_open', { lang }),
  recentProjects: () => invoke<RecentProject[]>('recent_projects'),
  boardLoad: (name: string) => invoke<Board>('board_load', { name }),
  boardAutoLayout: (name: string, on: boolean) => invoke<Board>('board_auto_layout', { name, on }),
  boardArrange: (name: string) => invoke<Board>('board_arrange', { name }),
  recentForget: (path: string) => invoke<void>('recent_forget', { path }),
  stageRenameKey: (key: string, newKey: string) => invoke<StageView[]>('stage_rename_key', { key, newKey }),
  taskNoteSet: (taskId: string, note: string) => invoke<StageView[]>('task_note_set', { taskId, note }),
  taskFileAdd: (taskId: string, path: string) => invoke<StageView[]>('task_file_add', { taskId, path }),
  taskFileRemove: (taskId: string, path: string) => invoke<StageView[]>('task_file_remove', { taskId, path }),
  openLicenses: () => invoke<void>('open_licenses'),
  boardSave: (name: string, board: Board) => invoke<void>('board_save', { name, board }),
  boardCreate: (name: string, title: string) => invoke<Board>('board_create', { name, title }),
  boardsList: () => invoke<string[]>('boards_list'),
  stagesList: () => invoke<StageView[]>('stages_list'),
  stageCreate: (title: string) => invoke<StageView>('stage_create', { title }),
  taskToggle: (taskId: string) => invoke<StageView[]>('task_toggle', { taskId }),
  taskAdd: (key: string, title: string) => invoke<StageView[]>('task_add', { key, title }),
  gateRun: (key: string) => invoke<StageView[]>('gate_run', { key }),
  artifactOpen: (path: string) => invoke<void>('artifact_open', { path }),
  gitHead: () => invoke<string | null>('git_head'),
  stageCommits: (key: string) => invoke<CommitRow[]>('stage_commits', { key }),
  usageSummary: () => invoke<UsageSummary>('usage_summary'),
  stageUpdate: (key: string, meta: StageMeta) => invoke<StageView[]>('stage_update', { key, meta }),
  stageSource: (key: string) => invoke<string>('stage_source', { key }),
  stageDelete: (key: string) => invoke<StageView[]>('stage_delete', { key }),
  taskRename: (taskId: string, title: string) => invoke<StageView[]>('task_rename', { taskId, title }),
  taskRemove: (taskId: string) => invoke<StageView[]>('task_remove', { taskId }),
  subtaskAdd: (taskId: string, title: string) => invoke<StageView[]>('subtask_add', { taskId, title }),
  agentSetup: () => invoke<{ cliPath: string | null; agentChanges: 'auto' | 'review'; requireEvidence: boolean; readRef: string; fetchIntervalSecs: number }>('agent_setup'),
  setGitConfig: (readRef: string, interval: number) => invoke<void>('set_git_config', { readRef, interval }),
  gitStatus: () => invoke<GitStatus>('git_status'),
  setRequireEvidence: (on: boolean) => invoke<void>('set_require_evidence', { on }),
  setAgentChanges: (review: boolean) => invoke<void>('set_agent_changes', { review }),
  activity: (limit: number, stage?: string) => invoke<LogEntry[]>('activity', { limit, stage: stage ?? null }),
  agentsList: () => invoke<AgentProfile[]>('agents_list'),
  openUrl: (url: string) => invoke<void>('open_url', { url }),
  claimRelease: (taskId: string) => invoke<StageView[]>('claim_release', { taskId }),
  themesList: () => invoke<unknown[]>('themes_list'),
  assetSave: (name: string, bytes: number[]) => invoke<string>('asset_save', { name, bytes }),
  stageRename: (key: string, title: string) => invoke<StageView[]>('stage_rename', { key, title }),
  stageLink: (from: string, to: string) => invoke<StageView[]>('stage_link', { from, to }),
  approvalsDecide: (taskId: string, approve: boolean, reasons: string[], comment?: string) =>
    invoke<StageView[]>('approvals_decide', { taskId, approve, reasons, comment: comment || null }),
  decisionAnswer: (taskId: string, choice: string) => invoke<StageView[]>('decision_answer', { taskId, choice }),
  inboxList: () => invoke<InboxItem[]>('inbox_list'),
  evidenceRecheck: (taskId: string) => invoke<number>('evidence_recheck', { taskId }),
  agenda: (today: string, days: number) => invoke<Agenda>('agenda', { today, days }),
  digest: (since: string, today: string) => invoke<Digest>('digest', { since, today }),
  templatesList: (lang: string) => invoke<TemplateInfo[]>('templates_list', { lang }),
  templateNew: (path: string, name: string, id: string, lang: string) => invoke<ProjectSnapshot>('template_new', { path, name, id, lang }),
  templateApply: (id: string, lang: string) => invoke<StageView[]>('template_apply', { id, lang }),
  templateSave: (name: string, title: string) => invoke<string>('template_save', { name, title }),
  stageUnlink: (from: string, to: string) => invoke<StageView[]>('stage_unlink', { from, to }),
};

/** URL of a project asset (plan/assets/...) served by the app's `plasset` protocol. */
export function assetUrl(path: string): string {
  const base = navigator.userAgent.includes('Windows') ? 'http://plasset.localhost/' : 'plasset://localhost/';
  return base + path.split('/').map(encodeURIComponent).join('/');
}

/** URL of an image attached to a task (project-relative path), served by `plasset` inside the project only. */
export function fileUrl(path: string): string {
  return assetUrl('files/' + path);
}

export const isImageFile = (f: string) => !/^https?:\/\//i.test(f) && /\.(png|jpe?g|gif|webp)$/i.test(f);

export function errorText(e: unknown): string {
  return typeof e === 'string' ? e : e instanceof Error ? e.message : String(e);
}
