import {test} from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync, readdirSync, statSync, existsSync} from 'node:fs';
import {join, relative, resolve} from 'node:path';

const root=resolve(import.meta.dirname,'..');
const omitted=new Set(['.git','node_modules','target','dist','.dominion']);
const retired=String.fromCharCode(103,111,97,108);

function walk(dir){
  const found=[];
  for(const name of readdirSync(dir)){
    if(omitted.has(name))continue;
    const full=join(dir,name);
    if(statSync(full).isDirectory())found.push(...walk(full));
    else found.push(full);
  }
  return found;
}
test('fresh runtime tree contains no retired command, module or label',()=>{
  const files=walk(root);
  const textual=/\.(rs|ts|mjs|js|json|toml|md|html|css)$/;
  for(const file of files){
    const label=relative(root,file).toLowerCase();
    assert.ok(!label.includes(retired), 'retired path at '+label);
    if(textual.test(file)){
      const content=readFileSync(file,'utf8').toLowerCase();
      assert.ok(!content.includes(retired),'retired source marker in '+label);
    }
  }
});
test('browser and SDK use the only native task API',()=>{
  const required=['src/main.rs','src/api.rs','src/store.rs','src/engine.rs',
    'web/index.html','web/app.js','sdk/typescript/src/index.ts'];
  for(const path of required)assert.ok(existsSync(join(root,path)),path);
  const route=readFileSync(join(root,'src/api.rs'),'utf8');
  const sdk=readFileSync(join(root,'sdk/typescript/src/index.ts'),'utf8');
  const ui=readFileSync(join(root,'web/app.js'),'utf8');
  assert.match(route,/\/api\/tasks/);
  assert.match(sdk,/\/api\/tasks/);
  assert.match(ui,/\/api\/tasks/);
});
