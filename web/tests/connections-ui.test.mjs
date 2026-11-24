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


test('SDK integration is visible inside existing plugin tab, not a fake installed registry',async()=>{
 const panel=readFileSync(resolve(dir,'../src/pages/extensions/SdkIntegrationPanel.tsx'),'utf8')
 assert.match(extensions,/tab === "plugins"[\s\S]*?<SdkIntegrationPanel \/>/)
 assert.match(extensions,/<RemoteWorkPanel \/>/)
 assert.match(panel,/const endpoint='\/api\/sdk\/runs'/)
 assert.match(panel,/useRemote<SdkRun\[\]>\(endpoint,\[\]\)/)
 assert.match(panel,/post\(endpoint\+'\/'\+encodeURIComponent\(current.id\)/)
 const {sdkSources,sdkControls,sdkVerified}=await import('../src/utils/sdkIntegration.ts')
 const groups=sdkSources([{id:'1',status:'running',source:{kind:'a:b',id:'c'}},
  {id:'2',status:'completed',source:{kind:'a',id:'b:c'}},{id:'3',status:'failed'}])
 assert.equal(groups.length,3)
 assert.equal(groups.find(g=>g.kind==='a:b').active,1)
 assert.equal(sdkVerified({id:'x',status:'completed',verified:true}),false)
 assert.equal(sdkVerified({id:'x',status:'completed',verified:true,taskOutcome:'succeeded'}),true)
 assert.equal(sdkControls({id:'x',status:'in_doubt',sessionId:'s'}).resume,false)
 assert.equal(sdkControls({id:'x',status:'in_doubt',sessionId:'s'}).reconcile,true)
 assert.equal(sdkControls({id:'x',status:'running',readOnly:true,sessionId:'s'}).cancel,false)
 assert.equal(sdkControls({id:'x',status:'needs_review',sessionId:'s',failureClass:'completion_evidence_unavailable'}).recheck,true)
})


test('registered plugin presence requires authenticated recent activity, not source names',async()=>{
 const {sdkPluginPresence}=await import('../src/utils/sdkPluginPresence.ts')
 const now=Date.parse('2026-11-24T00:00:00Z')
 assert.equal(sdkPluginPresence({status:'enabled'},now),'never')
 assert.equal(sdkPluginPresence({status:'enabled',lastSeenAt:'2026-11-23T23:59:30Z'},now),'recent')
 assert.equal(sdkPluginPresence({status:'enabled',lastSeenAt:'2026-11-23T23:57:00Z'},now),'stale')
 assert.equal(sdkPluginPresence({status:'disabled',lastSeenAt:'2026-11-23T23:59:30Z'},now),'disabled')
 const registry=readFileSync(resolve(dir,'../src/pages/extensions/SdkPluginRegistry.tsx'),'utf8')
 assert.match(registry,/const endpoint='\/api\/sdk\/plugins'/)
 assert.doesNotMatch(registry,/localStorage|sessionStorage/)
})


test('registered plugin filters cannot be spoofed by caller-supplied source labels',async()=>{
 const {sdkSourceKey}=await import('../src/utils/sdkIntegration.ts')
 assert.notEqual(sdkSourceKey({id:'1',status:'running',source:{kind:'sdk_plugin',id:'p'}}),
  sdkSourceKey({id:'2',status:'running',sdkPluginId:'p',source:{kind:'sdk_plugin',id:'p'}}))
})


test('registered namespace cannot collide with arbitrary source kind',async()=>{
 const {sdkSourceKey}=await import('../src/utils/sdkIntegration.ts')
 assert.notEqual(sdkSourceKey({id:'a',status:'running',source:{kind:'registered',id:'p'}}),sdkSourceKey({id:'b',status:'running',sdkPluginId:'p'}))
})


test('decision UI uses recorded evidence and native steering gates',async()=>{
 const {observedRate,sdkSteeringAllowed,steeringBytes}=await import('../src/utils/intelligenceView.ts')
 assert.equal(observedRate({uses:0,successes:0}),'—')
 assert.equal(observedRate({uses:4,successes:3}),'75% (3/4)')
 assert.equal(sdkSteeringAllowed({status:'running',sessionId:'s'}),true)
 assert.equal(sdkSteeringAllowed({status:'awaiting_approval',sessionId:'s'}),false)
 assert.equal(sdkSteeringAllowed({status:'running',sessionId:'s',steeringGate:{closed:true}}),false)
 assert.equal(sdkSteeringAllowed({status:'queued',sessionId:'s',steering:Array(8)}),false)
 assert.equal(steeringBytes('한'),3)
 const manager=readFileSync(resolve(dir,'../src/pages/memory/AlgorithmManager.tsx'),'utf8')
 assert.match(manager,/\/api\/algorithms\/evaluate/)
 const steering=readFileSync(resolve(dir,'../src/components/SdkSteering.tsx'),'utf8')
 assert.match(steering,/\+'\/steer'/)
})
