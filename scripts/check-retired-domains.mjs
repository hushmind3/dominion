// Single source-of-truth regression guard for retired product features.
import {existsSync,readFileSync,readdirSync} from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const read=p=>readFileSync(path.join(root,p),'utf8');
const errors=[];
for(const dir of ['src/api','src/db','src/engine','web/src/pages']){
  for(const entry of readdirSync(path.join(root,dir),{withFileTypes:true})){
    if(/^money(?:[_\.]|$)/i.test(entry.name) || entry.name.toLowerCase()==='money'){
      errors.push('Retired code must not return: '+dir+'/'+entry.name);
    }
  }
}
for(const [file,pattern] of [
  ['src/api/mod.rs',/money_routes|(?:mod|pub mod)\s+money\s*;/],
  ['src/engine/mod.rs',/(?:mod|pub mod)\s+money(?:_|\s*;)/],
  ['src/engine/jobs.rs',/money_tool::/],
  ['src/db.rs',/(?:mod money\s*;|money::SCHEMA)/],
  ['web/src/App.tsx',/MoneyPage|page\s*===\s*['"]money['"]/],
  ['web/src/components/WorkspaceShell.tsx',/['"]nav\.money['"]|['"]money['"]\s*\|/],
  ['web/src/pages/Operations.tsx',/api\/settings\/money|moneyModule|changeMoney/],
  ['.github/workflows/ci.yml',/cargo test[^\n]*\bmoney_/],
]){
  if(pattern.test(read(file)))errors.push('Active retired feature reference: '+file);
}
if(existsSync(path.join(root,'docs/MONEY.md')))errors.push('Retired documentation is still active: docs/MONEY.md');
if(errors.length){console.error(errors.join('\n'));process.exitCode=1;}
else console.log('Retired Money domain: no active code, routes, navigation or scheduler');
