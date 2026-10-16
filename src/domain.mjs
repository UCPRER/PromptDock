import legacyDefaults from '../src-tauri/legacy-default-data.json' with { type: 'json' };

export const DEFAULT_CATEGORY = '默认';
export const DEFAULT_SETTINGS = Object.freeze({ defaultAction: 'send', sendKey: 'enter', theme: 'system', shortcut: 'Ctrl+Alt+Space' });
const MAX_PROMPTS = 2000;
const MAX_CATEGORIES = 2001;
export function categoryName(value) {
  if (typeof value !== 'string' || !value.trim() || value.trim().length > 40) throw new Error('类别名称不能为空，且不能超过 40 个字符。');
  return value.trim();
}
export function normalizeImport(input) {
  if (!input || typeof input !== 'object' || Array.isArray(input) || input.version !== 1 || !Array.isArray(input.prompts)) throw new Error('文件格式不正确，需要 PromptDock 导出的 version 1 JSON 文件。');
  if (input.prompts.length > MAX_PROMPTS) throw new Error('单次最多导入 2000 条提示词。');
  const seen = new Set();
  const prompts = input.prompts.map((p, index) => {
    if (!p || typeof p !== 'object') throw new Error(`第 ${index + 1} 条提示词格式不正确。`);
    for (const key of ['id', 'title', 'content', 'category']) if (typeof p[key] !== 'string') throw new Error(`第 ${index + 1} 条提示词缺少 ${key}。`);
    if (!p.id || p.id.length > 120 || seen.has(p.id)) throw new Error('提示词 ID 为空、过长或重复。');
    if (!p.title.trim() || p.title.length > 100 || !p.content.trim() || p.content.length > 100000 || p.category.length > 40) throw new Error(`「${p.title || '未命名'}」的标题或正文不符合长度要求。`);
    seen.add(p.id);
    const timestamp = value => typeof value === 'number' && Number.isFinite(value) && value >= 0 ? Math.floor(value) : 0;
    return { id: p.id, title: p.title.trim(), content: p.content, category: p.category.trim() || DEFAULT_CATEGORY, tags: Array.isArray(p.tags) ? [...new Set(p.tags.filter(t => typeof t === 'string').map(t => t.trim()).filter(Boolean))].slice(0, 20).map(t => t.slice(0, 40)) : [], favorite: p.favorite === true, createdAt: timestamp(p.createdAt), updatedAt: timestamp(p.updatedAt), lastUsedAt: p.lastUsedAt == null ? null : timestamp(p.lastUsedAt), useCount: timestamp(p.useCount) };
  });
  if (input.categories !== undefined && !Array.isArray(input.categories)) throw new Error('类别列表格式不正确。');
  const categories = [...new Set([DEFAULT_CATEGORY, ...(input.categories || []).map(categoryName), ...prompts.map(p => p.category)])];
  if (categories.length > MAX_CATEGORIES) throw new Error('自定义类别不能超过 2000 个。');
  const raw = input.settings || {};
  return { version: 1, categories, prompts, settings: { defaultAction: ['send','insert','copy'].includes(raw.defaultAction) ? raw.defaultAction : 'send', sendKey: raw.sendKey === 'ctrlEnter' ? 'ctrlEnter' : 'enter', theme: ['system','light','dark'].includes(raw.theme) ? raw.theme : 'system', shortcut: typeof raw.shortcut === 'string' && raw.shortcut.length < 100 ? raw.shortcut : DEFAULT_SETTINGS.shortcut } };
}
export function migrateStoredData(input) {
  const data = normalizeImport(input);
  if (Object.hasOwn(input, 'categories')) return data;
  const prompts = data.prompts.filter(prompt => !legacyDefaults.prompts.some(old =>
    ['id','title','content','category'].every(key => prompt[key] === old[key]) && JSON.stringify(prompt.tags) === JSON.stringify(old.tags)));
  if (prompts.length !== data.prompts.length && !prompts.some(p => p.id === 'starter-test')) prompts.push(seedData().prompts[0]);
  return normalizeImport({...data, prompts, categories:[DEFAULT_CATEGORY, ...prompts.map(p => p.category)]});
}
export function applyMutation(current, action) {
  const data = normalizeImport(current);
  if (action.op === 'categoryCreate') {
    const name = categoryName(action.name);
    if (data.categories.includes(name)) throw new Error('这个类别已存在。');
    data.categories.push(name);
  } else if (action.op === 'categoryDelete') {
    const name = categoryName(action.name);
    if (name === DEFAULT_CATEGORY) throw new Error('默认类别不能删除。');
    if (!data.categories.includes(name)) throw new Error('类别已不存在，请刷新。');
    data.categories = data.categories.filter(item => item !== name);
    data.prompts = data.prompts.map(prompt => prompt.category === name ? {...prompt, category:DEFAULT_CATEGORY, updatedAt:Date.now()} : prompt);
  } else if (action.op === 'upsert') {
    const prompt = {...action.prompt, category:action.prompt.category.trim() || DEFAULT_CATEGORY};
    if (!data.categories.includes(prompt.category)) throw new Error('所选类别已不存在，请重新选择类别。');
    const index = data.prompts.findIndex(p => p.id === prompt.id);
    if (index < 0) data.prompts.unshift(prompt);
    else data.prompts[index] = {...prompt,lastUsedAt:data.prompts[index].lastUsedAt,useCount:data.prompts[index].useCount};
  } else if (action.op === 'delete') data.prompts = data.prompts.filter(p => p.id !== action.id);
  else if (action.op === 'favorite') {
    const prompt = data.prompts.find(p => p.id === action.id);
    if (!prompt) throw new Error('提示词已不存在，请刷新。');
    prompt.favorite = !prompt.favorite; prompt.updatedAt = Date.now();
  } else if (action.op === 'reorder') {
    const byId = new Map(data.prompts.map(p => [p.id,p]));
    if (action.ids.length !== byId.size || new Set(action.ids).size !== byId.size || action.ids.some(id => !byId.has(id))) throw new Error('排序数据已更新，请重试。');
    data.prompts = action.ids.map(id => byId.get(id));
  } else if (action.op === 'settings') data.settings = action.settings;
  else if (action.op === 'import') {
    const incoming = normalizeImport(action.data);
    if (action.mode === 'replace') return incoming;
    if (action.mode !== 'merge') throw new Error('不支持的导入方式。');
    data.prompts = mergePrompts(data.prompts, incoming.prompts);
    data.categories = [...new Set([...data.categories,...incoming.categories])];
  } else throw new Error('不支持的操作。');
  return normalizeImport(data);
}
export function filterPrompts(prompts, query = '', view = 'all', category = '') {
  const terms = query.toLocaleLowerCase().trim().split(/\s+/).filter(Boolean);
  const filtered = prompts.filter(p => (view !== 'favorites' || p.favorite) && (view !== 'recent' || p.lastUsedAt != null) && (!category || p.category === category) && terms.every(term => `${p.title}\n${p.content}\n${p.category}\n${p.tags.join(' ')}`.toLocaleLowerCase().includes(term)));
  return view === 'recent' ? filtered.slice().sort((a,b) => (b.lastUsedAt || 0) - (a.lastUsedAt || 0)) : filtered;
}
export function getVariables(content) {
  return [...new Set([...content.matchAll(/\{\{\s*([^{}\n]{1,60}?)\s*\}\}/g)].map(match => match[1].trim()).filter(Boolean))];
}
export function renderTemplate(content, values) {
  return content.replace(/\{\{\s*([^{}\n]{1,60}?)\s*\}\}/g, (original, key) => Object.hasOwn(values, key.trim()) ? String(values[key.trim()]) : original);
}
export function reorderPrompts(prompts, draggedId, targetId) {
  const ids = prompts.map(p => p.id); const from = ids.indexOf(draggedId); const to = ids.indexOf(targetId);
  if (from < 0 || to < 0 || from === to) return ids;
  ids.splice(from, 1); ids.splice(to, 0, draggedId); return ids;
}
export function mergePrompts(existing, incoming) {
  const byId = new Map(existing.map(p => [p.id, p]));
  for (const prompt of incoming) if (!byId.has(prompt.id) || byId.get(prompt.id).updatedAt < prompt.updatedAt) byId.set(prompt.id, prompt);
  if (byId.size > MAX_PROMPTS) throw new Error('合并后的提示词不能超过 2000 条。');
  return [...byId.values()];
}
export function seedData() {
  const now = Date.now();
  return { version: 1, categories:[DEFAULT_CATEGORY], settings: { ...DEFAULT_SETTINGS }, prompts:[{id:'starter-test',title:'test',content:'test',category:DEFAULT_CATEGORY,tags:[],favorite:false,createdAt:now,updatedAt:now,lastUsedAt:null,useCount:0}] };
}
