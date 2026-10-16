import test from 'node:test';
import assert from 'node:assert/strict';
import legacy from '../src-tauri/legacy-default-data.json' with { type: 'json' };
import { DEFAULT_CATEGORY, applyMutation, migrateStoredData, normalizeImport, seedData } from '../src/domain.mjs';

test('starter library has exactly one default category and one test prompt',()=>{
  const data=seedData();assert.deepEqual(data.categories,['默认']);assert.equal(data.prompts.length,1);
  assert.equal(data.prompts[0].title,'test');assert.equal(data.prompts[0].content,'test');assert.equal(data.prompts[0].category,'默认');
});
test('empty categories survive round trips and deleting their last prompt',()=>{
  let data=applyMutation(seedData(),{op:'categoryCreate',name:'  空类别  '});assert.ok(data.categories.includes('空类别'));
  data=normalizeImport(JSON.parse(JSON.stringify(data)));assert.ok(data.categories.includes('空类别'));
  data=applyMutation(data,{op:'delete',id:'starter-test'});assert.deepEqual(data.categories,['默认','空类别']);assert.equal(data.prompts.length,0);
  assert.equal(migrateStoredData(data).prompts.length,0);
});
test('deleting a populated category moves prompts without losing content or usage',()=>{
  let data=applyMutation(seedData(),{op:'categoryCreate',name:'工作'});
  data.prompts[0].category='工作';data.prompts[0].useCount=7;data.prompts[0].lastUsedAt=100;
  const changed=applyMutation(data,{op:'categoryDelete',name:'工作'});
  assert.deepEqual(changed.categories,['默认']);assert.equal(changed.prompts.length,1);
  assert.equal(changed.prompts[0].category,DEFAULT_CATEGORY);assert.equal(changed.prompts[0].content,data.prompts[0].content);
  assert.equal(changed.prompts[0].useCount,7);assert.equal(changed.prompts[0].lastUsedAt,100);
});
test('category validation rejects duplicates, invalid names and deleting default',()=>{
  const data=seedData(),original=JSON.stringify(data);
  for(const name of ['默认',' ','x'.repeat(41)])assert.throws(()=>applyMutation(data,{op:'categoryCreate',name}));
  assert.throws(()=>applyMutation(data,{op:'categoryDelete',name:'默认'}));assert.throws(()=>applyMutation(data,{op:'categoryDelete',name:'missing'}));
  assert.throws(()=>normalizeImport({...data,categories:'bad'}));assert.equal(JSON.stringify(data),original);
});
test('saving an editor opened before category deletion cannot resurrect the category',()=>{
  const data=applyMutation(seedData(),{op:'categoryCreate',name:'工作'});data.prompts[0].category='工作';const stale=data.prompts[0];
  const changed=applyMutation(data,{op:'categoryDelete',name:'工作'});
  assert.throws(()=>applyMutation(changed,{op:'upsert',prompt:stale}),/类别已不存在/);assert.ok(!changed.categories.includes('工作'));
});
test('imports preserve empty categories in both merge and replace modes',()=>{
  const current=applyMutation(seedData(),{op:'categoryCreate',name:'本地空类别'});
  const incoming={...seedData(),categories:['默认','导入空类别']};
  assert.deepEqual(applyMutation(current,{op:'import',data:incoming,mode:'merge'}).categories,['默认','本地空类别','导入空类别']);
  assert.deepEqual(applyMutation(current,{op:'import',data:incoming,mode:'replace'}).categories,['默认','导入空类别']);
});
test('explicitly importing old backups preserves their prompts and infers categories',()=>{
  const imported=normalizeImport(legacy);assert.equal(imported.prompts.length,9);assert.ok(imported.categories.includes('编程'));assert.equal(imported.categories[0],'默认');
});
test('stored legacy defaults are removed once while edits and user entries survive',()=>{
  const stored=structuredClone(legacy);stored.prompts[0].title='我修改过的标题';
  stored.prompts.push({...stored.prompts[0],id:'mine',title:'我的提示词',category:'我创建的类别'});
  const migrated=migrateStoredData(stored);
  assert.deepEqual(migrated.prompts.map(p=>p.id),['review','mine','starter-test']);
  assert.deepEqual(migrated.categories,['默认','编程','我创建的类别']);assert.equal(migrated.prompts[0].title,'我修改过的标题');
  assert.deepEqual(migrateStoredData(JSON.parse(JSON.stringify(migrated))),migrated);
  const clean=migrateStoredData(legacy);assert.deepEqual(clean.categories,['默认']);assert.deepEqual(clean.prompts.map(p=>p.title),['test']);
});
