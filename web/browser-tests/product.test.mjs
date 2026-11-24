import {test} from 'node:test'
import assert from 'node:assert/strict'
import {chromium} from 'playwright'
import {createServer} from 'node:http'
import {spawn} from 'node:child_process'
import {mkdtemp,rm} from 'node:fs/promises'
import {tmpdir} from 'node:os'
import {join,resolve} from 'node:path'
import {setTimeout as delay} from 'node:timers/promises'
import {fileURLToPath} from 'node:url'

const root=fileURLToPath(new URL('../../',import.meta.url))
async function listen(server){
 await new Promise((ok,fail)=>{server.once('error',fail);server.listen(0,'127.0.0.1',ok)})
 return server.address().port
}
async function until(fn,ms=15000){
 const deadline=Date.now()+ms
 let error
 while(Date.now()<deadline){
  try{const v=await fn();if(v)return v}catch(e){error=e}
  await delay(100)
 }
 throw Error('Condition timed out: '+(error?.message||'not ready'))
}

test('desktop app: actual model selection, persistent chat, settings and room navigation',{timeout:120000},async()=>{
 const directory=await mkdtemp(join(tmpdir(),'dominion-browser-'))
 const calls=[]
 const provider=createServer(async(req,res)=>{
  if(req.method==='GET'){
   res.setHeader('Content-Type','application/json')
   res.end(JSON.stringify({object:'list',data:[{id:'fixture-model',object:'model'},{id:'fixture-alt',object:'model'}]}))
   return
  }
  let body=''
  for await(const chunk of req)body+=chunk
  const parsed=JSON.parse(body)
  calls.push(parsed)
  if(parsed.stream){
   res.setHeader('Content-Type','text/event-stream')
   res.end('data: '+JSON.stringify({choices:[{delta:{content:'Browser integration reply'},finish_reason:'stop'}],usage:{prompt_tokens:10,completion_tokens:4,total_tokens:14}})+'\n\ndata: [DONE]\n\n')
  }else{
   res.setHeader('Content-Type','application/json')
   res.end(JSON.stringify({choices:[{message:{role:'assistant',content:'Browser integration reply'},finish_reason:'stop'}],usage:{prompt_tokens:10,completion_tokens:4,total_tokens:14}}))
  }
 })
 const modelPort=await listen(provider)
 const reservation=createServer()
 const port=await listen(reservation)
 await new Promise(ok=>reservation.close(ok))
 const base='http://127.0.0.1:'+port
 const child=spawn(resolve(root,'target/debug/dominion-rust'),[],{env:{...process.env,DOMINION_PORT:String(port),DOMINION_DATA:join(directory,'data'),DOMINION_WEB:resolve(root,'web/dist'),DOMINION_WORKSPACE:directory},stdio:['ignore','ignore','pipe']})
 let stderr=''
 child.stderr.on('data',b=>stderr+=String(b))
 let browser
 try{
  await until(async()=>{const r=await fetch(base+'/api/health');return r.ok},30000)
  const api=async(path,body)=>{
   const r=await fetch(base+path,body===undefined?{}:{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify(body)})
   assert.ok(r.ok,path+' '+r.status)
   return r.json()
  }
  const p=await api('/api/providers',{name:'Browser fixture',baseUrl:'http://127.0.0.1:'+modelPort+'/v1',kind:'openai',model:'fixture-model',enabled:true,toolMode:'native',local:true})
  const route=await api('/api/chat/inference-support?'+new URLSearchParams({providerId:p.id,model:'fixture-alt'}))
  assert.equal(route.model,'fixture-alt')
  browser=await chromium.launch({headless:true})
  const page=await browser.newPage({viewport:{width:1440,height:900},locale:'ko-KR'})
  const exceptions=[]
  page.on('pageerror',e=>exceptions.push(e.message))
  await page.goto(base+'/#/chat?provider='+encodeURIComponent(p.id)+'&model=fixture-alt')
  await page.locator('.page-chat .chat-ai-trigger').waitFor({state:'visible'})
  await until(async()=> (await page.locator('.page-chat .chat-ai-trigger').innerText()).includes('fixture-alt'))
  await page.locator('.page-chat textarea.conversation-composer-input').fill('Browser fixture question')
  const sent=page.waitForRequest(req=>req.method()==='POST'&&new URL(req.url()).pathname==='/api/chat')
  await page.locator('.page-chat button.conversation-composer-send').click()
  const wire=(await sent).postDataJSON()
  assert.equal(wire.providerId,p.id)
  assert.equal(wire.model,'fixture-alt')
  assert.equal(wire.directChat,true)
  assert.equal(wire.toolsEnabled,false)
  await page.locator('.page-chat [data-message-role="assistant"]').filter({hasText:'Browser integration reply'}).first().waitFor({state:'visible',timeout:30000})
  const session=await until(async()=>{
   const rows=await api('/api/sessions')
   return rows.find(row=>row.id===wire.sessionId)
  })
  const messages=await until(async()=>{
   const rows=await api('/api/sessions/'+encodeURIComponent(session.id)+'/messages')
   return rows.some(row=>row.role==='assistant'&&row.content.includes('Browser integration reply'))?rows:null
  })
  assert.ok(messages.some(row=>row.role==='user'&&row.content==='Browser fixture question'))
  assert.ok(calls.some(row=>row.model==='fixture-alt'))
  await page.reload()
  await page.locator('.page-chat [data-message-role="assistant"]').filter({hasText:'Browser integration reply'}).first().waitFor({state:'visible'})
  await page.locator('.page-chat .chat-history-search input').fill('___nonexistent_session___')
  assert.equal(await page.locator('.page-chat .chat-history-item').count(),0)
  await page.locator('.page-chat .chat-history-search input').fill('')
  await page.locator('.page-chat .chat-history-item').first().waitFor({state:'visible'})
  await page.keyboard.press('Control+,')
  await page.locator('.workspace-settings').waitFor({state:'visible'})
  await page.locator('.workspace-reply-preferences summary').click()
  await page.locator('.conversation-settings-inline').waitFor({state:'visible'})
  // Unknown generic route capabilities must not advertise a fabricated tier.
  assert.equal(await page.locator('.conversation-settings-inline select').count(),0)
  await page.locator('.workspace-back').click()
  await page.locator('.workspace-settings').waitFor({state:'detached'})
  await page.locator('.page-chat .conversation-switch button').last().click()
  await page.locator('.page-rooms').waitFor({state:'visible'})
  await page.locator('.page-rooms .conversation-switch button').first().click()
  await page.locator('.page-chat').waitFor({state:'visible'})
  await page.locator('.page-chat [data-message-role="assistant"]').filter({hasText:'Browser integration reply'}).first().waitFor({state:'visible'})
  assert.deepEqual(exceptions,[])
 }catch(error){
  throw Error(error.message+'\nBackend log tail:\n'+stderr.slice(-3000),{cause:error})
 }finally{
  await browser?.close()
  child.kill()
  await Promise.race([new Promise(ok=>child.once('exit',ok)),delay(2000)])
  provider.closeAllConnections()
  await new Promise(ok=>provider.close(ok))
  await rm(directory,{recursive:true,force:true})
 }
})
