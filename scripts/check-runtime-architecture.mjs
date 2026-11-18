// Verify the slim runtime graph without probing retired RSI/media engines.
import {execFileSync} from 'node:child_process'
import {existsSync,readFileSync,readdirSync} from 'node:fs'
import path from 'node:path'
import {fileURLToPath} from 'node:url'
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const meta=JSON.parse(execFileSync('cargo',['metadata','--format-version','1','--no-deps','--locked'],{cwd:root,encoding:'utf8'}))
const members=meta.packages.filter(pkg=>meta.workspace_members.includes(pkg.id))
const product=members.find(pkg=>pkg.name==='dominion-rust')
const core=members.find(pkg=>pkg.name==='dominion-core')
if(!product||!core)throw Error('Missing product/native core workspace member')
for(const pkg of members)if(!existsSync(pkg.manifest_path))throw Error('Missing manifest: '+pkg.manifest_path)
if(!product.dependencies.some(dep=>dep.name===core.name&&dep.path===path.dirname(core.manifest_path)))
  throw Error('Product must embed the Rust native execution core')
// Keep real-process E2E contracts focused while preventing a silent loss
// of the original cases during test refactors.
const e2eModules=['room_identity','room_collaboration','run_recovery',
  'experiments_backup','adaptation_program','preflight_tools','http_execution',
  'procedure_collaboration','codex_and_sdk']
const e2eRoot=readFileSync(path.join(root,'tests/e2e.rs'),'utf8')
if(e2eRoot.split('\n').length>1000)throw Error('E2E shared harness has grown too large')
let e2eCases=0
for(const name of e2eModules){
  const file='tests/e2e/'+name+'.rs'
  if(!e2eRoot.includes('mod '+name+';'))throw Error('E2E module missing from root: '+name)
  const body=readFileSync(path.join(root,file),'utf8')
  const lines=body.split('\n').length
  if(lines>1500)throw Error('E2E module grew past 1500 lines: '+file+' ('+lines+')')
  e2eCases+=(body.match(/^#\[test\]/gm)||[]).length
}
if(e2eCases!==106)throw Error('E2E regression tests lost or duplicated: '+e2eCases+' (expected 106)')

const runtime=readFileSync(path.join(root,'src/engine/chat/runtime.rs'),'utf8')
const adapter=readFileSync(path.join(root,'src/engine/chat/product.rs'),'utf8')
const chatPaths=[
 ['src/engine/chat/runtime.rs',1600],
 ['src/engine/chat/product.rs',2200],
 ['src/engine/chat/provider.rs',2200],
 ['src/engine/chat/runtime/program.rs',1200],
 ['src/engine/chat/runtime/tool_dispatch.rs',900],
 ['src/engine/chat/runtime/finalization.rs',1100],
 ['src/engine/chat/runtime/tool_catalog.rs',600],
 ['src/engine/chat/runtime/adaptation.rs',600],
 ['src/engine/chat/runtime/recovery.rs',350],
 ['src/engine/chat/runtime/tests.rs',1000],
 ['src/engine/chat/provider/tests.rs',1000],
]
for(const [file,maxLines] of chatPaths){
 const full=path.join(root,file)
 if(!existsSync(full))throw Error('Runtime boundary missing: '+file)
 const content=readFileSync(full,'utf8')
 const lines=content.split('\n').length
 if(lines>maxLines)throw Error('Runtime module grew past '+maxLines+' lines: '+file+' ('+lines+')')
}
for(const file of ['src/engine/chat/runtime.rs','src/engine/chat/product.rs',
    'src/engine/chat/runtime/tool_dispatch.rs','src/engine/chat/runtime/finalization.rs']){
 if(/\.all\("execution_operation"\)/.test(readFileSync(path.join(root,file),'utf8')))
   throw Error('Hot-path full tool journal scan reintroduced: '+file)
}

const outcome=readFileSync(path.join(root,'src/engine/chat/outcome.rs'),'utf8')
const driver=readFileSync(path.join(root,'crates/dominion-core/src/engine.rs'),'utf8')
const modules=readFileSync(path.join(root,'src/engine/mod.rs'),'utf8')
const quietTurn=modules.match(/pub async fn run_quiet\([\s\S]*?\n\}/)?.[0]
if(!quietTurn||quietTurn.includes('run_quiet_with_stats(')||
   !quietTurn.includes('chat::run_turn_background(')||
   !quietTurn.includes('drain_quiet_events(receiver)'))
  throw Error('Quiet background turns must drain events without collecting discarded tool evidence')
const tools=readFileSync(path.join(root,'src/engine/tools.rs'),'utf8')
if(!runtime.includes('dominion_core::engine::Engine::drive('))throw Error('Native driver must run product turns')
if(!outcome.includes('pub(super) fn determine_task_outcome(')||!runtime.includes('mod outcome;'))
  throw Error('Outcome policy must remain independently verifiable')
if(/while\s+!\s*(?:self\.)?finished/.test(runtime+adapter))throw Error('Legacy model retry loop reintroduced')
for(const phase of ['initialize','plan','model','verify','dispatch_tool','verify_tools'])
  if(!driver.includes(`host.${phase}(`))throw Error('Native driver missing phase: '+phase)
if(/^pub mod (rsi|video)(?:_|;)/m.test(modules)||/tool\("(?:rsi_[^"]*|algorithm_propose|unsafe_audit)"/.test(tools))
  throw Error('Retired generation engine or tool schema still registered')
for(const file of ['src/engine/video.rs','src/engine/rsi_evolve.rs','src/engine/rsi_worktree.rs'])
  if(existsSync(path.join(root,file)))throw Error('Retired engine source remains: '+file)
console.log(`Agent runtime graph OK: ${members.map(p=>p.name).join(', ')}`)

for(const file of readdirSync(path.join(root,'src/engine')))
  if(/^(?:rsi(?:_|\.)|video(?:_|\.))/.test(file))throw Error('Retired engine source remains: '+file)
const api=readFileSync(path.join(root,'src/api/mod.rs'),'utf8')
for(const line of api.split('\n'))if(/\.route\("\/api\/(?:rsi|video|youtube)(?:\/|\")/.test(line)&&/\bpost\(/.test(line))
  throw Error('Retired mutation endpoint is registered: '+line.trim())
const bootstrap=readFileSync(path.join(root,'src/bootstrap.rs'),'utf8')
if(bootstrap.includes('refresh_free_catalog('))throw Error('Idle provider-model sweep reintroduced')
// Check the transitive finalization and prompt-selection paths, not just
// the file that calls them: an indirect full-history scan is still O(history).
const improvementSource=readFileSync(path.join(root,'src/engine/improve.rs'),'utf8')
const finalizationUpdates=improvementSource.match(/pub fn execution_updates\([\s\S]*?\n\}/)?.[0]
const activeSelection=improvementSource.match(/pub fn active_values_with_execution\([\s\S]*?\n\}/)?.[0]
const recentBaseline=improvementSource.match(/fn baseline_success\([\s\S]*?\n\}/)?.[0]
if(!finalizationUpdates||
   /\.(?:try_all|all)\("(?:execution_operation|improvement)"\)/.test(finalizationUpdates)||
   !finalizationUpdates.includes('store.execution_operations(run_id)')||
   !finalizationUpdates.includes('store.try_records_by_ids("improvement"'))
  throw Error('Indirect full-table history scan returned to task finalization')
if(!activeSelection||
   /\.all\("improvement"\)/.test(activeSelection)||
   !activeSelection.includes('active_improvements_for_scope(')||
   (activeSelection.match(/choose_for_agent\(/g)||[]).length!==1)
  throw Error('Turn-time strategy catalog may rescan history or select one kernel per candidate')
if(!recentBaseline||
   /\.all\("execution"\)|\.rev\(\)\.take\(40\)/.test(recentBaseline)||
   !recentBaseline.includes('try_recent_records("execution", 40)'))
  throw Error('Strategy baseline must use recent indexed execution facts')
if(modules.includes('pub mod scheduler;')||bootstrap.includes('engine::scheduler::start(')||
   existsSync(path.join(root,'src/engine/scheduler.rs'))||
   /pub fn schedule_experiments\(/.test(improvementSource))
  throw Error('Retired autonomous experiment scheduling loop reintroduced')
if(/pub async fn run_quiet_with_stats\(/.test(modules))
  throw Error('Unused quiet-turn evidence wrapper reintroduced')
const store=readFileSync(path.join(root,'src/db.rs'),'utf8')
const jobs=readFileSync(path.join(root,'src/engine/jobs.rs'),'utf8')
const approval=readFileSync(path.join(root,'src/engine/chat/approval.rs'),'utf8')
if(!store.includes('pub fn pending_approvals(')||!approval.includes('store.pending_approvals('))
  throw Error('Indexed pending-approval expiry was bypassed')
if(!jobs.includes('store.running_recovery_records("execution")')||!jobs.includes('store.running_recovery_records("execution_operation")'))
  throw Error('Restart recovery must not deserialize every completed execution/operation')
if(!jobs.includes('store.uncertain_effect_sessions()')||!store.includes('pub fn uncertain_effect_sessions('))
  throw Error('Crash-uncertain tool effects must be examined once and fail closed')
if(!jobs.includes('context.finish_terminal(')||!jobs.includes('job.finalization_failed'))
  throw Error('Job success must have a durable terminal claim before audit')
const sdkApi=readFileSync(path.join(root,'src/api/sdk.rs'),'utf8')
if(!sdkApi.includes('state.store.reconcile_sdk_effect_once('))
  throw Error('SDK tool reconciliations must be a durable single claim')
if(!sdkApi.includes('state.store.sdk_evidence_for_session(session)')||
    !store.includes('pub fn sdk_evidence_for_session('))
  throw Error('SDK individual run polling must use its session-scoped evidence read')
if(!sdkApi.includes('state.store.sdk_evidence_rows()')||!store.includes('pub fn sdk_evidence_rows('))
  throw Error('SDK status polling must not scan full execution and tool history')
// Old optimizer control loops must not creep back in behind the real journal.
const algorithms=readFileSync(path.join(root,'src/engine/algorithms.rs'),'utf8')
const policySource=readFileSync(path.join(root,'src/engine/policy.rs'),'utf8')
const approvalSource=readFileSync(path.join(root,'src/engine/chat/approval.rs'),'utf8')
const startup=readFileSync(path.join(root,'src/migrations.rs'),'utf8')
if(/\bpub fn (?:meta_improve|propose_candidates)\(/.test(policySource))
  throw Error('Detached optimizer/RSI campaign loop reintroduced')
if(/\bUCB1 over\b|\(2\.0\s*\*\s*\(total\s*\+\s*1\.0\)/.test(algorithms))
  throw Error('Obsolete bandit selector reintroduced into active harness')
if(/\.all\("approval"\)/.test(approvalSource))
  throw Error('Tool approval rescans historical decisions')
if(/try_all\("(?:execution|execution_operation)"\)/.test(startup))
  throw Error('Restart migration rescans all completed execution payloads')
if(!store.includes('pub fn algorithms_for_kind(')||
   !algorithms.includes('state.store.algorithms_for_kind(kind)'))
  throw Error('Hot-path kernel selection must use indexed per-domain candidates')
if(!store.includes('pub fn legacy_improvements_to_migrate(')||
   !startup.includes('store.legacy_improvements_to_migrate()'))
  throw Error('Legacy improvement migration must not rewrite all records at every startup')
if(!store.includes('pub fn algorithm_usages_for_execution(')||
   !readFileSync(path.join(root,'src/engine/algorithms.rs'),'utf8')
     .includes('state.store.algorithm_usages_for_execution(execution_id)'))
  throw Error('Algorithm finalization must use indexed run-level usage records')

// Compare actual entry points and account for all extra model calls. A
// preflight/helper loop must not hide provider cost outside the run journal.
const helperSource=readFileSync(path.join(root,'src/engine/llm.rs'),'utf8')
const auxiliaryReceiptSource=readFileSync(path.join(root,'src/engine/llm/receipts.rs'),'utf8')
const finalizationSource=readFileSync(path.join(root,'src/engine/chat/runtime/finalization.rs'),'utf8')
for(const file of ['src/engine/chat/runtime/finalization.rs',
                   'src/engine/chat/runtime/adaptation.rs',
                   'src/engine/chat/runtime/program.rs']){
 const source=readFileSync(path.join(root,file),'utf8')
 if(!source.includes('llm::complete_for_execution(')||
    /llm::complete_for_(?:turn|session)\(/.test(source))
   throw Error('Unattributed auxiliary provider call in: '+file)
}
if(!helperSource.includes('mod receipts;')||
   !helperSource.includes('receipts::begin(')||
   !helperSource.includes('receipts::finish(')||
   !auxiliaryReceiptSource.includes('pub(crate) fn summarize(')||
   !finalizationSource.includes('store.auxiliary_model_calls(execution_id)')||
   !finalizationSource.includes('record["modelAccounting"]')||
   !store.includes('idx_aux_model_call_execution'))
  throw Error('Execution cost accounting no longer includes auxiliary upstream retries')
if(!sdkApi.includes('engine::run_quiet(&state,&request).await')||
   !runtime.includes('dominion_core::engine::Engine::drive(')||
   !sdkApi.includes('record["modelAccounting"]=run["modelAccounting"].clone()'))
  throw Error('SDK/chat execution or cost semantics diverged from common Rust core')

const frontend=readFileSync(path.join(root,'web/src/App.tsx'),'utf8')+readFileSync(path.join(root,'web/src/components/WorkspaceShell.tsx'),'utf8')
if(/VideoPage|SelfImprovementPage|id:'(?:video|rsi)'/.test(frontend))throw Error('Retired UI is reachable')
if(existsSync(path.join(root,'web/src/components/ImprovementNotice.tsx')))throw Error('Retired RSI prompt widget remains')
if(/resvg|comfyui/i.test(product.dependencies.map(d=>d.name).join(' ')))throw Error('Retired media renderer dependency remains')
for(const retired of ['dominion-harness','dominion-agent-sdk'])if(members.some(p=>p.name===retired)||product.dependencies.some(p=>p.name===retired))
  throw Error('Retired duplicate runtime returned: '+retired)
