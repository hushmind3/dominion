// Active source / UI / backend route contract gate for dominion-sub2.
// Intentionally excludes archived docs and persisted user SQLite data.
import {existsSync,readFileSync,readdirSync} from 'node:fs';
import {join,resolve} from 'node:path';
import {fileURLToPath} from 'node:url';

const root=resolve(fileURLToPath(new URL('../',import.meta.url)));
const source=(path)=>readFileSync(join(root,path),'utf8');
const errors=[];
const expect=(ok,message)=>{if(!ok)errors.push(message);};
const retired=[
  'src/api/money','src/engine/money','src/db/money.rs',
  'web/src/pages/Money.tsx','web/src/refreshScope.ts',
  'src/api/rsi.rs','src/api/video.rs','src/engine/goal_control.rs',
  'web/src/pages/Video.tsx','web/src/pages/SelfImprovement.tsx',
  'rsi/evaluations','benchmark/rsi-evidence',
  // Obsolete documentation that incorrectly advertised the withdrawn APIs.
  'docs/SUB2-RUNTIME-VIDEO-2026-11-15.md',
  'docs/SUB2-VIDEO-CANCEL-CHECKPOINT-2026-11-16.md',
  'docs/SUB2-VIDEO-POSTPRODUCTION-2026-11-16.md',
  'docs/SUB2-LEAN-CUTOVER-2026-11-16.md',
  'docs/SUB2-LEAN-CUTOVER-2026-11-17.md',
  'docs/SUB2-SDK-VIDEO-ARCHIVE.md',
  'docs/SUB2-SOURCE-AUDIT-2026-11-18.md',
];
for(const path of retired)expect(!existsSync(join(root,path)),`retired path remains: ${path}`);
const codePaths=[];
const visit=(dir)=>{
  const full=join(root,dir);
  if(!existsSync(full))return;
  for(const entry of readdirSync(full,{withFileTypes:true})){
    const path=join(dir,entry.name);
    if(entry.isDirectory()){if(entry.name!=='node_modules')visit(path);}
    else if(/\.(?:rs|ts|tsx|js|jsx|mjs|sql|ya?ml|json)$/.test(entry.name))codePaths.push(path);
  }
};
// Scan every active runtime surface, including Cloudflare, deploy routing,
// SQL schemas, frontend tests and public SDK. Historical docs and user DB
// records remain archived; they are not executable feature remnants.
for(const dir of [
  'src','web/src','web/tests','web/shared','sdk','tests','crates',
  'src-tauri/src','scripts','cloud','services','.github/workflows','assets/channels'
])visit(dir);
for(const path of codePaths){
  if(path==='scripts/check-sub2-contract.mjs')continue; // The gate's own banned-symbol regex is not product code.
  const text=source(path);
  const forbidden=[
    ['Money',/\bmoney(?:_|::|\/|\b)|MoneyPage|\/api\/money|\b(?:money|opportunity)_(?:miner|scanner|worker|scheduler)\b/i],
    ['embedded media',/\bvideo(?:_|\b)|\byoutube(?:_|\b)|\/api\/(?:video|youtube)\b|\bVideoJob\b|\barchived_media_job\b/i],
    ['independent RSI',/\brsi(?:_|\b|[A-Z])|\bRsi[A-Z]|\/api\/rsi\b/i],
    ['orphan goal controller',/\bgoal_control\b|\bGoalControl\b/i],
  ];
  for(const [feature,pattern] of forbidden){
    if(pattern.test(text))errors.push(`retired ${feature} reference in ${path}`);
  }
}
const router=source('src/api/mod.rs');
const handlers=source('src/api/providers.rs');
const paths=[...router.matchAll(/\.route\(\s*"(\/api\/[^"]+)"/g)].map(match=>match[1]);
const seen=new Set();
for(const path of paths){
  if(seen.has(path))errors.push(`duplicate route: ${path}`);
  seen.add(path);
}
for(const path of [
  '/api/providers/catalog',
  '/api/providers/catalog/:id/models',
  '/api/providers/openrouter/free-models',
  '/api/providers/:id/models',
  '/api/oauth/providers',
  '/api/oauth/connections',
  '/api/rooms',
  '/api/model-library/catalog',
  '/api/model-library/runtime',
  '/api/providers/cli/:source/login',
]){
  expect(seen.has(path),`main frontend needs missing backend route: ${path}`);
}
for(const handler of ['public_catalog_models','public_openrouter_free_catalog','provider_models']){
  expect(handlers.includes(`fn ${handler}(`),`missing model handler: ${handler}`);
}
for(const path of [
  'web/src/components/DominionCloudAccount.tsx',
  'web/src/components/ConnectedAccounts.tsx',
  'web/src/refreshEvents.ts',
  'web/src/utils/modelFree.ts',
  'web/src/utils/modelHardware.ts',
]){
  expect(existsSync(join(root,path)),`missing main frontend component: ${path}`);
}
const app=source('web/src/App.tsx');
expect(app.includes("from './refreshEvents'"),'App is not wired to main refresh scope');
expect(!/MoneyPage|page==='money'/.test(app),'Money page still mounted');
const oauth=source('src/api/cli_oauth.rs');
expect(oauth.includes('import_cli_github'),'GitHub CLI UI has no backend handler');
// The public documentation index is the entrypoint to current contracts.
// Check actual relative paths rather than trusting outdated markdown tables.
const docsIndex=source('docs/INDEX.md');
for(const [,linked] of docsIndex.matchAll(/\]\(([^)#]+)(?:#[^)]+)?\)/g)){
  if(/^(?:https?:|mailto:|#)/i.test(linked))continue;
  const path=decodeURIComponent(linked);
  expect(existsSync(resolve(root,'docs',path)),`broken current documentation link: ${path}`);
}
if(errors.length){
  console.error(`DOMINION sub2 contract gate failed (${errors.length}):`);
  for(const issue of errors)console.error(` - ${issue}`);
  process.exitCode=1;
}else{
  console.log(`DOMINION sub2 gate OK: ${codePaths.length} source files, ${paths.length} API routes; main UI + credential-free model discovery + no retired Money/video/RSI/goal-control runtime references.`);
}
