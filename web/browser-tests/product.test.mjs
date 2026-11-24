import {test} from 'node:test'
import assert from 'node:assert/strict'
import {chromium} from 'playwright'
import {createServer} from 'node:http'
import {spawn} from 'node:child_process'
import {mkdtemp,rm,stat} from 'node:fs/promises'
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
  if(parsed.stream&&JSON.stringify(parsed.messages||[]).includes('browser-sdk.md 파일을 만들어줘')){
   res.setHeader('Content-Type','text/event-stream')
   res.end('data: '+JSON.stringify({choices:[{delta:{tool_calls:[{index:0,id:'browser-sdk-write',type:'function',function:{name:'workspace_write',arguments:JSON.stringify({path:'browser-sdk.md',content:''})}}]},finish_reason:'tool_calls'}],usage:{prompt_tokens:40,completion_tokens:10,total_tokens:50}})+'\n\ndata: [DONE]\n\n')
   return
  }
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
  const supportLoaded=page.waitForResponse(r=>new URL(r.url()).pathname==='/api/chat/inference-support'&&new URL(r.url()).searchParams.get('model')==='fixture-alt'&&r.ok())
  await page.goto(base+'/#/chat?provider='+encodeURIComponent(p.id)+'&model=fixture-alt')
  await supportLoaded
  await page.waitForFunction(()=>document.querySelector('.page-chat .chat-ai-trigger')?.textContent?.includes('fixture-alt'))
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
  await page.locator('.page-chat .chat-history-item').first().waitFor({state:'visible'})
  await page.locator('.page-chat .chat-history-search input').fill('___nonexistent_session___')
  await page.locator('.page-chat .chat-history-item').first().waitFor({state:'detached'})
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
  await page.locator('.page-rooms .conversation-switch button').first().waitFor({state:'visible'})
  await page.locator('.page-rooms .conversation-switch button').first().click()
  await page.locator('.page-chat').waitFor({state:'visible'})
  await page.locator('.page-chat [data-message-role="assistant"]').filter({hasText:'Browser integration reply'}).first().waitFor({state:'visible'})
  // Native memory save and correction must reach the real database.
  await page.evaluate(()=>{location.hash='#/memory'})
  await page.getByLabel('저장할 내용',{exact:true}).fill('Browser persistent memory')
  await page.getByRole('button',{name:'기억 저장',exact:true}).click()
  await page.getByText('Browser persistent memory',{exact:true}).waitFor({state:'visible'})
  const original=await until(async()=> (await api('/api/memories')).find(row=>row.text==='Browser persistent memory'))
  await page.getByRole('button',{name:'기억 수정',exact:true}).first().click()
  await page.getByRole('textbox',{name:'기억 수정',exact:true}).fill('Browser corrected memory')
  await page.getByRole('button',{name:'수정 저장',exact:true}).click()
  await page.getByText('Browser corrected memory',{exact:true}).waitFor({state:'visible'})
  await until(async()=> (await api('/api/memories')).find(row=>row.text==='Browser corrected memory'&&row.id!==original.id&&row.retrievalEligible))
  // Room policy is stored by the room rather than private chat preferences.
  const agent=await api('/api/agents',{name:'Browser agent',providerId:p.id,model:'fixture-alt',systemPrompt:'Answer briefly.'})
  const room=await api('/api/rooms',{name:'Browser room',agentIds:[agent.id],models:[{providerId:p.id,model:'fixture-alt'}],toolsEnabled:false,autoLearning:false})
  await page.evaluate(()=>{location.hash='#/rooms'})
  await page.locator('.page-rooms .room-list-item').filter({hasText:'Browser room'}).click()
  await page.locator('.page-rooms .room-main').waitFor({state:'visible'})
  await page.keyboard.press('Control+,')
  await page.locator('.workspace-reply-preferences summary').click()
  await page.locator('.conversation-settings-inline').waitFor({state:'visible'})
  const tools=page.locator('.conversation-settings-inline label').filter({hasText:'도구 사용'}).locator('input[type=checkbox]')
  assert.equal(await tools.isChecked(),false)
  const optionSaved=page.waitForResponse(r=>r.request().method()==='POST'&&new URL(r.url()).pathname==='/api/rooms'&&r.request().postDataJSON().id===room.id)
  await tools.click()
  const savedResponse=await optionSaved
  assert.ok(savedResponse.ok(),await savedResponse.text())
  assert.equal((await savedResponse.json()).toolsEnabled,true)
  await until(async()=> (await api('/api/rooms/'+room.id)).toolsEnabled===true && await tools.isChecked() && await tools.isEnabled())
  await page.locator('.workspace-back').click()
  await page.keyboard.press('Control+,')
  await page.locator('.workspace-reply-preferences summary').click()
  await until(async()=>await tools.isChecked())
  // SDK UI must select an exact model and show the real approval boundary.
  await page.evaluate(()=>{location.hash='#/home'})
  await page.getByText('고급 작업 도구',{exact:true}).click()
  await page.locator('#sdk-goal').fill('browser-sdk.md 파일을 만들어줘')
  await page.locator('#sdk-provider').selectOption(p.id)
  await page.locator('#sdk-model').selectOption('fixture-alt')
  const sdkStart=page.waitForRequest(req=>req.method()==='POST'&&new URL(req.url()).pathname==='/api/sdk/runs')
  await page.getByRole('button',{name:'작업 시작',exact:true}).click()
  const sdkWire=(await sdkStart).postDataJSON()
  assert.equal(sdkWire.providerId,p.id)
  assert.equal(sdkWire.model,'fixture-alt')
  assert.ok(sdkWire.idempotencyKey)
  const approve=page.getByRole('button',{name:'이 작업 1회 승인',exact:true})
  await approve.waitFor({state:'visible',timeout:30000})
  await assert.rejects(stat(join(directory,'browser-sdk.md')),e=>e.code==='ENOENT')
  await approve.click()
  await page.getByText(/검증된 작업 결과/).waitFor({state:'visible',timeout:30000})
  assert.equal((await stat(join(directory,'browser-sdk.md'))).size,0)
  await until(async()=> (await api('/api/sdk/runs')).some(row=>row.model==='fixture-alt'&&row.status==='completed'&&row.verified===true))
  assert.ok(calls.some(row=>row.model==='fixture-alt'&&JSON.stringify(row.messages||[]).includes('browser-sdk.md 파일을 만들어줘')))
  // Existing Extensions screen now owns the external application lifecycle.
  await page.evaluate(()=>{location.hash='#/extensions'})
  await page.getByRole('button',{name:/설치됨/}).click()
  const extensions=page.getByRole('region',{name:'외부 SDK 앱'})
  await extensions.getByRole('button',{name:'앱 등록',exact:true}).click()
  await extensions.getByLabel('앱 ID',{exact:true}).fill('browser.assistant')
  await extensions.getByLabel('앱 이름',{exact:true}).fill('Browser external assistant')
  await extensions.getByRole('button',{name:'등록 저장',exact:true}).click()
  await extensions.getByText('Browser external assistant',{exact:true}).waitFor({state:'visible'})
  await extensions.getByRole('button',{name:'활성화',exact:true}).click()
  await until(async()=> (await api('/api/extensions/apps')).some(row=>row.id==='browser.assistant'&&row.enabled))
  await extensions.getByRole('button',{name:'앱별 인증키 관리',exact:true}).click()
  await extensions.getByLabel('허용 제공자',{exact:true}).selectOption(p.id)
  await extensions.getByLabel('허용 모델',{exact:true}).selectOption('fixture-alt')
  await extensions.getByLabel('허용 요청 횟수',{exact:true}).fill('1')
  await extensions.getByRole('button',{name:'키 발급',exact:true}).click()
  const key=extensions.getByLabel('일회 표시 인증키',{exact:true})
  await key.waitFor({state:'visible'})
  const token=await key.inputValue()
  assert.match(token,/^[a-f0-9]{64}$/)
  const grantPath='/api/extensions/apps/browser.assistant/grant'
  const grant=await api(grantPath)
  assert.equal(grant.remainingRuns,1)
  assert.ok(!JSON.stringify(grant).includes(token))
  const scoped=await fetch(base+'/api/extensions/apps/browser.assistant/runs',{
   method:'POST',headers:{'Content-Type':'application/json','x-dominion-extension-token':token},
   body:JSON.stringify({instruction:'Browser scoped reply',idempotencyKey:'browser-app-1'})
  })
  const scopedRun=await scoped.json()
  assert.ok(scoped.ok,JSON.stringify(scopedRun))
  await until(async()=>{
   const response=await fetch(base+'/api/extensions/apps/browser.assistant/runs/'+scopedRun.id,{headers:{'x-dominion-extension-token':token}})
   const row=await response.json()
   return row.status==='completed'
  },30000)
  await extensions.getByText(/남은 요청 0/).waitFor({state:'visible'})
  await extensions.getByRole('button',{name:'키 숨기기',exact:true}).click()
  assert.equal(await key.count(),0)
  page.once('dialog',dialog=>dialog.accept())
  await extensions.getByRole('button',{name:'키 폐기',exact:true}).click()
  await until(async()=>await api(grantPath)===null)
  await extensions.getByRole('button',{name:'비활성화',exact:true}).click()
  await until(async()=> (await api('/api/extensions/apps')).some(row=>row.id==='browser.assistant'&&!row.enabled))
  page.once('dialog',dialog=>dialog.accept())
  await extensions.getByRole('button',{name:'등록 해제',exact:true}).click()
  await until(async()=> (await api('/api/extensions/apps')).every(row=>row.id!=='browser.assistant'))
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
