<script setup lang="ts">
import { computed, nextTick, onMounted, onUnmounted, reactive, ref, watch } from 'vue';
import { ArrowDownToLine, ArrowUpFromLine, ArrowUpRight, Braces, Check, ChevronDown, CircleHelp, Clock3, Copy, Folder, Keyboard, Layers, LayoutGrid, LoaderCircle, Maximize2, Minus, Monitor, MoreHorizontal, PanelRight, Pencil, Pin, Plus, Search, Send, Settings2, Star, Target as TargetIcon, Trash2, X } from 'lucide-vue-next';
import * as bridge from './bridge';
import { DEFAULT_CATEGORY, categoryName, filterPrompts, getVariables, normalizeImport, renderTemplate, reorderPrompts, seedData } from './domain.mjs';
import type { Action, AppData, Mutation, NativeStatus, Prompt, Settings, Target } from './types';
const dock = new URLSearchParams(location.search).get('view') === 'dock';
const data = ref<AppData>(seedData());
const loaded = ref(false); const fatal = ref(''); const busy = ref(false); const sendingId = ref('');
const view = ref('all'); const category = ref(''); const search = ref(''); const searchEl = ref<HTMLInputElement>();
const status = ref<NativeStatus>({target:null,shortcutError:null,dataPath:''});
const targets = ref<Target[]>([]); const showTargets = ref(false); const refreshing = ref(false);
const editing = ref<Prompt|null>(null); const editorTags = ref(''); const editorError = ref('');
const showSettings = ref(false); const draftSettings = reactive<Settings>({...data.value.settings});
const showHelp = ref(false); const openMenu = ref(''); const draggedId = ref('');
const toast = ref<{text:string;error:boolean}|null>(null); let toastTimer: ReturnType<typeof setTimeout>;
const importCandidate = ref<AppData|null>(null); const fileInput = ref<HTMLInputElement>();
const deleteCandidate = ref<Prompt|null>(null);
const showCategoryEditor = ref(false); const categoryDraft = ref(''); const categoryError = ref('');
const categoryDeleteCandidate = ref<string|null>(null);
const variableRequest = ref<{prompt:Prompt;action:Action;names:string[]}|null>(null); const variableValues = ref<Record<string,string>>({});
const unlisteners: (()=>void)[] = [];
const categories = computed(() => data.value.categories);
watch(categories, values => {
  if (category.value && !values.includes(category.value)) { category.value=DEFAULT_CATEGORY;view.value='all'; }
  if (editing.value && !values.includes(editing.value.category)) editing.value.category=DEFAULT_CATEGORY;
  if (categoryDeleteCandidate.value && !values.includes(categoryDeleteCandidate.value)) categoryDeleteCandidate.value=null;
});
const favoriteCount = computed(() => data.value.prompts.filter(p=>p.favorite).length);
const visible = computed(() => filterPrompts(data.value.prompts,search.value,view.value,category.value));
const heading = computed(() => category.value || ({all:'我的提示词',favorites:'我的收藏',recent:'最近使用'}[view.value] || '我的提示词'));
const actionNames = {send:'发送',insert:'仅填入',copy:'复制'};
const promptActions = ['copy','insert','send'] as const;
const activeAction = ref<Action|null>(null);
const canUse = computed(() => loaded.value && !busy.value && !sendingId.value);
const targetText = computed(() => status.value.target?.title || (bridge.native ? '选择目标窗口' : '浏览器预览'));
function notify(text:string,error=false){ clearTimeout(toastTimer);toast.value={text,error};toastTimer=setTimeout(()=>toast.value=null,error?6500:3200); }
function errorMessage(error:unknown){return error instanceof Error?error.message:String(error);}
async function change(action:Mutation){busy.value=true;try{data.value=await bridge.mutate(action);return true;}catch(error){notify(errorMessage(error),true);return false;}finally{busy.value=false;}}
function selectView(next:string,group=''){view.value=next;category.value=group;search.value='';openMenu.value='';}
function relativeTime(timestamp:number|null){if(!timestamp)return '尚未使用';const minutes=Math.max(0,Math.floor((Date.now()-timestamp)/60000));if(minutes<1)return '刚刚使用';if(minutes<60)return `${minutes} 分钟前`;if(minutes<1440)return `${Math.floor(minutes/60)} 小时前`;return `${Math.floor(minutes/1440)} 天前`;}
function beginEdit(prompt?:Prompt){editing.value=prompt?{...prompt,tags:[...prompt.tags]}:{id:crypto.randomUUID(),title:'',content:'',category:category.value||DEFAULT_CATEGORY,tags:[],favorite:false,createdAt:Date.now(),updatedAt:Date.now(),lastUsedAt:null,useCount:0};editorTags.value=prompt?.tags.join('，')||'';editorError.value='';openMenu.value='';nextTick(()=>document.getElementById('prompt-title')?.focus());}
async function saveEditor(){if(!editing.value)return;const prompt={...editing.value,title:editing.value.title.trim(),category:editing.value.category.trim()||DEFAULT_CATEGORY,tags:[...new Set(editorTags.value.split(/[,，]/).map(t=>t.trim()).filter(Boolean))].slice(0,20),updatedAt:Date.now()};try{normalizeImport({version:1,categories:categories.value,prompts:[prompt],settings:data.value.settings});}catch(error){editorError.value=errorMessage(error);return;}if(await change({op:'upsert',prompt})){editing.value=null;notify('提示词已保存');}}
function openCategoryCreator(){categoryDraft.value='';categoryError.value='';showCategoryEditor.value=true;nextTick(()=>document.getElementById('category-name')?.focus());}
async function saveCategory(){let name:string;try{name=categoryName(categoryDraft.value);if(categories.value.includes(name))throw new Error('这个类别已存在。');}catch(error){categoryError.value=errorMessage(error);return;}if(await change({op:'categoryCreate',name})){showCategoryEditor.value=false;if(editing.value)editing.value.category=name;else selectView('all',name);notify('类别已创建');}}
async function deleteCategory(){const name=categoryDeleteCandidate.value;if(name&&await change({op:'categoryDelete',name})){categoryDeleteCandidate.value=null;selectView('all',DEFAULT_CATEGORY);notify('类别已删除，条目已移至默认类别');}}
function duplicate(prompt:Prompt){beginEdit({...prompt,id:crypto.randomUUID(),title:(prompt.title+' · 副本').slice(0,100),createdAt:Date.now(),updatedAt:Date.now(),lastUsedAt:null,useCount:0});}
async function remove(){if(deleteCandidate.value && await change({op:'delete',id:deleteCandidate.value.id})){deleteCandidate.value=null;notify('已删除提示词');}}
async function toggleFavorite(prompt:Prompt){await change({op:'favorite',id:prompt.id});}
async function dropOn(targetId:string){if(!draggedId.value||search.value||view.value==='recent')return;const ids=reorderPrompts(data.value.prompts,draggedId.value,targetId);draggedId.value='';await change({op:'reorder',ids});}
async function run(prompt:Prompt, action:Action=data.value.settings.defaultAction, resolved?:string){
  if(!canUse.value)return;
  const names=getVariables(prompt.content);
  if(names.length && resolved===undefined){variableRequest.value={prompt,action,names};variableValues.value=Object.fromEntries(names.map(name=>[name,'']));return;}
  sendingId.value=prompt.id;activeAction.value=action;openMenu.value='';
  try{const result=await bridge.act(prompt.id,resolved??prompt.content,action);notify(result.message);if(bridge.native)status.value=await bridge.getStatus();}
  catch(error){notify(errorMessage(error),true);}finally{sendingId.value='';activeAction.value=null;}
}
async function runVariables(){const request=variableRequest.value;if(!request)return;if(request.names.some(name=>!variableValues.value[name]?.trim())){notify('请填写所有变量。',true);return;}variableRequest.value=null;await run(request.prompt,request.action,renderTemplate(request.prompt.content,variableValues.value));}
async function refreshTargets(){refreshing.value=true;try{targets.value=await bridge.listTargets();}catch(error){notify(errorMessage(error),true);}finally{refreshing.value=false;}}
async function openTargets(){showTargets.value=true;await refreshTargets();}
async function chooseTarget(target:Target){try{status.value.target=await bridge.bindTarget(target.hwnd);showTargets.value=false;notify('已选择目标窗口，发送或填入时会自动定位输入框');}catch(error){notify(errorMessage(error),true);}}
function openSettings(){Object.assign(draftSettings,data.value.settings);showSettings.value=true;}
async function saveSettings(){if(await change({op:'settings',settings:{...draftSettings}})){showSettings.value=false;status.value=await bridge.getStatus();notify('设置已保存');}}
async function exportFile(){try{if(await bridge.exportData())notify('已导出提示词');}catch(error){notify(errorMessage(error),true);}}
async function pickImport(){try{if(bridge.native){importCandidate.value=await bridge.importDataNative();}else fileInput.value?.click();}catch(error){notify(errorMessage(error),true);}}
async function readImport(event:Event){const input=event.target as HTMLInputElement;const file=input.files?.[0];if(!file)return;try{if(file.size>10*1024*1024)throw new Error('导入文件不能超过 10 MB。');importCandidate.value=normalizeImport(JSON.parse(await file.text()));}catch(error){notify(errorMessage(error),true);}finally{input.value='';}}
async function confirmImport(mode:'merge'|'replace'){if(!importCandidate.value)return;if(await change({op:'import',data:importCandidate.value,mode})){importCandidate.value=null;notify('导入完成');}}
async function launchDock(){try{await bridge.showDock();}catch(error){notify(errorMessage(error),true);}}
function applyTheme(){document.documentElement.dataset.theme=data.value.settings.theme;}
watch(()=>data.value.settings.theme,applyTheme,{immediate:true});
function onKey(event:KeyboardEvent){if((event.ctrlKey||event.metaKey)&&event.key.toLowerCase()==='k'){event.preventDefault();searchEl.value?.focus();}if(event.key==='Escape'){if(showCategoryEditor.value)showCategoryEditor.value=false;else if(categoryDeleteCandidate.value)categoryDeleteCandidate.value=null;else if(editing.value)editing.value=null;else if(variableRequest.value)variableRequest.value=null;else if(showTargets.value)showTargets.value=false;else if(showSettings.value)showSettings.value=false;else if(showHelp.value)showHelp.value=false;else if(openMenu.value)openMenu.value='';else if(dock)void bridge.hideWindow();}}
async function init(){fatal.value='';try{data.value=await bridge.loadData();status.value=await bridge.getStatus();loaded.value=true;unlisteners.push(await bridge.onData(next=>data.value=next));unlisteners.push(await bridge.onTarget(target=>{status.value.target=target;})); }catch(error){fatal.value=errorMessage(error);}}
onMounted(()=>{void init();document.addEventListener('keydown',onKey);});
onUnmounted(()=>{unlisteners.forEach(fn=>fn());document.removeEventListener('keydown',onKey);clearTimeout(toastTimer);});
</script>

<template>
  <div class="app-shell" :class="{ 'is-dock': dock }" @click="openMenu = ''">
    <header class="titlebar" @mousedown.left="bridge.startDrag()" @dblclick="!dock && bridge.toggleMaximize()">
      <div class="titlebar-brand"><span class="brand-mark"><Layers :size="15" /></span><span>PromptDock</span><span v-if="dock" class="titlebar-subtitle">快捷面板</span></div>
      <div class="window-buttons" @mousedown.stop @dblclick.stop>
        <button v-if="dock" class="window-button" title="打开管理窗口" aria-label="打开管理窗口" @click="bridge.showManager()"><ArrowUpRight :size="15" /></button>
        <button v-if="!dock" class="window-button" aria-label="最小化" @click="bridge.minimize()"><Minus :size="15" /></button>
        <button v-if="!dock" class="window-button" aria-label="最大化或还原" @click="bridge.toggleMaximize()"><Maximize2 :size="13" /></button>
        <button class="window-button close-window" :title="bridge.native ? '收起到托盘' : '关闭预览'" aria-label="关闭窗口" @click="bridge.hideWindow()"><X :size="16" /></button>
      </div>
    </header>

    <div v-if="fatal" class="fatal-state"><CircleHelp :size="36" /><h2>暂时无法加载提示词</h2><p>{{ fatal }}</p><button class="button primary" @click="init">重新加载</button></div>
    <template v-else>
      <aside v-if="!dock" class="sidebar">
        <div class="workspace-label"><span class="workspace-avatar">P</span><div><strong>个人提示词库</strong><small>本地保存 · 随时可用</small></div></div>
        <nav class="primary-nav" aria-label="提示词导航">
          <button :class="{ active: view === 'all' && !category }" @click="selectView('all')"><LayoutGrid :size="18" /><span>全部提示词</span><small>{{ data.prompts.length }}</small></button>
          <button :class="{ active: view === 'favorites' }" @click="selectView('favorites')"><Star :size="18" /><span>我的收藏</span><small>{{ favoriteCount }}</small></button>
          <button :class="{ active: view === 'recent' }" @click="selectView('recent')"><Clock3 :size="18" /><span>最近使用</span></button>
        </nav>
        <div class="nav-section-label category-heading">类别<span>{{ categories.length }}</span><button class="icon-button" aria-label="新建类别" title="新建类别" :disabled="!canUse" @click="openCategoryCreator"><Plus :size="15" /></button></div>
        <nav class="category-nav" aria-label="提示词类别"><div v-for="group in categories" :key="group" class="category-nav-row"><button :class="{active:category===group}" :aria-label="'查看类别 '+group" @click="selectView('all',group)"><Folder :size="17" /><span>{{ group }}</span><small>{{ data.prompts.filter(p=>p.category===group).length }}</small></button><button v-if="group!==DEFAULT_CATEGORY" class="icon-button category-delete" :aria-label="'删除类别 '+group" title="删除类别" :disabled="!canUse" @click="categoryDeleteCandidate=group"><Trash2 :size="14" /></button></div></nav>
        <div class="sidebar-bottom"><button @click="showHelp=true"><CircleHelp :size="17" />使用指南</button><button @click="openSettings"><Settings2 :size="17" />偏好设置</button><div class="local-status"><span></span>数据仅保存在这台电脑</div></div>
      </aside>

      <main class="main-content">
        <div v-if="!bridge.native" class="preview-banner"><Monitor :size="15" /><span>界面预览 · 桌面发送功能需运行 Windows 版</span></div>
        <div v-if="!dock" class="page-header"><div><div class="eyebrow">YOUR PROMPT LIBRARY</div><h1>{{ heading }}</h1><p>让常用指令，触手可及。</p></div><div class="header-actions"><button class="button secondary" @click="launchDock"><PanelRight :size="16" />悬浮面板</button><button class="button primary" :disabled="!canUse" @click="beginEdit()"><Plus :size="17" />新建提示词</button></div></div>

        <div class="target-bar" :class="{unbound:!status.target}"><div class="target-indicator"><Monitor :size="16" /></div><button class="target-selector" :title="targetText" @click="openTargets"><span class="target-caption">{{ status.target ? (status.target.bound ? '发送至 · 已自动定位' : '发送至 · 自动定位输入框') : '发送至' }}</span><strong>{{ targetText }}</strong></button><button class="icon-button" title="选择或绑定目标窗口" aria-label="选择目标窗口" @click="openTargets"><ChevronDown :size="16" /></button><span v-if="!dock" class="target-action-label">默认操作：{{ actionNames[data.settings.defaultAction] }}</span></div>
        <div v-if="status.shortcutError" class="inline-warning"><Keyboard :size="16" />{{ status.shortcutError }}</div>

        <div class="library-toolbar"><div class="search-field"><Search :size="17" /><input ref="searchEl" v-model="search" type="search" aria-label="搜索提示词" placeholder="搜索标题、内容或标签…"><kbd v-if="!dock">Ctrl K</kbd></div><div v-if="!dock" class="toolbar-actions"><button class="icon-button" title="导入 JSON" aria-label="导入提示词" @click="pickImport"><ArrowDownToLine :size="17" /></button><button class="icon-button" title="导出 JSON" aria-label="导出提示词" @click="exportFile"><ArrowUpFromLine :size="17" /></button></div></div>
        <div v-if="dock" class="dock-tabs" aria-label="筛选提示词"><button :class="{active:view==='all'}" @click="selectView('all')">全部</button><button :class="{active:view==='favorites'}" @click="selectView('favorites')"><Star :size="13" />收藏</button><button :class="{active:view==='recent'}" @click="selectView('recent')">最近</button><button class="dock-add" aria-label="新增提示词" @click="beginEdit()"><Plus :size="16" /></button></div>
        <div v-if="!dock" class="results-heading"><span>{{ search ? '搜索结果' : '全部条目' }} <b>{{ visible.length }}</b></span><small>{{ view === 'recent' ? '按最近使用排序' : (search ? '匹配标题、正文与标签' : '拖动卡片调整顺序') }}</small></div>

        <div v-if="!loaded" class="empty-state"><LoaderCircle class="spin" :size="26" /><p>正在加载提示词…</p></div>
        <div v-else-if="!visible.length" class="empty-state"><div class="empty-icon"><Search :size="25" /></div><h3>{{ search ? '没有找到匹配的提示词' : view === 'favorites' ? '收藏常用的提示词' : view === 'recent' ? '这里会显示最近用过的提示词' : '添加你的第一条提示词' }}</h3><p>{{ search ? '试试其他关键词，或清空搜索。' : view === 'favorites' ? '点击卡片上的星标，就能在这里快速找到。' : '从一个经常重复的指令开始。' }}</p><button v-if="search" class="button secondary" @click="search=''">清空搜索</button><button v-else-if="view==='all'" class="button primary" @click="beginEdit()"><Plus :size="16" />新建提示词</button></div>

        <div v-else class="prompt-grid" :class="{ 'dock-grid': dock }">
          <article v-for="prompt in visible" :key="prompt.id" class="prompt-card" :class="{dragging:draggedId===prompt.id}" :draggable="!dock&&!search&&view!=='recent'&&!busy" @dragstart="draggedId=prompt.id" @dragend="draggedId=''" @dragover.prevent @drop.prevent="dropOn(prompt.id)">
            <div class="card-top"><div class="category-icon" :class="'category-'+prompt.category"><Folder :size="18" /></div><span class="card-category">{{ prompt.category }}</span><div class="card-tools"><button class="icon-button favorite-button" :class="{selected:prompt.favorite}" :title="prompt.favorite?'取消收藏':'收藏'" :aria-label="prompt.favorite?'取消收藏 '+prompt.title:'收藏 '+prompt.title" :aria-pressed="prompt.favorite" :disabled="!canUse" @click="toggleFavorite(prompt)"><Star :size="15" /></button><div class="menu-anchor"><button class="icon-button" :aria-label="'更多操作 '+prompt.title" @click.stop="openMenu=openMenu===prompt.id?'':prompt.id"><MoreHorizontal :size="17" /></button><div v-if="openMenu===prompt.id" class="dropdown-menu" @click.stop><button @click="beginEdit(prompt)"><Pencil :size="14" />编辑</button><button @click="duplicate(prompt)"><Layers :size="14" />创建副本</button><button class="danger-text" @click="deleteCandidate=prompt;openMenu='' "><Trash2 :size="14" />删除</button></div></div></div></div>
            <button class="card-body" :aria-label="'编辑 '+prompt.title" @click="beginEdit(prompt)"><h3>{{ prompt.title }}<Braces v-if="getVariables(prompt.content).length" :size="14" class="template-mark" /></h3><p>{{ prompt.content }}</p></button>
            <div v-if="!dock&&prompt.tags.length" class="card-tags"><span v-for="tag in prompt.tags.slice(0,3)" :key="tag">{{ tag }}</span></div>
            <div class="card-footer">
              <span class="usage-label"><Clock3 :size="12" />{{ relativeTime(prompt.lastUsedAt) }}</span>
              <div class="card-actions" role="group" :aria-label="prompt.title+' 的操作'">
                <button v-for="action in promptActions" :key="action" class="prompt-action" :class="{primary:action===data.settings.defaultAction}" :disabled="!canUse" :aria-label="actionNames[action]+' '+prompt.title" @click="run(prompt,action)">
                  <LoaderCircle v-if="sendingId===prompt.id&&activeAction===action" class="spin" :size="14" />
                  <Copy v-else-if="action==='copy'" :size="14" /><ArrowUpRight v-else-if="action==='insert'" :size="14" /><Send v-else :size="14" />
                  <span>{{ actionNames[action] }}</span>
                </button>
              </div>
            </div>
          </article>
        </div>
        <footer v-if="!dock" class="page-footer"><span><Keyboard :size="14" />{{ data.settings.shortcut }} 唤出快捷面板</span><button @click="showHelp=true">了解一键发送<ArrowUpRight :size="12" /></button></footer>
      </main>
      <footer v-if="dock" class="dock-footer"><span><Pin :size="12" />置顶快捷面板</span><button @click="bridge.showManager()">管理提示词<ArrowUpRight :size="13" /></button></footer>
    </template>

    <div v-if="editing" class="modal-backdrop" @mousedown.self="editing=null"><form class="modal editor-modal" role="dialog" aria-modal="true" aria-labelledby="editor-heading" @submit.prevent="saveEditor"><div class="modal-heading"><div><div class="eyebrow">PROMPT EDITOR</div><h2 id="editor-heading">{{ data.prompts.some(p=>p.id===editing?.id)?'编辑提示词':'新建提示词' }}</h2></div><button type="button" class="icon-button" aria-label="关闭编辑器" @click="editing=null"><X :size="20" /></button></div><label for="prompt-title">标题</label><input id="prompt-title" v-model="editing.title" maxlength="100" required placeholder="给这个提示词起一个好记的名字"><div class="form-columns"><div><label for="prompt-category">类别</label><div class="category-select-row"><select id="prompt-category" v-model="editing.category"><option v-for="group in categories" :key="group" :value="group">{{ group }}</option></select><button type="button" class="icon-button" aria-label="编辑器新建类别" title="新建类别" @click="openCategoryCreator"><Plus :size="17" /></button></div></div><div><label for="prompt-tags">标签 <span>用逗号分隔</span></label><input id="prompt-tags" v-model="editorTags" maxlength="400" placeholder="例如：审查，调试"></div></div><label for="prompt-content">提示词正文</label><textarea id="prompt-content" v-model="editing.content" required maxlength="100000" rows="9" placeholder="写下你希望重复使用的指令…" /><div class="editor-hint"><span><Braces :size="14" />用双花括号添加变量，例如：<code v-pre>{{主题}}</code></span><span>{{ editing.content.length.toLocaleString() }} 字</span></div><p v-if="editorError" class="form-error" role="alert">{{ editorError }}</p><div class="modal-footer"><label class="checkbox-label"><input v-model="editing.favorite" type="checkbox">加入收藏</label><div><button class="button secondary" type="button" @click="editing=null">取消</button><button class="button primary" type="submit" :disabled="busy"><Check :size="16" />保存提示词</button></div></div></form></div>

    <div v-if="showTargets" class="modal-backdrop" @mousedown.self="showTargets=false"><section class="modal target-modal" role="dialog" aria-modal="true" aria-labelledby="targets-heading"><div class="modal-heading"><div><div class="eyebrow">DESTINATION</div><h2 id="targets-heading">选择发送目标</h2></div><button class="icon-button" aria-label="关闭目标选择" @click="showTargets=false"><X :size="20" /></button></div><p class="modal-description">选择一个已打开的窗口。每次发送或填入都会自动定位聊天输入框，无需手动绑定。</p><div v-if="!bridge.native" class="notice-box">浏览器预览无法控制其他应用。请运行 Windows 桌面版后，在这里选择 Codex。</div><div v-else class="target-list"><button v-for="target in targets" :key="target.hwnd" :class="{selected:status.target?.hwnd===target.hwnd}" @click="chooseTarget(target)"><span class="target-app-icon"><Monitor :size="20" /></span><span><strong>{{ target.title }}</strong><small>{{ target.process }}</small></span><Check v-if="status.target?.hwnd===target.hwnd" :size="17" /></button><p v-if="!targets.length&&!refreshing" class="muted">没有可选择的窗口，请先打开 Codex。</p></div><div v-if="status.target" class="binding-box"><TargetIcon :size="18" /><div><strong>自动定位聊天输入框</strong><p>点击发送或填入时会自动查找输入框；已记录的输入框失效后也会自动重新查找。</p></div></div><div class="modal-footer"><span class="muted">{{ status.target?.bound?'已自动定位，失效后会自动重新查找':'发送或填入时自动寻找输入框' }}</span><button class="button secondary" :disabled="refreshing" @click="refreshTargets">{{ refreshing?'正在刷新…':'刷新窗口' }}</button></div></section></div>

    <div v-if="showSettings" class="modal-backdrop" @mousedown.self="showSettings=false"><form class="modal" role="dialog" aria-modal="true" aria-labelledby="settings-heading" @submit.prevent="saveSettings"><div class="modal-heading"><div><div class="eyebrow">PREFERENCES</div><h2 id="settings-heading">偏好设置</h2></div><button class="icon-button" type="button" aria-label="关闭设置" @click="showSettings=false"><X :size="20" /></button></div><label for="default-action">默认突出显示的操作</label><select id="default-action" v-model="draftSettings.defaultAction"><option value="send">填入并发送</option><option value="insert">仅填入</option><option value="copy">仅复制</option></select><label for="send-key">目标应用的发送快捷键</label><select id="send-key" v-model="draftSettings.sendKey"><option value="enter">Enter</option><option value="ctrlEnter">Ctrl + Enter</option></select><small class="field-hint">应与 Codex 设置中的发送方式保持一致。</small><label for="global-shortcut">唤出 / 收起快捷面板</label><input id="global-shortcut" v-model="draftSettings.shortcut" required maxlength="100" placeholder="Ctrl+Alt+Space"><label for="theme">外观</label><select id="theme" v-model="draftSettings.theme"><option value="system">跟随系统</option><option value="light">浅色</option><option value="dark">深色</option></select><div class="storage-location"><Folder :size="15" /><span>{{ status.dataPath }}</span></div><div class="modal-footer"><button class="button secondary" type="button" @click="showSettings=false">取消</button><button class="button primary" :disabled="busy" type="submit">保存设置</button></div></form></div>

    <div v-if="variableRequest" class="modal-backdrop" @mousedown.self="variableRequest=null"><form class="modal variable-modal" role="dialog" aria-modal="true" aria-labelledby="variables-heading" @submit.prevent="runVariables"><div class="modal-heading"><div><div class="eyebrow">TEMPLATE</div><h2 id="variables-heading">{{ variableRequest.prompt.title }}</h2></div><button type="button" class="icon-button" aria-label="关闭模板" @click="variableRequest=null"><X :size="20" /></button></div><p class="modal-description">填写这些内容，生成本次使用的提示词。支持多行输入和粘贴，Enter 换行。</p><div v-for="(name,index) in variableRequest.names" :key="name"><label :for="'variable-'+index">{{ name }}</label><textarea :id="'variable-'+index" v-model="variableValues[name]" required maxlength="5000" rows="4" :placeholder="'输入'+name" /></div><div class="modal-footer"><button type="button" class="button secondary" @click="variableRequest=null">取消</button><button class="button primary" type="submit"><Send :size="15" />{{ actionNames[variableRequest.action] }}</button></div></form></div>

    <div v-if="deleteCandidate" class="modal-backdrop" @mousedown.self="deleteCandidate=null"><section class="modal compact-modal" role="alertdialog" aria-modal="true" aria-labelledby="delete-heading"><div class="modal-heading"><h2 id="delete-heading">删除提示词？</h2><button class="icon-button" aria-label="取消删除" @click="deleteCandidate=null"><X :size="19" /></button></div><p class="modal-description">「{{ deleteCandidate.title }}」将从提示词库中移除。</p><div class="modal-footer"><button class="button secondary" @click="deleteCandidate=null">取消</button><button class="button danger" :disabled="busy" @click="remove">删除</button></div></section></div>

    <div v-if="importCandidate" class="modal-backdrop" @mousedown.self="importCandidate=null"><section class="modal compact-modal" role="dialog" aria-modal="true" aria-labelledby="import-heading"><div class="modal-heading"><h2 id="import-heading">导入 {{ importCandidate.prompts.length }} 条提示词</h2><button class="icon-button" aria-label="取消导入" @click="importCandidate=null"><X :size="19" /></button></div><p class="modal-description">合并会保留现有内容，相同 ID 使用较新的版本。替换会用导入文件覆盖当前提示词库和设置。</p><div class="import-actions"><button class="button primary" :disabled="busy" @click="confirmImport('merge')">合并到提示词库</button><button class="button secondary" :disabled="busy" @click="confirmImport('replace')">替换当前提示词库</button></div></section></div>

    <div v-if="showHelp" class="modal-backdrop" @mousedown.self="showHelp=false"><section class="modal help-modal" role="dialog" aria-modal="true" aria-labelledby="help-heading"><div class="modal-heading"><div><div class="eyebrow">GET STARTED</div><h2 id="help-heading">第一次使用</h2></div><button class="icon-button" aria-label="关闭指南" @click="showHelp=false"><X :size="20" /></button></div><ol class="help-steps"><li><strong>选择目标窗口</strong><p>打开 Codex 聊天，在上方“发送至”中选择对应窗口。</p></li><li><strong>输入框自动识别</strong><p>无需点击绑定按钮。每次发送或填入都会自动查找，输入框被重建后也会自动重新定位。</p></li><li><strong>打开悬浮面板</strong><p>点击右上角“悬浮面板”，或使用 {{ data.settings.shortcut }} 唤出。</p></li><li><strong>点击提示词发送</strong><p>默认粘贴提示词并直接触发发送，不检查草稿或用输入框文字拦截发送。已有文字按普通粘贴行为处理；卡片底部可直接选择复制、仅填入或发送。</p></li></ol><div class="notice-box">如果 Enter 只换行，请在偏好设置中将发送快捷键改为 Ctrl + Enter。普通权限的程序可能无法控制以管理员身份运行的目标窗口。</div><div class="modal-footer"><span class="muted">PromptDock 0.1.5 · 本地工具</span><button class="button primary" @click="showHelp=false">开始使用</button></div></section></div>
    <div v-if="showCategoryEditor" class="modal-backdrop" @mousedown.self="showCategoryEditor=false"><form class="modal compact-modal" role="dialog" aria-modal="true" aria-labelledby="category-editor-heading" @submit.prevent="saveCategory"><div class="modal-heading"><h2 id="category-editor-heading">新建类别</h2><button type="button" class="icon-button" aria-label="关闭新建类别" @click="showCategoryEditor=false"><X :size="19" /></button></div><label for="category-name">类别名称</label><input id="category-name" v-model="categoryDraft" required maxlength="40" placeholder="输入类别名称"><p v-if="categoryError" class="form-error" role="alert">{{ categoryError }}</p><div class="modal-footer"><button type="button" class="button secondary" @click="showCategoryEditor=false">取消</button><button type="submit" class="button primary" :disabled="busy">创建类别</button></div></form></div>
    <div v-if="categoryDeleteCandidate" class="modal-backdrop" @mousedown.self="categoryDeleteCandidate=null"><section class="modal compact-modal" role="alertdialog" aria-modal="true" aria-labelledby="category-delete-heading"><div class="modal-heading"><h2 id="category-delete-heading">删除类别？</h2><button class="icon-button" aria-label="取消删除类别" @click="categoryDeleteCandidate=null"><X :size="19" /></button></div><p class="modal-description">删除「{{ categoryDeleteCandidate }}」后，其中的 {{ data.prompts.filter(p=>p.category===categoryDeleteCandidate).length }} 条提示词会移到“默认”类别，提示词内容会保留。</p><div class="modal-footer"><button class="button secondary" @click="categoryDeleteCandidate=null">取消</button><button class="button danger" :disabled="busy" @click="deleteCategory">删除类别</button></div></section></div>
    <input ref="fileInput" class="hidden-input" type="file" accept="application/json,.json" @change="readImport">
    <div v-if="toast" class="toast" :class="{error:toast.error}" role="status" aria-live="polite"><CircleHelp v-if="toast.error" :size="17" /><Check v-else :size="17" /><span>{{ toast.text }}</span><button aria-label="关闭提示" @click="toast=null"><X :size="14" /></button></div>
  </div>
</template>
