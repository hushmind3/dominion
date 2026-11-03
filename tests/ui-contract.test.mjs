import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync,readdirSync} from 'node:fs';
import path from 'node:path';

const pages=path.resolve('src/pages');
test('pages use reusable design-system controls rather than raw form elements',()=>{
 for(const name of readdirSync(pages).filter(n=>n.endsWith('.tsx'))){
  const src=readFileSync(path.join(pages,name),'utf8');
  assert.doesNotMatch(src,/<(?:button|input|textarea|select|table)(?=[\s/>])/,
   name+' contains a raw control instead of a shared primitive');
 }
});
test('design tokens are centralized and GoalRunner is part of workflows',()=>{
 const css=readFileSync('src/style.css','utf8');
 for(const token of ['--color-brand:','--color-canvas:','--color-ink:','--radius-panel:'])assert.ok(css.includes(token));
 const primitive=readFileSync('src/components/Primitives.tsx','utf8');
 for(const name of ['Button','TextInput','TextArea','SelectInput','DataTable'])assert.ok(primitive.includes('export const '+name));
 const page=readFileSync('src/pages/Workflows.tsx','utf8');
 assert.ok(page.includes('<GoalRunner/>'));
});
