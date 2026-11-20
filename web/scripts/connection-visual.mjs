import assert from 'node:assert/strict'
import {spawn} from 'node:child_process'
import {mkdir,stat} from 'node:fs/promises'
import {fileURLToPath} from 'node:url'
import path from 'node:path'
import {chromium} from 'playwright'

// Browser-level visual acceptance against the actual built Vite bundle.
// Provider responses are fixtures: verification of layout and navigation,
// never a claim that external messaging credentials actually work.
const root=fileURLToPath(new URL('../../',import.meta.url))
const artifacts=path.join(root,'web','artifacts','connection-visual')
const port=Number(process.env.DOMINION_VISUAL_PORT||4382)
const server=spawn(process.execPath,['web/node_modules/vite/bin/vite.js','preview','--host','127.0.0.1','--port',String(port),'--strictPort'],{
 cwd:root,stdio:['ignore','pipe','pipe']
})
let output=''
server.stdout.on('data',data=>{output+=data.toString()})
server.stderr.on('data',data=>{output+=data.toString()})
const base='http://127.0.0.1:'+port
const fixtures=[
 {id:'discord',name:'Discord',category:'community',mode:'rest-poll',available:true,
  description:'서버·채널 메시지를 수신하고 도미니언 회의실로 전달합니다.',docs:'https://discord.com/developers/',
  requiresRoom:true,maxRoomParticipants:9,features:['message','replies'],
  fields:[{key:'token',label:'Bot Token',type:'secret',required:true},
   {key:'guildId',label:'서버 ID',type:'text',required:true},
   {key:'channelId',label:'채널·스레드 ID',type:'text',required:true}],
  instructions:['봇을 생성하고 메시지 권한을 부여합니다.']},
 {id:'telegram',name:'Telegram',category:'messaging',mode:'polling',available:true,
  description:'Bot API로 공용 그룹 대화에 참여합니다.',docs:'https://core.telegram.org/bots/api',
  requiresRoom:true,features:['dm','replies'],
  fields:[{key:'token',label:'Bot Token',type:'secret',required:true},
   {key:'chatId',label:'Chat ID',type:'text',required:true}]},
 {id:'slack',name:'Slack',category:'workplace',mode:'outbound',available:true,
  description:'도미니언 대화를 Slack 채널에 전송합니다.',docs:'https://slack.com',
  requiresRoom:true,features:['messages'],
  fields:[{key:'token',label:'Access Token',type:'secret',required:true},
   {key:'channelId',label:'Channel ID',type:'text',required:true}]}
]
let connections=[]
const json=(route,body,status=200)=>route.fulfill({
 status,contentType:'application/json; charset=utf-8',body:JSON.stringify(body)
})
const routeApi=async route=>{
 const url=new URL(route.request().url())
 const name=url.pathname
 const method=route.request().method()
 if(name==='/api/events')return route.fulfill({status:200,headers:{'content-type':'text/event-stream'},body:': fixture\n\n'})
 if(name==='/api/health')return json(route,{branch:'dominion-sub3',commit:'visual-smoke'})
 if(name==='/api/status')return json(route,{jobs:0})
 if(name==='/api/approvals')return json(route,[])
 if(name==='/api/channels/catalog')return json(route,fixtures)
 if(name==='/api/rooms')return json(route,[{id:'room-alpha',name:'테스트 연구실',agentIds:['a','b']}])
 if(name==='/api/agents')return json(route,[{id:'general',name:'일반 에이전트'}])
 if(name==='/api/providers')return json(route,[])
 if(name==='/api/channels'&&method==='GET')return json(route,connections)
 if(name==='/api/channels'&&method==='POST'){
  const record=JSON.parse(route.request().postData()||'{}')
  const saved={...record,id:'visual-connection',token:undefined,configured:true,
   credentialsPresent:{token:true},missing:[],status:record.enabled?'connected':'disabled',
   runtimeError:null,delivery:{pending:0,sending:0,sent:0,needsReview:0}}
  connections=[saved]
  return json(route,saved)
 }
 if(name.endsWith('/diagnose')&&method==='POST')return json(route,{verified:true,message:'Mock remote credential accepted'})
 if(name.endsWith('/delivery/reviews'))return json(route,{total:0,items:[]})
 return json(route,[])
}
async function waitReady(){
 for(let i=0;i<90;i++){
  if(server.exitCode!==null)throw Error('Vite preview exited: '+output)
  try{const response=await fetch(base);if(response.ok)return}
  catch{}
  await new Promise(resolve=>setTimeout(resolve,250))
 }
 throw Error('Preview did not start: '+output)
}
const screenshots=[]
const shot=async(page,file)=>{
 const target=path.join(artifacts,file)
 await page.screenshot({path:target,fullPage:true,animations:'disabled'})
 const meta=await stat(target)
 assert.ok(meta.size>12000,'Nontrivial screenshot required: '+file)
 screenshots.push({file,bytes:meta.size})
}
let browser
try{
 await mkdir(artifacts,{recursive:true})
 await waitReady()
 browser=await chromium.launch({headless:true,args:['--no-sandbox']})
 const page=await browser.newPage({viewport:{width:1440,height:900},deviceScaleFactor:1,locale:'ko-KR'})
 const errors=[]
 page.on('pageerror',error=>errors.push(error.message))
 await page.addInitScript(()=>localStorage.setItem('dominion.locale','ko'))
 await page.route('**/api/**',routeApi)
 await page.goto(base+'/#/connections',{waitUntil:'domcontentloaded'})
 await page.getByRole('heading',{name:'아직 연결한 채널이 없습니다'}).waitFor({timeout:15000})
 await shot(page,'01-empty-desktop.png')
 await page.getByRole('button',{name:'채널 연결'}).first().click()
 await page.getByRole('heading',{name:'어디에 연결할까요?'}).waitFor()
 await shot(page,'02-platform-catalog.png')
 await page.locator('section[aria-label="플랫폼 추가"] button').filter({hasText:'Discord'}).last().click()
 const editor=page.locator('[role="dialog"][aria-label*="Discord"]')
 await editor.waitFor()
 await shot(page,'03-credentials-step.png')
 await editor.locator('input[type="password"]').fill('visual-test-only-token')
 await editor.getByRole('button',{name:'다음'}).click()
 await editor.getByRole('heading',{name:'대화 대상 연결'}).waitFor()
 await editor.getByLabel('서버 ID *').fill('12345')
 await editor.getByLabel('채널·스레드 ID *').fill('67890')
 await editor.locator('select').first().selectOption('room-alpha')
 await shot(page,'04-destination-step.png')
 await editor.getByRole('button',{name:'다음'}).click()
 await editor.getByRole('heading',{name:'실행 전 확인'}).waitFor()
 await shot(page,'05-review-step.png')
 await editor.getByRole('button',{name:'저장'}).click()
 await page.getByText('연결 인증 확인').first().waitFor({timeout:12000})
 assert.equal(connections.length,1,'Save should materialize a real row')
 await shot(page,'06-connected-and-diagnosed.png')
 await page.setViewportSize({width:390,height:844})
 await page.reload({waitUntil:'domcontentloaded'})
 await page.getByText('Discord').first().waitFor({timeout:15000})
 await shot(page,'07-mobile-connections.png')
 await page.locator('section[aria-label="등록한 연결"] button').filter({hasText:'Discord'}).first().click()
 await page.getByRole('button',{name:'연결 목록으로'}).waitFor({timeout:15000})
 await shot(page,'08-mobile-details.png')
 assert.deepEqual(errors,[],'No uncaught React page errors during visual flow')
 console.log(JSON.stringify({ok:true,items:screenshots},null,2))
}finally{
 await browser?.close()
 server.kill('SIGTERM')
}
