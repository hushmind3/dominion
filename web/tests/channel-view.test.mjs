import {test} from 'node:test'
import assert from 'node:assert/strict'
import {channelHealth,sortChannelBindings,matchingChannels,channelDraftMissing,normalizeChannelProbe} from '../src/utils/channelView.ts'

const entry={id:'slack',name:'Slack',category:'workplace',mode:'outbound',description:'Send room replies',docs:'https://slack.com',
 available:true,status:'available',features:['messages'],requiresRoom:true,fields:[
  {key:'token',label:'Access token',type:'secret',required:true},
  {key:'channelId',label:'Channel ID',type:'text',required:true}
 ]}
const binding=(props={})=>({id:'a',name:'Work',kind:'slack',agentId:'',providerId:'',model:'',
 enabled:true,mentionOnly:false,configured:true,status:'outbound_ready',...props})

test('channel lifecycle surfaces setup, delivery failures and paused links separately',()=>{
 assert.equal(channelHealth(binding({configured:false,missing:['token']})),'setup')
 assert.equal(channelHealth(binding({delivery:{needsReview:2}})),'issue')
 assert.equal(channelHealth(binding({status:'error',runtimeError:'Expired token'})),'issue')
 assert.equal(channelHealth(binding({status:'outbound_ready'})),'outbound')
 assert.equal(channelHealth(binding({status:'connected'})),'connected')
 assert.equal(channelHealth(binding({status:'sync_disabled'})),'paused')
 assert.equal(channelHealth(binding({enabled:false,status:'disabled'})),'disabled')
})
test('connected rows prioritize actionable failures without changing underlying records',()=>{
 const records=[binding({id:'healthy',name:'Healthy'}),binding({id:'broken',name:'Broken',configured:false,missing:['token']})]
 const ordered=sortChannelBindings(records)
 assert.deepEqual(ordered.map(row=>row.id),['broken','healthy'])
 assert.equal(records[0].id,'healthy')
})
test('catalog filtering matches features and category without inventing uninstalled drivers',()=>{
 assert.deepEqual(matchingChannels([entry],'messages','workplace').map(row=>row.id),['slack'])
 assert.deepEqual(matchingChannels([entry],'messages','messaging'),[])
 assert.deepEqual(matchingChannels([entry],'SLACK','all').map(row=>row.id),['slack'])
})
test('editing retains sealed secrets and requires destination and room fields',()=>{
 const saved={name:'Work',roomId:'room1',channelId:'C123',token:'',credentialsPresent:{token:true}}
 assert.deepEqual(channelDraftMissing(entry,saved),[])
 assert.deepEqual(channelDraftMissing(entry,{...saved,credentialsPresent:{token:false}}),['Access token'])
 assert.deepEqual(channelDraftMissing(entry,{...saved,roomId:''}),['roomId'])
 assert.deepEqual(channelDraftMissing(entry,{...saved,channelId:''}),['Channel ID'])
})

test('diagnosis results separate verified authentication from failed reachability and preserve timestamp',()=>{
 const at='2026-11-20T09:00:00.000Z'
 assert.deepEqual(normalizeChannelProbe({verified:true,message:'OK'},'Verified','Check needed',at),{
  status:'verified',message:'Verified',checkedAt:at
 })
 assert.deepEqual(normalizeChannelProbe({verified:false,message:'Token expired'},'Verified','Check needed',at),{
  status:'unverified',message:'Token expired',checkedAt:at
 })
 assert.equal(normalizeChannelProbe({},'Verified','Check needed',at).status,'unverified')
})
