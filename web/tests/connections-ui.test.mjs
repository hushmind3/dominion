import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
import {resolve,dirname} from 'node:path'
import {
 channelsByKind,visibleChannelCatalog,channelDraftMissing,channelStatusTone,
} from '../src/utils/connectionState.ts'

const dir=dirname(fileURLToPath(import.meta.url))
const page=readFileSync(resolve(dir,'../src/pages/Connections.tsx'),'utf8')
const extensions=readFileSync(resolve(dir,'../src/pages/Extensions.tsx'),'utf8')
const capabilities=readFileSync(resolve(dir,'../src/pages/Capabilities.tsx'),'utf8')
const accounts=readFileSync(resolve(dir,'../src/components/ConnectedAccounts.tsx'),'utf8')
const modelHub=readFileSync(resolve(dir,'../src/features/models/useModelHub.ts'),'utf8')
const modelDialog=readFileSync(resolve(dir,'../src/features/models/ConnectionTools.tsx'),'utf8')
const models=readFileSync(resolve(dir,'../src/pages/Models.tsx'),'utf8')
const entry=(id,name=id,fields=[])=>({
 id,name,category:'community',mode:'webhook',description:'Channel',docs:'',available:true,
 status:'available',features:[],fields,requiresRoom:true,
})
const binding=(kind,id)=>({
 kind,id,name:id,agentId:'',providerId:'',model:'',enabled:true,
 mentionOnly:true,status:'connected',configured:true,credentialsPresent:{token:true},
})

test('connection navigation keeps the original seven tabs and no new workspace menu',()=>{
 const ids=[...page.matchAll(/\{id:'(channels|quick-connect|market|installed|skills|plugins|capabilities)'/g)].map(match=>match[1])
 assert.deepEqual(ids,['channels','quick-connect','market','installed','skills','plugins','capabilities'])
 assert.equal((page.match(/className="connection-tabs"/g)||[]).length,1)
 assert.match(page,/CapabilitiesPage embedded/)
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
