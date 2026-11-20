// Inspect the native runtime graph and existing delivery contracts.
import {execFileSync} from 'node:child_process'
import {existsSync,readFileSync} from 'node:fs'
import path from 'node:path'
import {fileURLToPath} from 'node:url'
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
const meta=JSON.parse(execFileSync('cargo',['metadata','--format-version','1','--no-deps','--locked'],{cwd:root,encoding:'utf8'}))
const members=meta.packages.filter(pkg=>meta.workspace_members.includes(pkg.id))
const product=members.find(pkg=>pkg.name==='dominion-rust')
const core=members.find(pkg=>pkg.name==='dominion-core')
const algorithms=members.find(pkg=>pkg.name==='dominion-algorithms')
if(!product||!core||!algorithms)throw Error('Missing product/native core/algorithm workspace member')
for(const pkg of members)if(!existsSync(pkg.manifest_path))throw Error('Missing manifest: '+pkg.manifest_path)
if(!product.dependencies.some(dep=>dep.name===core.name&&dep.path===path.dirname(core.manifest_path))||
   !product.dependencies.some(dep=>dep.name===algorithms.name&&dep.path===path.dirname(algorithms.manifest_path)))
  throw Error('Product gateway must compose core and algorithms')
if(!algorithms.dependencies.some(dep=>dep.name===core.name&&dep.path===path.dirname(core.manifest_path))||
   core.dependencies.some(dep=>dep.name===algorithms.name))
  throw Error('Algorithm dependency must point towards core, never backwards')
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
if(e2eCases!==111)throw Error('E2E regression tests lost or duplicated: '+e2eCases+' (expected 111)')

const runtime=readFileSync(path.join(root,'src/engine/chat/runtime.rs'),'utf8')
// Product-specific chat phases must be owned outside the native chat directory.
// This keeps the native execution loop small without changing its module API.
const chatModule=readFileSync(path.join(root,'src/engine/chat/mod.rs'),'utf8')
for(const [owner,original,replacement] of [["src/engine/chat/runtime.rs","src/engine/chat/product.rs","src/addons/integrations/chat_product.rs"],["src/engine/chat/mod.rs","src/engine/chat/context.rs","src/addons/knowledge/chat_context.rs"],["src/engine/chat/mod.rs","src/engine/chat/room_context.rs","src/addons/integrations/chat_room_context.rs"],["src/engine/chat/runtime.rs","src/engine/chat/runtime/program.rs","src/addons/strategies/chat_program.rs"],["src/engine/chat/runtime.rs","src/engine/chat/runtime/adaptation.rs","src/addons/strategies/chat_adaptation.rs"]]){
  const declared=owner==='src/engine/chat/mod.rs'?chatModule:
    readFileSync(path.join(root,owner),'utf8')
  if(!declared.includes('../../'+replacement.replace(/^src\//,''))||
     existsSync(path.join(root,original))||
     !existsSync(path.join(root,replacement)))
    throw Error('Chat product extension leaked into engine: '+original)
}
const boardAddonIndex=readFileSync(path.join(root,'src/addons/mod.rs'),'utf8')
const board=readFileSync(path.join(root,'src/addons/integrations/room_board.rs'),'utf8')
const rooms=readFileSync(path.join(root,'src/addons/integrations/rooms.rs'),'utf8')
const boardRoomContext=readFileSync(path.join(root,'src/addons/integrations/chat_room_context.rs'),'utf8')
const dbSource=readFileSync(path.join(root,'src/db.rs'),'utf8')
const roomCursor=readFileSync(path.join(root,'src/db/room_cursor.rs'),'utf8')
if(!boardAddonIndex.includes('pub mod room_board;')||
   !rooms.includes('crate::addons::room_board::publish(')||
   !boardRoomContext.includes('crate::addons::room_board::context_frame(')||
   !board.includes('operation["toolReceipt"]["verified"]==true')||
   !dbSource.includes('CREATE INDEX IF NOT EXISTS idx_room_agent_cursor_session')||
   !roomCursor.includes('pub fn commit_room_agent_read(')||
   !rooms.includes('state.store.commit_room_agent_read(')||
   !boardRoomContext.includes('room_agent_read(')||
   !dbSource.includes('CREATE INDEX IF NOT EXISTS idx_room_work_source')||
   !dbSource.includes('pub fn room_work_receipts('))
  throw Error('Room work board lost verified receipt provenance or scoped SQLite admission')
const adapter=readFileSync(path.join(root,'src/addons/integrations/chat_product.rs'),'utf8')
const chatPaths=[
 ['src/engine/chat/runtime.rs',1600],
 ['src/addons/integrations/chat_product.rs',2200],
 ['src/engine/chat/provider.rs',2200],
 ['src/addons/strategies/chat_program.rs',1200],
 ['src/engine/chat/runtime/tool_dispatch.rs',900],
 ['src/engine/chat/runtime/finalization.rs',1100],
 ['src/addons/strategies/chat_tool_catalog.rs',600],
 ['src/addons/strategies/chat_adaptation.rs',600],
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
for(const file of ['src/engine/chat/runtime.rs','src/addons/integrations/chat_product.rs',
    'src/engine/chat/runtime/tool_dispatch.rs','src/engine/chat/runtime/finalization.rs']){
 if(/\.all\("execution_operation"\)/.test(readFileSync(path.join(root,file),'utf8')))
   throw Error('Hot-path full tool journal scan reintroduced: '+file)
}

const activeContract=readFileSync(path.join(root,'ENGINE.md'),'utf8')
const harnessContract=readFileSync(path.join(root,'docs/HARNESS.md'),'utf8')
const extensionContract=readFileSync(path.join(root,'docs/EXTENSION-HOST-2026-11-20.md'),'utf8')
for(const source of [activeContract,harnessContract,extensionContract]){
  if(source.includes('crates/dominion-core/src/engine.rs')||
     !source.includes('dominion-algorithms'))
    throw Error('Active architecture docs disagree with the single Rust harness owner')
}
const outcome=readFileSync(path.join(root,'src/engine/chat/outcome.rs'),'utf8')
const driver=readFileSync(path.join(root,'crates/dominion-algorithms/src/engine.rs'),'utf8')
const nativePort=readFileSync(path.join(root,'crates/dominion-core/src/runtime_port.rs'),'utf8')
const coreLib=readFileSync(path.join(root,'crates/dominion-core/src/lib.rs'),'utf8')
const algorithmsLib=readFileSync(path.join(root,'crates/dominion-algorithms/src/lib.rs'),'utf8')
if(coreLib.includes('pub mod context;')||
   !algorithmsLib.includes('pub mod context;')||
   !existsSync(path.join(root,'crates/dominion-algorithms/src/context.rs')))
  throw Error('Context projection algorithm returned to infrastructure core')
if(coreLib.includes('pub mod engine;')||
   !coreLib.includes('pub mod runtime_port;')||
   nativePort.includes('pub struct Engine;')||
   !nativePort.includes('pub trait Host: Send')||
   !nativePort.includes('pub trait AgentLoop<H: Host>')||
   !driver.includes('impl<H: Host> dominion_core::runtime_port::AgentLoop<H> for Engine')||
   !driver.includes('pub async fn drive<H: Host>')||
   !driver.includes('dominion_core::runtime_port::'))
  throw Error('Agent iteration logic leaked into core or algorithm crate missing')
const modules=readFileSync(path.join(root,'src/engine/mod.rs'),'utf8')
const allowedExecutionModules=new Set(["effect","chat","capability","verification","recovery","lease","llm","policy","router","run_state","tools","execution_budget","sandbox","workspace_scope"])
const declaredExecutionModules=[...modules.matchAll(/^pub mod ([a-z_]+);$/gm)].map(m=>m[1])
if(declaredExecutionModules.length!==allowedExecutionModules.size||
   declaredExecutionModules.some(m=>!allowedExecutionModules.has(m)))
  throw Error('Optional module added to native execution core; register it under src/addons instead')
const addonIndex=readFileSync(path.join(root,'src/addons/mod.rs'),'utf8')
const strategyPort=readFileSync(path.join(root,'crates/dominion-core/src/strategy.rs'),'utf8')
const strategyOwner=readFileSync(path.join(root,'src/addons/strategies/turn_policy.rs'),'utf8')
if(!strategyPort.includes('pub trait TurnStrategyPort')||
   !strategyOwner.includes('impl TurnStrategyPort for ScopedAlgorithms')||
   !runtime.includes('crate::addons::runtime_hooks::strategy_port(')||
   runtime.includes('ScopedAlgorithms::new(')||
   !runtime.includes('intelligence.execution_limits(')||
   !runtime.includes('intelligence.decompose(')||
   !runtime.includes('intelligence.tool_discovery(')||
   !runtime.includes('intelligence.needs_plan(')||
   runtime.includes('crate::engine::decompose::')||
   runtime.includes('"execution.budget"')||
   runtime.includes('"tool.discovery"')||
   runtime.includes('"task.decomposition"')||
   !runtime.includes('../../addons/strategies/chat_tool_catalog.rs'))
  throw Error('Algorithm implementation or selector leaked into native turn loop')
const runtimeHooks=readFileSync(path.join(root,'src/addons/runtime_hooks.rs'),'utf8')
if(!addonIndex.includes('pub mod runtime_hooks;')||
   !runtimeHooks.includes('pub fn prepare_turn(')||
   !runtimeHooks.includes('pub fn strategies_enabled_for_execution(')||
   !runtime.includes('crate::addons::runtime_hooks::prepare_turn(')||
   !runtime.includes('crate::addons::runtime_hooks::after_turn(')||
   runtime.includes('crate::engine::improve::active_values_with_execution('))
  throw Error('Optional strategy lifecycle regained direct core ownership')
const benchmarkRunner=readFileSync(path.join(root,'src/bin/dominion-benchmark.rs'),'utf8')
if(!benchmarkRunner.includes('../addons/strategies/plan_quality.rs')||
   benchmarkRunner.includes('../engine/plan_quality.rs'))
  throw Error('Benchmark must import the extracted plan-quality add-on')
const addonModules=[{"group":"knowledge","name":"agent_growth"},{"group":"knowledge","name":"distill"},{"group":"knowledge","name":"experience"},{"group":"knowledge","name":"learning"},{"group":"knowledge","name":"memory"},{"group":"knowledge","name":"principle"},{"group":"knowledge","name":"reflect"},{"group":"knowledge","name":"room_knowledge"},{"group":"knowledge","name":"skills"},{"group":"strategies","name":"adaptation"},{"group":"strategies","name":"adaptation_loop"},{"group":"strategies","name":"algorithm_eval"},{"group":"strategies","name":"algorithms"},{"group":"strategies","name":"debate"},{"group":"strategies","name":"decompose"},{"group":"strategies","name":"episode"},{"group":"strategies","name":"improve"},{"group":"strategies","name":"plan_quality"},{"group":"strategies","name":"policy_patch"},{"group":"strategies","name":"preflight_fixture"},{"group":"strategies","name":"procedure"},{"group":"integrations","name":"attachments"},{"group":"integrations","name":"bridge"},{"group":"integrations","name":"browser"},{"group":"integrations","name":"channel_adapter"},{"group":"integrations","name":"cli_model"},{"group":"integrations","name":"code_index"},{"group":"integrations","name":"codex_oauth"},{"group":"integrations","name":"dev"},{"group":"integrations","name":"discord"},{"group":"integrations","name":"extensions"},{"group":"integrations","name":"gateway"},{"group":"integrations","name":"mcp"},{"group":"integrations","name":"models"},{"group":"integrations","name":"oauth"},{"group":"integrations","name":"personas"},{"group":"integrations","name":"room_state"},{"group":"integrations","name":"rooms"},{"group":"integrations","name":"telegram"},{"group":"operations","name":"backup_keys"},{"group":"operations","name":"maintenance"},{"group":"operations","name":"remote_backup"},{"group":"operations","name":"vault_rotation"},{"group":"knowledge","name":"compact"},{"group":"strategies","name":"goal_control"},{"group":"strategies","name":"obligation_repair"},{"group":"strategies","name":"program"},{"group":"strategies","name":"reasoning"},{"group":"integrations","name":"delegation_worktree"},{"group":"operations","name":"jobs"},{"group":"operations","name":"wake"},{"group":"operations","name":"job_control"}]
for(const {group,name} of addonModules){
  const suffix=['adaptation','extensions'].includes(name)?name+'/mod.rs':name+'.rs'
  if(!addonIndex.includes('pub mod '+name+';')||
     !addonIndex.includes(group+'/'+suffix)||
     !existsSync(path.join(root,'src/addons',group,suffix))||
     modules.includes('pub mod '+name+';'))
    throw Error('Optional implementation leaked into execution core: '+name)
}
if(!modules.includes('pub use crate::addons::{'))
  throw Error('Product add-on compatibility exports missing')
const quietTurn=modules.match(/pub async fn run_quiet\([\s\S]*?\n\}/)?.[0]
if(!quietTurn||quietTurn.includes('run_quiet_with_stats(')||
   !quietTurn.includes('chat::run_turn_background(')||
   !quietTurn.includes('drain_quiet_events(receiver)'))
  throw Error('Quiet background turns must drain events without collecting discarded tool evidence')
if(!runtime.includes('dominion_core::runtime_port::AgentLoop::run('))throw Error('Native driver must run product turns')
if(!outcome.includes('pub(super) fn determine_task_outcome(')||!runtime.includes('mod outcome;'))
  throw Error('Outcome policy must remain independently verifiable')
if(/while\s+!\s*(?:self\.)?finished/.test(runtime+adapter))throw Error('Legacy model retry loop reintroduced')
for(const phase of ['initialize','plan','model','verify','dispatch_tool','verify_tools'])
  if(!driver.includes(`host.${phase}(`))throw Error('Native driver missing phase: '+phase)
console.log(`Agent runtime graph OK: ${members.map(p=>p.name).join(', ')}`)
const bootstrap=readFileSync(path.join(root,'src/bootstrap.rs'),'utf8')
if(bootstrap.includes('refresh_free_catalog('))throw Error('Idle provider-model sweep reintroduced')
// Check the transitive finalization and prompt-selection paths, not just
// the file that calls them: an indirect full-history scan is still O(history).
const improvementSource=readFileSync(path.join(root,'src/addons/strategies/improve.rs'),'utf8')
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
if(/pub async fn run_quiet_with_stats\(/.test(modules))
  throw Error('Unused quiet-turn evidence wrapper reintroduced')
const store=readFileSync(path.join(root,'src/db.rs'),'utf8')
const jobs=readFileSync(path.join(root,'src/addons/operations/jobs.rs'),'utf8')
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
const selectedAlgorithms=readFileSync(path.join(root,'src/addons/strategies/algorithms.rs'),'utf8')
const policySource=readFileSync(path.join(root,'src/engine/policy.rs'),'utf8')
const approvalSource=readFileSync(path.join(root,'src/engine/chat/approval.rs'),'utf8')
const startup=readFileSync(path.join(root,'src/migrations.rs'),'utf8')
if(/\bpub fn (?:meta_improve|propose_candidates)\(/.test(policySource))
  throw Error('Detached optimizer/RSI campaign loop reintroduced')
if(/\bUCB1 over\b|\(2\.0\s*\*\s*\(total\s*\+\s*1\.0\)/.test(selectedAlgorithms))
  throw Error('Obsolete bandit selector reintroduced into active harness')
if(/\.all\("approval"\)/.test(approvalSource))
  throw Error('Tool approval rescans historical decisions')
if(/try_all\("(?:execution|execution_operation)"\)/.test(startup))
  throw Error('Restart migration rescans all completed execution payloads')
if(!store.includes('pub fn algorithms_for_kind(')||
   !selectedAlgorithms.includes('state.store.algorithms_for_kind(kind)'))
  throw Error('Hot-path kernel selection must use indexed per-domain candidates')
if(!store.includes('pub fn legacy_improvements_to_migrate(')||
   !startup.includes('store.legacy_improvements_to_migrate()'))
  throw Error('Legacy improvement migration must not rewrite all records at every startup')
if(!store.includes('pub fn algorithm_usages_for_execution(')||
   !readFileSync(path.join(root,'src/addons/strategies/algorithms.rs'),'utf8')
     .includes('state.store.algorithm_usages_for_execution(execution_id)'))
  throw Error('Algorithm finalization must use indexed run-level usage records')

// Compare actual entry points and account for all extra model calls. A
// preflight/helper loop must not hide provider cost outside the run journal.
const helperSource=readFileSync(path.join(root,'src/engine/llm.rs'),'utf8')
const auxiliaryReceiptSource=readFileSync(path.join(root,'src/engine/llm/receipts.rs'),'utf8')
const finalizationSource=readFileSync(path.join(root,'src/engine/chat/runtime/finalization.rs'),'utf8')
for(const file of ['src/engine/chat/runtime/finalization.rs',
                   'src/addons/strategies/chat_adaptation.rs',
                   'src/addons/strategies/chat_program.rs']){
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
   !runtime.includes('dominion_core::runtime_port::AgentLoop::run(')||
   !sdkApi.includes('record["modelAccounting"]=run["modelAccounting"].clone()'))
  throw Error('SDK/chat execution or cost semantics diverged from common Rust core')

// An evidence-scoped repair directive must be consumed before another paid
// reviewer; unknown external effects cannot enter the automatic retry loop.
if(!finalizationSource.includes('pub(super) fn acceptance_repair(')||
   !adapter.includes('let repair = acceptance_repair(&progress, checks, can_retry);')||
   !adapter.includes('repair == AcceptanceRepair::Reconcile')||
   !adapter.includes('repair != AcceptanceRepair::Reviewer')||
   !adapter.includes('if can_retry {'))
  throw Error('Verifier-grounded repair escalation or uncertain-effect stop was bypassed')
// Delegated work must reuse the shared budget and journal. Historical
// child results are bounded in SQLite, never fetched from the entire archive.
const delegateSource=readFileSync(path.join(root,'src/addons/integrations/tool_delegation.rs'),'utf8')
const toolSource=readFileSync(path.join(root,'src/engine/tools.rs'),'utf8')
for(const addon of ['native_tools.rs','native_web.rs','native_github.rs',
                    'tool_delegation.rs','tool_github_repositories.rs','tool_github_replace.rs']){
  if(!existsSync(path.join(root,'src/addons/integrations',addon)))
    throw Error('Optional native tool module missing: '+addon)
}
if(toolSource.includes('async fn github_api(')||
   toolSource.includes('async fn web_search(')||
   !toolSource.includes('other => native_tools::execute(ctx,other,args).await'))
  throw Error('Remote connector implementation reintroduced into native tool executor')
const capabilitySource=readFileSync(path.join(root,'src/engine/capability.rs'),'utf8')
const dispatchSource=readFileSync(path.join(root,'src/engine/chat/runtime/tool_dispatch.rs'),'utf8')
const delegatedResult=delegateSource.match(/pub\(super\) fn task_payload\([\s\S]*?\n\}/)?.[0]
if(!delegatedResult||
   /\.all\("execution_operation"\)|\.session_execution_operations\(/.test(delegatedResult)||
   !delegatedResult.includes('recent_completed_session_operations(session_id, 20)')||
   !store.includes('idx_completed_operation_session_recent'))
  throw Error('Delegated result again loads the historical operation archive')
if(!toolSource.includes('pub(crate) use delegation::{profile_tool_allowed, profile_tool_visible}')||
   !delegateSource.includes('pub(crate) fn profile_tool_allowed(')||
   !runtime.includes('profile_tool_visible(&session,tool_name(tool))')||
   !capabilitySource.includes('profile_tool_allowed(&session,name,&arguments)')||
   !dispatchSource.includes('let role_admitted = tools::profile_tool_allowed(session, &call.name, &arguments)')||
   !dispatchSource.includes('extensions::tool_allowed(')||
   !dispatchSource.includes('if !role_admitted || effect_block.is_some()'))
  throw Error('Task-role tool admission can be bypassed by catalog/discovery/approval')

// Backend extension boundary: provider discovery, optional knowledge and
// channel drivers must not regain direct dependencies from the Rust chat loop.
const extensionHost=readFileSync(path.join(root,'src/addons/integrations/extensions/mod.rs'),'utf8')
const extensionTools=readFileSync(path.join(root,'src/addons/integrations/extensions/tools.rs'),'utf8')
const extensionKnowledge=readFileSync(path.join(root,'src/addons/integrations/extensions/knowledge.rs'),'utf8')
const extensionChannels=readFileSync(path.join(root,'src/addons/integrations/extensions/channels.rs'),'utf8')
const extensionJobs=readFileSync(path.join(root,'src/addons/integrations/extensions/jobs.rs'),'utf8')
const privateContext=readFileSync(path.join(root,'src/addons/knowledge/chat_context.rs'),'utf8')
const roomContext=readFileSync(path.join(root,'src/addons/integrations/chat_room_context.rs'),'utf8')
const apiRoutes=readFileSync(path.join(root,'src/api/mod.rs'),'utf8')
if(!addonIndex.includes('pub mod extensions;')||
   !extensionHost.includes('pub enum Extension')||
   !extensionHost.includes('pub fn enabled(')||
   !extensionHost.includes('pub fn tool_allowed(')||
   !extensionTools.includes('pub async fn external(')||
   !extensionTools.includes('pub async fn invoke_mcp(')||
   !extensionKnowledge.includes('pub fn memory_context(')||
   !extensionChannels.includes('pub fn start_inbound(')||
   !extensionJobs.includes('pub async fn run('))
  throw Error('Backend optional-extension host or its typed providers are missing')
if(/mcp::definitions\(|mcp::call\(/.test(runtime+adapter+dispatchSource)||
   !runtime.includes('extensions::tools::external(')||
   !adapter.includes('extensions::tools::catalog(')||
   !dispatchSource.includes('extensions::tools::invoke_mcp(')||
   !capabilitySource.includes('extensions::tools::catalog('))
  throw Error('Core chat/discovery regained a direct MCP server dependency')
if(!privateContext.includes('extensions::knowledge::memory_context(')||
   !roomContext.includes('extensions::knowledge::memories_for_room(')||
   !roomContext.includes('extensions::knowledge::visible_skills(')||
   !toolSource.includes('extensions::tool_allowed('))
  throw Error('Optional memory/skills bypass the extension gate')
if(!bootstrap.includes('extensions::channels::start_inbound(')||
   !jobs.includes('extensions::channels::start_outbound(')||
   !jobs.includes('extensions::jobs::run(')||
   !apiRoutes.includes('/api/extensions/runtime')||
   !apiRoutes.includes('/api/sessions/:id/extensions'))
  throw Error('Messenger or extension policy is no longer owned by the backend extension host')
