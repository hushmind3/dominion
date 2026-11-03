import test from 'node:test';
import assert from 'node:assert/strict';
import fs from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const read=p=>fs.readFileSync(path.join(root,p),'utf8');
const pages=fs.readdirSync(path.join(root,'src/pages')).filter(x=>x.endsWith('.tsx'));

test('component redesign has a single Tailwind token source and zero-byte legacy stylesheet',()=>{
 assert.equal(read('src/style.css').length,0);
 const theme=read('src/design/tokens.css');
 assert.match(theme,/@import\s+"tailwindcss"/);
 assert.match(theme,/@theme\s*\{/);
 for(const token of ['--color-canvas','--color-surface','--color-ink','--color-muted','--color-line','--color-brand'])assert.ok(theme.includes(token),token);
 assert.match(read('src/main.tsx'),/design\/tokens\.css/);
});
test('all ten route pages delegate rendering to feature components, with no bespoke controls or classes',()=>{
 assert.equal(pages.length,10);
 for(const page of pages){
  const code=read('src/pages/'+page);
  assert.match(code,/export\s+\{/,'missing feature export '+page);
  assert.match(code,/from '\.\.\/features\//,'missing feature delegate '+page);
  assert.doesNotMatch(code,/className\s*=|<button\b|<input\b|<textarea\b|<table\b|style\s*=\s*\{/i,'legacy page markup in '+page);
 }
});
test('all feature screens consume shared design components and no inline CSS objects',()=>{
 const features=fs.readdirSync(path.join(root,'src/features')).filter(x=>x.endsWith('.tsx'));
 assert.ok(features.length>=9);
 for(const file of features){
  const code=read('src/features/'+file);
  if(file!=='ChatParts.tsx')assert.match(code,/from '\.\.\/design\/Primitives'/,'missing design composition: '+file);
  assert.doesNotMatch(code,/style\s*=\s*\{/,'inline CSS in '+file);
  assert.doesNotMatch(code,/<button\b|<input\b|<textarea\b|<table\b/,'raw controls outside design system: '+file);
 }
});
test('UI exposes a desktop hamburger and does not seed an external model application',()=>{
 const shell=read('src/design/AppShell.tsx');
 assert.match(shell,/toggle/);
 assert.match(shell,/lg:pl-\[74px\]/);
 assert.match(shell,/lg:pl-\[250px\]/);
 const providers=read('server/providers.mjs');
 assert.match(providers,/dominion-native/);
 assert.doesNotMatch(providers,/\{id:'lmstudio',name:'LM Studio',kind:'openai'/);
});
