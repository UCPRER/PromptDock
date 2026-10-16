import { invoke, isTauri } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { getCurrentWindow } from '@tauri-apps/api/window';
import type { Action, AppData, Mutation, NativeStatus, Target } from './types';
import { applyMutation, migrateStoredData, normalizeImport, seedData } from './domain.mjs';
export const native = isTauri();
const PREVIEW_KEY = 'promptdock-preview-v1';
let channel: BroadcastChannel | undefined;
const subscribers = new Set<(data:AppData) => void>();
if (!native && typeof BroadcastChannel !== 'undefined') { channel = new BroadcastChannel(PREVIEW_KEY); channel.onmessage = e => subscribers.forEach(fn => fn(normalizeImport(e.data))); }
export async function loadData(): Promise<AppData> {
  if (native) return normalizeImport(await invoke('load_data'));
  const raw = localStorage.getItem(PREVIEW_KEY); if (!raw) return seedData();
  const stored = JSON.parse(raw); const migrated = migrateStoredData(stored);
  if (!Object.hasOwn(stored,'categories') && localStorage.getItem(PREVIEW_KEY+'-before-categories')===null) localStorage.setItem(PREVIEW_KEY+'-before-categories',raw);
  if (JSON.stringify(stored) !== JSON.stringify(migrated)) localStorage.setItem(PREVIEW_KEY, JSON.stringify(migrated));
  return migrated;
}
export async function mutate(action: Mutation): Promise<AppData> {
  if (native) return normalizeImport(await invoke('mutate_data', { action }));
  const valid = applyMutation(await loadData(), action); localStorage.setItem(PREVIEW_KEY, JSON.stringify(valid)); channel?.postMessage(valid); subscribers.forEach(fn => fn(valid)); return valid;
}
export async function onData(fn:(data:AppData)=>void) { if(native) return listen<AppData>('data-changed', e => fn(normalizeImport(e.payload))); subscribers.add(fn); return () => subscribers.delete(fn); }
export async function getStatus():Promise<NativeStatus> { return native ? invoke('get_status') : { target:null, shortcutError:null, dataPath:'浏览器预览 · 数据保存在此浏览器' }; }
export async function onTarget(fn:(target:Target|null)=>void) { return native ? listen<Target|null>('target-changed', e => fn(e.payload)) : () => {}; }
export async function listTargets():Promise<Target[]> { return native ? invoke('list_targets') : []; }
export async function bindTarget(hwnd:string):Promise<Target> { return invoke('bind_target', { hwnd }); }
export async function act(id:string, text:string, action:Action):Promise<{ message:string }> {
  if(native) return invoke('run_prompt', { id, text, action });
  if(action !== 'copy') throw new Error('这是界面预览。向其他应用填入或发送，需要运行 Windows 桌面版。');
  await navigator.clipboard.writeText(text); return {message:'已复制提示词'};
}
export async function showDock() { if(native) return invoke('show_dock'); window.open(`${location.pathname}?view=dock`, 'promptdock-dock', 'width=400,height=660'); }
export async function showManager() { if(native) return invoke('show_manager'); window.open(location.pathname, 'promptdock-manager'); }
export async function hideWindow() { if(native) await getCurrentWindow().hide(); else window.close(); }
export async function minimize() { if(native) await getCurrentWindow().minimize(); }
export async function toggleMaximize() { if(native) await getCurrentWindow().toggleMaximize(); }
export async function startDrag() { if(native) await getCurrentWindow().startDragging(); }
export async function exportData() { if(native) return invoke<boolean>('export_data'); const blob = new Blob([JSON.stringify(await loadData(),null,2)], {type:'application/json'}); const url=URL.createObjectURL(blob); const a=document.createElement('a'); a.href=url;a.download=`PromptDock-${new Date().toISOString().slice(0,10)}.json`;a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);return true; }
export async function importDataNative():Promise<AppData|null> { const raw=await invoke<string|null>('pick_import'); return raw == null ? null : normalizeImport(JSON.parse(raw)); }
