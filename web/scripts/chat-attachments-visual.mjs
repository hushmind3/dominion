import assert from 'node:assert/strict'
import {spawn} from 'node:child_process'
import {chromium} from 'playwright'
import path from 'node:path'
import {fileURLToPath} from 'node:url'

const root=fileURLToPath(new URL('../../',import.meta.url))
const port=Number(process.env.DOMINION_ATTACHMENTS_VISUAL_PORT||4392)
const base='http://127.0.0.1:'+port
const server=spawn(process.execPath,['node_modules/vite/bin/vite.js',
 '--host','127.0.0.1','--port',String(port),'--strictPort'],{
 cwd:path.join(root,'web'),stdio:['ignore','pipe','pipe']
})
let logs=''
server.stdout.on('data',chunk=>{logs+=chunk})
server.stderr.on('data',chunk=>{logs+=chunk})
const session={id:'file-chat',title:'첨부 파일 검증',mode:'chat',agentId:null,
 providerId:'mock-provider',model:'mock-model'}
const uploadIds=[]
let sessionCreated=false
let messages=[]
let downloaded=0
let chatRequests=0
const sample=Buffer.from('A local owner note for the model')
const photo=Buffer.from('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+jRncAAAAASUVORK5CYII=','base64')
const reply=(route,obj,status=200)=>route.fulfill({
 status,contentType:'application/json',body:JSON.stringify(obj)
})
async function mock(route){
 const request=route.request(),pathname=new URL(request.url()).pathname,method=request.method()
 if(pathname==='/api/events')return route.fulfill({
  status:200,contentType:'text/event-stream',body:':ok\n\n'
 })
 if(pathname==='/api/providers')return reply(route,[{
  id:'mock-provider',name:'Test Model',enabled:true,kind:'openai',
  model:'mock-model',baseUrl:'http://127.0.0.1:9/v1'
 }])
 if(pathname==='/api/agents'||pathname==='/api/approvals')return reply(route,[])
 if(pathname==='/api/status')return reply(route,{agents:0,jobs:0,providers:1})
 if(pathname==='/api/model-library/runtime')return reply(route,{status:'idle'})
 if(pathname==='/api/model-library/installed')return reply(route,[])
 if(pathname==='/api/sessions'&&method==='GET')return reply(route,sessionCreated?[session]:[])
 if(pathname==='/api/sessions'&&method==='POST'){
  const body=JSON.parse(request.postData()||'{}')
  assert.equal(body.mode,'chat')
  sessionCreated=true
  return reply(route,session)
 }
 if(pathname==='/api/sessions/file-chat/messages')return reply(route,messages)
 if(pathname==='/api/sessions/file-chat/usage')return reply(route,{calls:1,totalTokens:18})
 if(pathname==='/api/attachments'&&method==='POST'){
  const body=JSON.parse(request.postData()||'{}')
  assert.equal(body.sessionId,'file-chat')
  assert.ok(['owner-note.txt','photo.png'].includes(body.name))
  assert.deepEqual(Buffer.from(body.data,'base64'),body.name==='photo.png'?photo:sample)
  assert.equal(body.mimeType,body.name==='photo.png'?'image/png':'text/plain')
  const id='attachment-'+(uploadIds.length+1)
  uploadIds.push(id)
  return reply(route,{
   id,name:body.name,mimeType:body.mimeType,
   isImage:body.name==='photo.png',sessionId:'file-chat',
   size:body.name==='photo.png'?photo.length:sample.length
  })
 }
 if(pathname==='/api/chat'&&method==='POST'){
  const body=JSON.parse(request.postData()||'{}')
  chatRequests++
  assert.equal(body.sessionId,'file-chat')
  assert.equal(body.mode,'chat')
  assert.equal(body.toolsEnabled,false)
  assert.equal(body.input,'첨부 파일을 확인해 주세요.')
  assert.deepEqual(body.attachmentIds,uploadIds)
  messages.push({id:'owner1',role:'user',session_id:'file-chat',content:body.input,
   metadata:{attachments:[
    {id:'attachment-1',name:'owner-note.txt',size:sample.length,isImage:false},
    {id:'attachment-2',name:'photo.png',size:photo.length,isImage:true}
   ]}})
  messages.push({id:'assistant1',role:'assistant',session_id:'file-chat',
   content:'첨부를 확인했습니다.'})
  const events=[
   {type:'text',text:'첨부를 확인했습니다.'},
   {type:'assistant_saved',messageId:'assistant1',sessionId:'file-chat'},
   {type:'done',sessionId:'file-chat'}
  ]
  return route.fulfill({
   status:200,contentType:'text/event-stream',
   body:events.map(event=>'data: '+JSON.stringify(event)+'\n\n').join('')
  })
 }
 if(pathname==='/api/attachments/attachment-2/content'&&method==='GET'){
  return route.fulfill({
   status:200,contentType:'image/png',body:photo
  })
 }
 if(pathname==='/api/attachments/attachment-1/content'&&method==='GET'){
  downloaded++
  return route.fulfill({
   status:200,contentType:'text/plain; charset=utf-8',body:sample
  })
 }
 return reply(route,[])
}
async function ready(){
 for(let n=0;n<90;n++){
  if(server.exitCode!==null)throw Error('Vite exited: '+logs)
  try{if((await fetch(base)).ok)return}catch{}
  await new Promise(resolve=>setTimeout(resolve,200))
 }
 throw Error('Vite not ready: '+logs)
}
let browser
try{
 await ready()
 browser=await chromium.launch({headless:true,args:['--no-sandbox']})
 const page=await browser.newPage({viewport:{width:390,height:844},locale:'ko-KR',acceptDownloads:true})
 const errors=[]
 page.on('pageerror',e=>errors.push(e.message))
 await page.addInitScript(()=>localStorage.setItem('dominion.locale','ko'))
 await page.route('**/api/**',mock)
 await page.goto(base+'/#/chat',{waitUntil:'domcontentloaded'})
 const sender=page.getByRole('button',{name:'보내기'})
 await page.locator('input[type="file"][aria-label="파일 첨부"]').setInputFiles([
  {name:'owner-note.txt',mimeType:'text/plain',buffer:sample},
  {name:'photo.png',mimeType:'image/png',buffer:photo}
 ])
 await page.getByText('owner-note.txt',{exact:true}).first().waitFor()
 await page.getByText('photo.png',{exact:true}).first().waitFor()
 assert.equal(await sender.isDisabled(),false,'Files alone must enable Send')
 await sender.click()
 await page.getByText('첨부를 확인했습니다.',{exact:true}).waitFor({timeout:15000})
 assert.equal(uploadIds.length,2,'Each file/image must be uploaded exactly once')
 assert.equal(chatRequests,1,'Only one model turn should be submitted')
 const owner=page.locator('[data-role="user"]')
 await owner.first().waitFor()
 assert.match(await owner.first().textContent(),/첨부 파일을 확인해 주세요/)
 const download=page.waitForEvent('download')
 await owner.first().getByRole('button',{name:'owner-note.txt'}).click()
 const item=await download
 assert.equal(item.suggestedFilename(),'owner-note.txt')
 assert.equal(downloaded,1)
 await owner.first().getByRole('button',{name:'photo.png'}).click()
 const preview=page.getByRole('dialog',{name:'이미지 미리보기'})
 await preview.waitFor({timeout:15000})
 assert.equal(await preview.locator('img').getAttribute('alt'),'photo.png')
 assert.ok(await preview.locator('img').evaluate(image=>image.complete&&image.naturalWidth>0))
 await preview.getByRole('button',{name:'닫기'}).click()
 await preview.waitFor({state:'detached'})
 await page.reload({waitUntil:'domcontentloaded'})
 await page.locator('[data-role="user"]').first().waitFor()
 assert.equal(await page.locator('[data-role="user"]').count(),1)
 assert.deepEqual(errors,[],'Attachment flow raised React errors')
 console.log('Sub3 new plain Chat upload + model attachmentIds + retained download: PASS')
}finally{
 await browser?.close().catch(()=>{})
 server.kill('SIGTERM')
}
