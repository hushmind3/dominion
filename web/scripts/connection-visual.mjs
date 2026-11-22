import assert from 'node:assert/strict'
import {spawn} from 'node:child_process'
import {mkdir,stat} from 'node:fs/promises'
import {fileURLToPath} from 'node:url'
import path from 'node:path'
import {chromium} from 'playwright'

// Browser-level visual acceptance against real Vite development rendering.
 // No production/release build is needed.
// Provider responses are fixtures: verification of layout and navigation,
// never a claim that external messaging credentials actually work.
const root=fileURLToPath(new URL('../../',import.meta.url))
const artifacts=path.join(root,'web','artifacts','connection-visual')
const port=Number(process.env.DOMINION_VISUAL_PORT||4382)
const server=spawn(process.execPath,['node_modules/vite/bin/vite.js','--host','127.0.0.1','--port',String(port),'--strictPort'],{
 cwd:path.join(root,'web'),stdio:['ignore','pipe','pipe']
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
let taskTeams=[]
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
 if(name==='/api/task-teams/bridges')return json(route,{bridges:{
  codex:{enabled:true,binaryAvailable:true,authentication:'unknown'},
  claude:{enabled:true,binaryAvailable:true,authentication:'unknown'},
  qwen:{enabled:true,binaryAvailable:true,authentication:'unknown'}}})
 if(name==='/api/task-teams'&&method==='GET')return json(route,taskTeams)
 if(name==='/api/task-teams'&&method==='POST'){
  const body=JSON.parse(route.request().postData()||'{}')
  const team={id:'visual-team',status:'queued',goal:body.goal,
   createdAt:new Date().toISOString(),teamState:{phase:'queued'},
   tasks:body.tasks.map(step=>({taskId:step.id,mode:step.mode,
    runtime:step.runtime,isolation:step.isolation,
    dependsOn:step.dependsOn,status:'queued',verified:false,
    childJobId:'team-child:visual-team:'+step.id}))}
  taskTeams=[team,...taskTeams]
  return json(route,team,202)
 }
 if(name==='/api/task-teams/visual-team'&&method==='GET')return json(route,taskTeams[0])
 if(name==='/api/channels/catalog')return json(route,fixtures)
 if(name==='/api/rooms')return json(route,[{id:'room-alpha',name:'테스트 연구실',agentIds:['a','b']}])
 if(name==='/api/agents')return json(route,[{id:'general',name:'일반 에이전트'}])
 if(name==='/api/providers')return json(route,[])
 if(name==='/api/oauth/providers')return json(route,[
  {id:'github',name:'GitHub',configured:true,connected:false,scope:'repo',callback:base+'/api/oauth/github/callback'},
  {id:'google',name:'Google',configured:false,connected:false,scope:'drive',callback:base+'/api/oauth/google/callback'}
 ])
 if(name==='/api/oauth/connections')return json(route,[])
 if(name==='/api/mcp'||name==='/api/mcp/presets'||name==='/api/mcp/discover'||name==='/api/skills')return json(route,[])
 if(name==='/api/mcp/catalog')return json(route,[
  {name:'Example filesystem tools',description:'Browse a local workspace through MCP',packageName:'@modelcontextprotocol/server-filesystem'},
  {name:'Example code search',description:'Search and inspect source repositories',packageName:'example-code-search'}
 ])
 if(name==='/api/skill-catalog')return json(route,{items:[],total:0,updatedAt:''})
 if(name==='/api/capabilities')return json(route,{items:[]})
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
  if(server.exitCode!==null)throw Error('Vite dev server exited: '+output)
  try{const response=await fetch(base);if(response.ok)return}
  catch{}
  await new Promise(resolve=>setTimeout(resolve,250))
 }
 throw Error('Vite dev server did not start: '+output)
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
 // Exercise the other existing connection tabs without adding new navigation.
 await page.setViewportSize({width:1440,height:900})
 for(const [label,title,file] of [
  ['계정 연결',null,'09-linked-accounts.png'],
  ['인증 앱 설정',null,'10-app-credentials.png'],
  ['MCP 마켓','MCP 마켓','11-mcp-market.png'],
  ['설치됨','설치된 MCP 서버','12-installed-mcp.png'],
  ['스킬','스킬','13-skills.png'],
  ['플러그인','플러그인','14-plugins.png'],
  ['역량 마켓','역량 마켓','15-capabilities.png']
 ]){
  await page.getByRole('button',{name:label,exact:true}).first().click()
  if(title)await page.getByRole('heading',{name:title,exact:true}).first().waitFor({timeout:15000})
  await page.waitForTimeout(200)
  await shot(page,file)
 }
 // Existing Activity navigation owns the Task Team composer, without new tabs.
 await page.goto(base+'/#/activity',{waitUntil:'domcontentloaded'})
 const teamPanel=page.locator('[aria-label="작업 팀"]')
 if(await teamPanel.getByRole('button',{name:'열기'}).count()){
  await teamPanel.getByRole('button',{name:'열기'}).click()
 }
 await teamPanel.getByRole('button',{name:'팀 만들기'}).click()
 await teamPanel.getByLabel('팀 목표').fill('Review project with local and Qwen agents')
 await teamPanel.getByLabel('작업 지시문').first().fill('Inspect source contracts')
 await teamPanel.getByRole('button',{name:'조사 Worker'}).click()
 await teamPanel.getByLabel('작업 지시문').nth(1).fill('Independently review the plan')
 await teamPanel.getByLabel('실행 엔진').nth(1).selectOption('qwen')
 // A multi-worker plan is owner input; verify refresh preserves this unsent DAG.
 await page.reload({waitUntil:'domcontentloaded'})
 await teamPanel.getByLabel('팀 목표').waitFor({timeout:15000})
 assert.equal(await teamPanel.getByLabel('팀 목표').inputValue(),'Review project with local and Qwen agents')
 assert.equal(await teamPanel.getByLabel('작업 지시문').nth(1).inputValue(),'Independently review the plan')
 assert.equal(await teamPanel.getByLabel('실행 엔진').nth(1).inputValue(),'qwen')
 assert.equal(await teamPanel.getByLabel('작업 의존성 미리보기').count(),1)
 await shot(page,'16-task-team-create-desktop.png')
 await teamPanel.getByRole('button',{name:'팀 실행'}).click()
 await teamPanel.getByText('팀이 실행 대기열에 등록됐습니다.').waitFor()
 assert.equal(taskTeams.length,1)
 assert.equal(taskTeams[0].tasks[0].runtime,'native')
 assert.equal(taskTeams[0].tasks[1].runtime,'qwen')
 assert.deepEqual(taskTeams[0].tasks[1].dependsOn,[],
  'Independent read workers should be parallel without accidental dependencies')
 await shot(page,'17-task-team-running-desktop.png')
 await page.setViewportSize({width:390,height:844})
 await shot(page,'18-task-team-mobile.png')
 await page.setViewportSize({width:1440,height:900})
 assert.deepEqual(errors,[],'No uncaught React page errors during visual flow')
 console.log(JSON.stringify({ok:true,items:screenshots},null,2))
}finally{
 await browser?.close()
 server.kill('SIGTERM')
}
