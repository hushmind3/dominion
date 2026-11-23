import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
import {resolve,dirname} from 'node:path'
import {
 channelsByKind,visibleChannelCatalog,channelDraftMissing,channelStatusTone,
} from '../src/utils/connectionState.ts'
import {canonicalWorkspaceHash} from '../src/utils/workspaceRouting.ts'

const dir=dirname(fileURLToPath(import.meta.url))
const page=readFileSync(resolve(dir,'../src/pages/Connections.tsx'),'utf8')
const extensions=readFileSync(resolve(dir,'../src/pages/Extensions.tsx'),'utf8')
const nav=readFileSync(resolve(dir,'../src/components/WorkspaceNavigation.tsx'),'utf8')
const chatWelcome=readFileSync(resolve(dir,'../src/components/chat/ChatWelcome.tsx'),'utf8')
const roomWelcome=readFileSync(resolve(dir,'../src/components/rooms/RoomWelcome.tsx'),'utf8')
const roomPage=readFileSync(resolve(dir,'../src/pages/Rooms.tsx'),'utf8')
const composer=readFileSync(resolve(dir,'../src/components/chat/ConversationComposer.tsx'),'utf8')
const picker=readFileSync(resolve(dir,'../src/components/chat/ConversationTargetPicker.tsx'),'utf8')
const app=readFileSync(resolve(dir,'../src/App.tsx'),'utf8')
const settingsWorkspace=readFileSync(resolve(dir,'../src/components/SettingsWorkspace.tsx'),'utf8')
const ui=readFileSync(resolve(dir,'../src/components/ui/WorkspaceUI.tsx'),'utf8')
const capabilities=readFileSync(resolve(dir,'../src/pages/Capabilities.tsx'),'utf8')
const accounts=readFileSync(resolve(dir,'../src/components/ConnectedAccounts.tsx'),'utf8')
const modelHub=readFileSync(resolve(dir,'../src/features/models/useModelHub.ts'),'utf8')
const modelDialog=readFileSync(resolve(dir,'../src/features/models/ConnectionTools.tsx'),'utf8')
const models=readFileSync(resolve(dir,'../src/pages/Models.tsx'),'utf8')
const settings=readFileSync(resolve(dir,'../src/pages/SettingsHub.tsx'),'utf8')
const memory=readFileSync(resolve(dir,'../src/pages/Memory.tsx'),'utf8')
const prompts=readFileSync(resolve(dir,'../src/pages/Prompts.tsx'),'utf8')
const workspace=readFileSync(resolve(dir,'../src/workspacePages.tsx'),'utf8')
const routingHook=readFileSync(resolve(dir,'../src/hooks/useWorkspaceNavigation.ts'),'utf8')
const ko=readFileSync(resolve(dir,'../src/i18n/ko.ts'),'utf8')
const en=readFileSync(resolve(dir,'../src/i18n/en.ts'),'utf8')
const entry=(id,name=id,fields=[])=>({
 id,name,category:'community',mode:'webhook',description:'Channel',docs:'',available:true,
 status:'available',features:[],fields,requiresRoom:true,
})
const binding=(kind,id)=>({
 kind,id,name:id,agentId:'',providerId:'',model:'',enabled:true,
 mentionOnly:true,status:'connected',configured:true,credentialsPresent:{token:true},
})

test('MCP registry and MCP server UI does not mislabel MCPs as generic tools',()=>{
 assert.match(ko,/'ext\.registry': '공식 MCP 레지스트리'/)
 assert.match(ko,/'ext\.connectedServers': '등록된 MCP 서버'/)
 assert.match(ko,/'ext\.tab\.servers': 'MCP 서버'/)
 assert.match(ko,/'ext\.toolSearch': 'MCP 연결 확인'/)
 assert.match(ko,/'ext\.url': 'MCP 서버 주소'/)
 assert.match(ko,/'ext\.realTools': 'MCP에서 제공하는 기능'/)
 assert.match(en,/'ext\.registry': 'Official MCP Registry'/)
 assert.match(extensions,/MCP 레지스트리/)
 assert.match(extensions,/추천 MCP 서버 보기/)
 assert.match(extensions,/주요 MCP 연결/)
 assert.doesNotMatch(extensions,/추천 도구 보기|주요 도구 연결|<Blocks size=\{17\} \/> 도구/)
})

test('Service connections reuses the real MCP registry and installed editor, without a new navigation item',()=>{
 assert.match(page,/id:'mcp',label:tr\('MCP 연결','MCP connections'\)/)
 assert.match(page,/id:'registry',label:tr\('MCP 레지스트리','MCP registry'\)/)
 assert.match(page,/import\('\.\/Extensions'\)/)
 assert.match(page,/section=\{view==='mcp'\?'installed':view==='registry'\?'market':view\}/)
 assert.match(page,/onMcpRegistered=\{\(\)=>choose\('mcp'\)\}/)
 assert.match(extensions,/onMcpRegistered\?\.\(\)/)
 assert.match(extensions,/const mcpOnlyEmbed=embedded&&/)
 assert.match(extensions,/skills = useRemote<Skill\[]>\(mcpOnlyEmbed\?null/)
 assert.match(extensions,/\{!mcpOnlyEmbed&&<section className="mb-6 rounded-xl/)
 assert.match(extensions,/await mcp\.reload\(\);[\s\S]*?onMcpRegistered\?\.\(\)/)
 assert.match(extensions,/"\/api\/mcp\/catalog" \+ \(term/)
 assert.match(extensions,/"\/api\/mcp\/discover"/)
 assert.match(page,/id:'mcp',label:tr\('MCP 연결','MCP connections'\)/)
 assert.doesNotMatch(nav,/MCP 레지스트리|역량 마켓|id:'extensions'/)
})

test('Sub4-style conversation history and isolated settings use Tailwind without duplicate menus',()=>{
 assert.match(nav,/const history=useMemo/)
 assert.match(nav,/mode==='chat'\?history\.length/)
 assert.match(nav,/aria-label=\{tr\('대화 전환','Conversation switch'\)\}/)
 assert.match(nav,/aria-label=\{tr\('대화 목록','Conversation history'\)\}/)
 assert.match(nav,/bg-\[#F4F1EA\]/)
 assert.match(nav,/onClick=\{\(\)=>navigate\('settings'\)\}/)
 assert.doesNotMatch(nav,/id:'models' as const|id:'prompts' as const|id:'capabilities' as const|id:'extensions' as const/)
 assert.doesNotMatch(nav,/extra\.map\(renderItem\)|전체 기능|All features/)
 assert.match(app,/<SettingsWorkspace conversationSettingsRef=/)
 assert.match(app,/hidden=\{mode!=='chat'\}/)
 assert.match(app,/inert=\{workspacePage\|\|/)
 assert.match(settingsWorkspace,/<DialogBackdrop onDismiss=\{onClose\}>/)
 assert.match(settingsWorkspace,/aria-label=\{en\?'Management navigation':'관리 메뉴'\}/)
 assert.match(settingsWorkspace,/<WorkspacePage embedded page=\{page\}/)
 assert.doesNotMatch(settingsWorkspace,/import .*\.css/)
 assert.match(page,/id:'plugins',label:tr\('플러그인','Plugins'\)/)
 assert.match(page,/id:'skills',label:tr\('스킬','Skills'\)/)
 assert.match(page,/<ViewTabs value=\{view\}/)
 assert.match(ui,/export function ViewTabs/)
 assert.match(extensions,/\{tab === "plugins" &&/)
 assert.match(extensions,/\{tab === "skills" &&/)
})

test('memory owns personas and prompts; settings owns AIs and native capabilities',()=>{
 assert.match(memory,/id:'personas',label:tr\('AI·페르소나','AIs & personas'\)/)
 assert.match(memory,/id:'shared',label:tr\('공통·시스템 지침','Shared & system instructions'\)/)
 assert.match(memory,/<PromptsPage section=\{view==='memories'\?'shared':view\} embedded\/>/)
 assert.match(prompts,/<SystemPromptPanel\/>/)
 assert.match(prompts,/<AgentsPage embedded/)
 assert.match(settings,/id:'ai',label:tr\('AI 연결','AI connections'\)/)
 assert.match(settings,/id:'features',label:tr\('기본 기능','Built-in capabilities'\)/)
 assert.match(settings,/<CapabilitiesPage embedded navigate=\{\(\)=>\{\}\}\/>/)
 assert.match(workspace,/render: \(navigate: Navigate\) => <SettingsHub navigate=\{navigate\} \/>/)
 assert.match(routingHook,/canonicalWorkspaceHash\(window\.location\.hash\)/)
 assert.match(capabilities,/embedded\?\[\]:catalog\.value/)
})

test('chat and group frontends expose genuine Sub4-like onboarding and contextual actions',()=>{
 assert.match(chatWelcome,/t\('welcome\.title'\)/)
 assert.match(chatWelcome,/onPrompt\(t\(prompt\)\)/)
 assert.match(chatWelcome,/onConnect/)
 assert.match(chatWelcome,/sm:grid-cols-3/)
 assert.match(roomWelcome,/rooms\.welcome\.title/)
 assert.match(roomWelcome,/#\/memory\?view=personas/)
 assert.match(roomPage,/header className="flex min-w-0 shrink-0 flex-wrap items-center/)
 assert.match(roomPage,/current\.name/)
 assert.match(roomPage,/setTeamOpen\(true\);setInspector\(false\)/)
 assert.match(composer,/conversation-composer group min-w-0/)
 assert.match(composer,/border-t border-\[#F0E9DD\]/)
 assert.match(picker,/role="group" aria-label=\{tr\('연결 서비스 선택'/)
 assert.match(picker,/providers\.map\(item=><Button/)
})

test('old navigation links resolve to the new location without losing prompt targets',()=>{
 const expected=[
  ['#/models','#/settings?view=ai'],
  ['#/capabilities','#/settings?view=features'],
  ['#/extensions','#/connections?view=mcp'],
  ['#/extensions?tab=market','#/connections?view=registry'],
  ['#/extensions?tab=plugins','#/connections?view=plugins'],
  ['#/extensions?tab=skills','#/connections?view=skills'],
  ['#/home','#/settings?view=overview'],
  ['#/activity','#/settings?view=activity'],
  ['#/prompts','#/memory?view=shared'],
  ['#/prompts?view=groups&room=g1','#/memory?view=groups&room=g1'],
  ['#/agents?agent=a1','#/memory?agent=a1&view=personas'],
 ]
 for(const [before,after] of expected)assert.equal(canonicalWorkspaceHash(before),after,before)
 assert.equal(canonicalWorkspaceHash('#/chat?session=s1'),'#/chat?session=s1')
 assert.equal(canonicalWorkspaceHash('#/connections?view=mcp'),'#/connections?view=mcp')
 assert.equal(canonicalWorkspaceHash('#/memory?view=personas'),'#/memory?view=personas')
})

test('filters deduplicate catalog and prioritize saved connections',()=>{
 const bound=channelsByKind([binding('telegram','b1')])
 const list=[entry('discord','Discord'),entry('telegram','Telegram'),entry('telegram','Telegram Duplicate')]
 assert.deepEqual(visibleChannelCatalog(list,bound,'','all',false).map(x=>x.id),['telegram','discord'])
 assert.deepEqual(visibleChannelCatalog(list,bound,'','all',true).map(x=>x.id),['telegram'])
 assert.deepEqual(visibleChannelCatalog(list,bound,'b1','all',false).map(x=>x.id),['telegram'])
 assert.deepEqual(visibleChannelCatalog(list,bound,'discord','workplace',false).map(x=>x.id),[])
})

test('channel draft keeps stored secret but rejects blank required credentials',()=>{
 const item=entry('telegram','Telegram',[
  {key:'token',label:'Token',type:'secret',required:true},
  {key:'chatId',label:'Chat',type:'text',required:true},
 ])
 assert.deepEqual(channelDraftMissing(item,{name:'Account',enabled:true,chatId:'room',roomId:'r1',credentialsPresent:{token:true}}),[])
 assert.deepEqual(channelDraftMissing(item,{name:'Account',enabled:true,chatId:'',roomId:'r1'}),['token','chatId'])
 assert.deepEqual(channelDraftMissing(item,{name:'Account',enabled:true,chatId:'target',token:'123'}),['roomId'])
 assert.deepEqual(channelDraftMissing(item,{name:'Account',enabled:false,chatId:'target',token:'123'}),[])
 assert.deepEqual(channelDraftMissing(item,{name:'  ',enabled:false,chatId:'target',token:'123'}),['name'])
})

test('health indicators never call disabled or broken channels connected',()=>{
 const base=binding('discord','d1')
 assert.equal(channelStatusTone(base),'online')
 assert.equal(channelStatusTone({...base,enabled:false}),'disabled')
 assert.equal(channelStatusTone({...base,status:'error'}),'error')
 assert.equal(channelStatusTone({...base,runtimeError:'auth rejected'}),'error')
 assert.equal(channelStatusTone({...base,configured:false}),'error')
 assert.equal(channelStatusTone({...base,status:'starting'}),'pending')
})

test('MCP discovery, native tools and plugin catalogs remain separated',()=>{
 assert.doesNotMatch(extensions,/pluginRemote|setPluginRemote/)
 assert.match(extensions,/if \(tab === "installed"\) void discover\(true\)/)
 assert.match(extensions,/if \(tab === "market"\) void loadCatalog\(\)/)
 assert.match(capabilities,/embedded\?\[\]:catalog\.value/)
 assert.match(extensions,/\{!embedded && <Headline/)
})

test('opening GitHub device URL does not submit CLI confirmation prematurely',()=>{
 const link=accounts.match(/<a href=\{githubCli.url[\s\S]*?<\/a>/)?.[0]
 assert.ok(link)
 assert.doesNotMatch(link,/\/api\/providers\/cli\/github\/login\/input/)
 assert.match(accounts,/navigator\.clipboard\.writeText/)
})

test('each installed MCP plugin is independently switchable without removing it',()=>{
 assert.match(extensions,/checked=\{x.enabled !== false\}/)
 assert.match(extensions,/onChange=\{event => void toggleMcp\(x, event.target.checked\)\}/)
 assert.match(extensions,/await post\('\/api\/mcp', \{\.\.\.record, enabled\}\)/)
 assert.match(extensions,/await mcp\.reload\(\)/)
 assert.match(extensions,/setTools\(previous => previous\.filter\(tool => tool\.mcpId !== record\.id\)\)/)
 assert.match(extensions,/if \(enabled && tab === 'installed'\) void discover\(true\)/)
 assert.match(extensions,/if \(revision !== discoveryVersion\.current\) return/)
 assert.match(extensions,/useState<ExtensionTab>\("installed"\)/)
})

test('local model connections expose the saved tool adapter rather than hiding compat mode',()=>{
 assert.match(modelHub,/toolMode:kind==='codex'\?'compat':toolMode/)
 assert.match(modelHub,/setToolMode\(preset\.local\|\|preset\.kind==='ollama'\?'native':'auto'\)/)
 assert.match(modelDialog,/value=\{toolMode\}/)
 assert.match(modelDialog,/<option value="native">/)
 assert.match(modelDialog,/<option value="compat">/)
 assert.match(models,/provider\.toolMode==='compat'/)
})
