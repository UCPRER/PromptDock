export type Action = 'send' | 'insert' | 'copy';
export interface Prompt { id: string; title: string; content: string; category: string; tags: string[]; favorite: boolean; createdAt: number; updatedAt: number; lastUsedAt: number | null; useCount: number }
export interface Settings { defaultAction: Action; sendKey: 'enter' | 'ctrlEnter'; theme: 'system' | 'light' | 'dark'; shortcut: string }
export interface AppData { version: 1; categories: string[]; prompts: Prompt[]; settings: Settings }
export interface Target { hwnd: string; pid: number; title: string; process: string; bound: boolean }
export interface NativeStatus { target: Target | null; shortcutError: string | null; dataPath: string }
export type Mutation = { op: 'upsert'; prompt: Prompt } | { op: 'delete'; id: string } | { op: 'favorite'; id: string } | { op: 'reorder'; ids: string[] } | { op: 'settings'; settings: Settings } | { op: 'import'; data: AppData; mode: 'merge' | 'replace' } | { op: 'categoryCreate'; name: string } | { op: 'categoryDelete'; name: string };
