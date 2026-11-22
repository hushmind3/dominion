// Real Chromium + Vite consumer bridge test with deterministic API fixtures.
// This checks React rendering, the outgoing Chat wire contract and owner saved
// reconciliation. Native Rust model execution is covered by separate tests.
import assert from 'node:assert/strict'
import {spawn} from 'node:child_process'
import {dirname,resolve} from 'node:path'
import {fileURLToPath} from 'node:url'
import {chromium} from 'playwright'

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..')
const port=5197
const origin='http://127.0.0.1:'+port
const vite=spawn(process.execPath,[resolve(root,'node_modules/vite/bin/vite.js'),
 '--host','127.0.0.1','--port',String(port),'--strictPort'],{
 cwd:root,stdio:['ignore','pipe','pipe']
})
let log=''
for(const pipe of [vite.stdout,vite.stderr])pipe.on('data',data=>{log=(log+data.toString()).slice(-4000)})
let browser
const requests=[]
let sessionExists=false
const persisted=[]
async function ready(){
 for(let n=0;n<80;n++){
  if(vite.exitCode!==null)throw Error('Vite closed: '+log)
  try{if((await fetch(origin)).ok)return}catch{}
  await new Promise(resolve=>setTimeout(resolve,150))
 }
 throw Error('Vite did not respond: '+log)
}
try{
 await ready()
 browser=await chromium.launch({headless:true})
 const page=await browser.newPage({viewport:{width:1280,height:800},locale:'ko-KR'})
 const pageErrors=[]
 page.on('pageerror',err=>pageErrors.push(err.message))
 // Match owner HTTP routes only: glob **/api/** would also intercept
 // Vite's source modules at /src/api/*.ts and prevent React from booting.
 await page.route(url=>new URL(url).pathname.startsWith('/api/'),async route=>{
  const req=route.request()
  const path=new URL(req.url()).pathname
  if(path==='/api/events'){
   await route.fulfill({status:200,contentType:'text/event-stream',body:': fixture\n\n'})
   return
  }
  if(path==='/api/chat'){
   const body=req.postDataJSON()
   requests.push(body)
   assert.equal(body.sessionId,'smoke-chat')
   assert.match(body.requestId,/^[\da-f-]{36}$/i)
   assert.equal(body.conversationMode,'chat')
   assert.equal(body.directChat,true)
   assert.equal(body.toolsExecutionEnabled,false)
   const owner={id:'owner-'+requests.length,session_id:'smoke-chat',role:'user',
    content:body.input,created_at:'2026-11-22T00:00:00Z',metadata:{clientTurnId:body.clientTurnId}}
   const answer={id:'assistant-'+requests.length,session_id:'smoke-chat',role:'assistant',
    content:'응답 확인 '+requests.length,created_at:'2026-11-22T00:00:01Z',metadata:{}}
   persisted.push(owner,answer)
   const events=[
    {type:'owner_saved',messageId:owner.id,sessionId:'smoke-chat'},
    {type:'text',text:answer.content},
    {type:'assistant_saved',messageId:answer.id,sessionId:'smoke-chat'},
    {type:'done',sessionId:'smoke-chat'}
   ]
   await route.fulfill({status:200,contentType:'text/event-stream',
    body:events.map(e=>'data: '+JSON.stringify(e)+'\n\n').join('')})
   return
  }
  let data=[]
  if(path==='/api/health')data={ok:true,branch:'dominion-sub2'}
  else if(path==='/api/status')data={jobs:0,providers:1,agents:0,sessions:Number(sessionExists)}
  else if(path==='/api/sessions'){
   if(req.method()==='POST'){
    sessionExists=true
    data={id:'smoke-chat',title:'새 대화',providerId:'p1',model:'fixture-model',chatMode:'chat'}
   }else data=sessionExists?[{id:'smoke-chat',title:'새 대화',providerId:'p1',model:'fixture-model',chatMode:'chat'}]:[]
  }else if(path==='/api/sessions/smoke-chat/messages')data=persisted
  else if(path==='/api/sessions/smoke-chat/usage')data={totalTokens:12}
  else if(path==='/api/providers')data=[{id:'p1',name:'Mock provider',kind:'openai',
   model:'fixture-model',enabled:true,hasKey:true,baseUrl:'http://127.0.0.1:1'}]
  else if(path==='/api/providers/p1/models')data=[{id:'fixture-model',name:'Fixture model',context:8192}]
  else if(path==='/api/providers/catalog')data={providers:[]}
  else if(path==='/api/providers/free-catalog')data={status:'ready',providers:[],counts:{}}
  else if(path==='/api/agents'||path==='/api/approvals'||path==='/api/rooms')data=[]
  else if(path==='/api/model-library/runtime')data={status:'idle'}
  await route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(data)})
 })
 await page.goto(origin+'/#/chat',{waitUntil:'domcontentloaded'})
 const composer=page.locator('form.conversation-composer textarea')
 try{
  await composer.waitFor({state:'visible',timeout:15000})
 }catch(error){
  console.error('SUB2 SMOKE DIAGNOSTIC',{
   url:page.url(),pageErrors,
   body:(await page.locator('body').innerText().catch(()=>'')).slice(0,2500),
   html:(await page.locator('body').innerHTML().catch(()=>'')).slice(0,2500),
   vite:log.slice(-1500)
  })
  throw error
 }
 await composer.fill('웹 검색해서 확인해줘')
 await composer.press('Enter')
 await page.getByText('응답 확인 1').first().waitFor({timeout:12000})
 assert.equal(requests.length,1)
 assert.equal(requests[0].webSearch,true,'explicit public lookup must opt in')
 assert.equal(requests[0].clientTurnId,requests[0].requestId)
 await composer.fill('안녕하세요')
 await composer.press('Enter')
 await page.getByText('응답 확인 2').first().waitFor({timeout:12000})
 assert.equal(requests[1].webSearch,false,'ordinary conversation must not expose web tools')
 assert.deepEqual(pageErrors,[],'React page errors')
 console.log('PASS: React/Vite Chat bridge, SQLite acknowledgement and read-only search opt-in')
}finally{
 if(browser)await browser.close().catch(()=>{})
 vite.kill('SIGTERM')
}
