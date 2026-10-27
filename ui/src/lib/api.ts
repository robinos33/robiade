import { invoke } from '@tauri-apps/api/core';
import type { Agent, AssetContent, Entry, Environment, Overview, SearchHit, Session, Task, TaskStatus } from './types';

export const api = {
  environment: () => invoke<Environment>('environment'),
  listHistory: () => invoke<Session[]>('list_history'),
  readTranscript: (sessionId: string) => invoke<Entry[]>('read_transcript', { sessionId }),
  searchHistory: (query: string) => invoke<SearchHit[]>('search_history', { query }),
  stats: (windowDays: number, projectRoot: string | null) =>
    invoke<Overview>('stats', {
      windowDays,
      utcOffsetMinutes: -new Date().getTimezoneOffset(),
      projectRoot,
    }),
  listTasks: () => invoke<Task[]>('list_tasks'),
  createTask: (title: string, projectPath: string | null) => invoke<Task>('create_task', { title, projectPath }),
  updateTask: (id: string, patch: { title?: string; notes?: string; status?: TaskStatus }) =>
    invoke<Task>('update_task', { id, ...patch }),
  deleteTask: (id: string) => invoke<void>('delete_task', { id }),
  listAgents: () => invoke<Agent[]>('list_agents'),
  spawnAgent: (projectPath: string, resumeSessionId: string | null, cols: number, rows: number) =>
    invoke<Agent>('spawn_agent', { projectPath, resumeSessionId, cols, rows }),
  writeAgent: (id: string, data: string) => invoke<void>('write_agent', { id, data }),
  resizeAgent: (id: string, cols: number, rows: number) => invoke<void>('resize_agent', { id, cols, rows }),
  killAgent: (id: string) => invoke<void>('kill_agent', { id }),
  markAgentSeen: (id: string) => invoke<void>('mark_agent_seen', { id }),
  readAsset: (id: string, path: string) => invoke<AssetContent>('read_asset', { id, path }),
  quit: () => invoke<void>('quit'),
};
