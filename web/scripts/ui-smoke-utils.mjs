// Real Chromium smoke fixture for the React app, not a snapshot or no-op script.
// The network boundary is mocked; all DOM interactions use the shipped components.
import assert from 'node:assert/strict'
import {spawn} from 'node:child_process'
import {createServer} from 'node:net'
import {mkdir} from 'node:fs/promises'
import {fileURLToPath} from 'node:url'
import path from 'node:path'
import {chromium} from 'playwright'

const root=fileURLToPath(new URL('../',import.meta.url))
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms))
async function port() {
 const server=createServer()
 await new Promise(resolve=>server.listen(0,'127.0.0.1',resolve))
 const n=server.address().port
 await new Promise(resolve=>server.close(resolve))
 return n
}
async function start() {
 const n=await port(),url='http://127.0.0.1:'+n
 const child=spawn(process.execPath,['node_modules/vite/bin/vite.js','--host','127.0.0.1','--port',String(n),'--strictPort'],{cwd:root,stdio:['ignore','pipe','pipe']})
 let logs=''
 for(const stream of [child.stdout,child.stderr])stream.on('data',data=>logs+=data.toString().slice(0,3000))
 for(let i=0;i<140;i++){
  if(child.exitCode!==null)throw Error('Vite exited: '+logs)
  try{const res=await fetch(url);if(res.ok)return{child,url,logs:()=>logs}}catch{}
  await sleep(150)
 }
 child.kill()
 throw Error('Vite did not start: '+logs)
}
function fixture() {
 const db={
  mcp:[{id:'mock-mcp',name:'Mock Test MCP',transport:'http',url:'https://example.invalid/mcp',enabled:true}],
  providers:[{id:'mock-local',name:'Test Local',kind:'ollama',model:'qwen-test',baseUrl:'http://127.0.0.1:11434/v1',enabled:true,local:true,toolMode:'native',auth:'none'}],
  agents:[{id:'test-persona',name:'Test Persona',description:'Mock persona',systemPrompt:'Be clear.',providerId:'mock-local',model:'qwen-test',enabled:true}],
  sessions:[{id:'recent-chat',title:'Recent Test Chat',providerId:'mock-local',model:'qwen-test',createdAt:'2026-11-23T00:00:00Z'}],
  memories:[{id:'memory-one',text:'Remember test choice',scope:'shared',status:'active',source:'user',retrievalEligible:true,createdAt:'2026-11-23T00:00:00Z'}],
  chatRequests:[],mcpWrites:[],providerWrites:[],memoryWrites:[],jobs:[],plugins:[],pluginWrites:[],
 }
 const ok=(route,value,status=200,headers={})=>route.fulfill({status,contentType:'application/json',body:JSON.stringify(value),headers})
 const handler=async route=>{
  const request=route.request(),method=request.method(),url=new URL(request.url()),p=url.pathname
  let body={}
  if(method!=='GET')try{body=request.postDataJSON()||{}}catch{}
  if(method==='POST'&&p==='/api/chat'){
   db.chatRequests.push(body)
   return route.fulfill({status:200,headers:{'Content-Type':'text/event-stream','Cache-Control':'no-cache'},body:'data: {"type":"owner_saved","messageId":"owner-fixture"}\n\ndata: {"type":"done"}\n\n'})
  }
  if(p==='/api/mcp'&&method==='POST'){
   db.mcpWrites.push(body)
   const i=db.mcp.findIndex(row=>row.id===body.id)
   const saved={...body,id:body.id||'mock-mcp-'+db.mcp.length}
   if(i>=0)db.mcp[i]=saved;else db.mcp.push(saved)
   return ok(route,saved)
  }
  if(p==='/api/jobs'&&method==='POST'){
   db.jobs.push(body)
   return ok(route,{id:'mock-job-'+db.jobs.length,kind:body.kind,status:'queued',input:body.input},202)
  }
  if(p==='/api/providers'&&method==='POST'){
   db.providerWrites.push(body)
   const saved={...body,id:body.id||'mock-local'}
   const i=db.providers.findIndex(row=>row.id===saved.id)
   if(i>=0)db.providers[i]=saved;else db.providers.push(saved)
   return ok(route,saved)
  }
  if(p==='/api/sessions'&&method==='POST'){
   const saved={...body,id:'new-chat',title:body.title||'New Chat'}
   db.sessions.push(saved)
   return ok(route,saved)
  }
  if(p==='/api/memories'&&method==='POST'){
   const saved={...body,id:'new-memory',retrievalEligible:true}
   db.memories.push(saved);db.memoryWrites.push(body);return ok(route,saved)
  }
  if(p==='/api/memories/memory-one/correct'&&method==='POST'){
   db.memoryWrites.push(body);db.memories[0]={...db.memories[0],text:body.text};return ok(route,db.memories[0])
  }
  if(p==='/api/plugins/managed'&&method==='POST'){
   db.pluginWrites.push({method,body})
   const saved={...body,state:'created',running:false,ports:[],exitCode:0,imageId:'sha256:fixture',requestedPort:body.port}
   db.plugins=db.plugins.filter(row=>row.id!==body.id).concat(saved)
   return ok(route,saved)
  }
  const pluginMatch=p.match(/^\/api\/plugins\/managed\/([^/]+)(?:\/(start|stop|logs))?$/)
  if(pluginMatch){
   const plugin=db.plugins.find(row=>row.id===pluginMatch[1])
   if(!plugin)return ok(route,{error:'Missing plugin'},404)
   if(pluginMatch[2]==='logs')return ok(route,{stdout:'Voice fixture ready',stderr:'',truncated:false})
   db.pluginWrites.push({method,id:plugin.id,action:pluginMatch[2]})
   if(method==='DELETE'){Object.assign(plugin,{state:'retained',running:false});return ok(route,{removed:true,dataRetained:true,volume:'fixture-data'})}
   Object.assign(plugin,{state:pluginMatch[2]==='start'?'running':'exited',running:pluginMatch[2]==='start'})
   return ok(route,plugin)
  }
  if(method!=='GET')return ok(route,{ok:true})
  if(p==='/api/health'||p==='/api/status')return ok(route,{ok:true,status:'running',engine:'mock'})
  if(p==='/api/sessions')return ok(route,db.sessions)
  if(/^\/api\/sessions\/[^/]+\/messages$/.test(p))return ok(route,[])
  if(p==='/api/providers')return ok(route,db.providers)
  if(/^\/api\/providers\/[^/]+\/models$/.test(p))return ok(route,[{id:'qwen-test',name:'qwen-test',isDefault:true}])
  if(p==='/api/providers/catalog')return ok(route,{providers:[]})
  if(p==='/api/capabilities')return ok(route,{items:[{id:'web_search',name:'Web Search',description:'Search current web information',namespace:'builtin',available:true},{id:'file_read',name:'Read File',description:'Read workspace files',namespace:'builtin',available:true}]})
  if(p==='/api/agents')return ok(route,db.agents)
  if(p==='/api/memories')return ok(route,db.memories)
  if(p==='/api/mcp')return ok(route,db.mcp)
  if(p==='/api/mcp/discover')return ok(route,db.mcp.filter(row=>row.enabled!==false).map(row=>({mcpId:row.id,name:'mcp__mock__search',description:'Mock plugin search'})))
  if(p==='/api/mcp/presets')return ok(route,[])
  if(p==='/api/mcp/catalog')return ok(route,[{name:'Parallel MCP 2.0',description:'2026 web search and fetching',url:'https://mcp.parallel.ai/mcp'}])
  if(p==='/api/plugins/managed')return ok(route,{available:true,items:db.plugins})
  if(p==='/api/plugins/market')return ok(route,{items:[{repository:'example/fixture',description:'Fixture extension',stars:12,url:'https://github.com/example/fixture',detailsLoaded:false}],errors:[]})
  if(p==='/api/plugins/market/details')return ok(route,{repository:'example/fixture',description:'Fixture extension',stars:12,url:'https://github.com/example/fixture',detailsLoaded:true,sourceDownload:'https://github.com/example/fixture/archive/abc.zip',assets:[{name:'fixture.zip',url:'https://github.com/example/fixture/releases/download/v1/fixture.zip',size:1234}]})
  if(p==='/api/plugins/search')return ok(route,{items:[{id:'npm:voice-fixture',source:'npm',name:'voice-fixture',kind:'package',version:'1.0.0',description:'Voice fixture',license:'MIT',url:'https://www.npmjs.com/package/voice-fixture',compatibility:'unverified'}],errors:[],pypiMode:'exact_name',searchOnly:true})
  if(p==='/api/skill-catalog')return ok(route,{items:[],total:0,updatedAt:''})
  if(p==='/api/model-library/runtime')return ok(route,{status:'idle'})
  if(p==='/api/extensions/runtime')return ok(route,{enabled:{},configured:{}})
  if(p==='/api/policy')return ok(route,{id:'default',autoApprove:[],autoApproveAll:false,autoApproveMcp:false})
  if(p==='/api/jobs')return ok(route,db.jobs)
  if(/^\/api\/(sessions|agents|rooms|mcp|skills|jobs|channels|approvals|memories|executions|activities)(\/|$)/.test(p))return ok(route,[])
  return ok(route,{})
 }
 return {db,handler}
}
export async function withBrowser(name,callback,viewports=[{width:1280,height:800}]){
 const server=await start()
 let browser
 try {
  browser=await chromium.launch({headless:true,args:['--no-sandbox']})
  for(const viewport of viewports){
   const context=await browser.newContext({viewport})
   const {db,handler}=fixture()
   // Intercept only backend /api requests. A broad **/api/** glob also
   // intercepts Vite frontend modules under /src/api/, replacing JS with JSON.
   await context.route(url=>new URL(url).pathname.startsWith('/api/'),handler)
   const page=await context.newPage(),errors=[],networkErrors=[],consoleErrors=[]
   page.on('pageerror',error=>errors.push(error.message))
   page.on('console',message=>{if(message.type()==='error')consoleErrors.push(message.text())})
   page.on('requestfailed',request=>networkErrors.push(request.url()+': '+request.failure()?.errorText))
   page.on('response',response=>{if(response.status()>=400)networkErrors.push(response.status()+' '+response.url())})
   try{
    try{
     await callback({page,db,url:server.url,viewport})
     assert.deepEqual(errors,[],name+': unexpected React page errors')
     console.log('PASS '+name+' '+viewport.width+'px')
    }catch(error){
     console.error('FAIL '+name+' '+viewport.width+'px: '+String(error))
     console.error('URL: '+page.url())
     console.error('PAGE ERRORS: '+JSON.stringify(errors.slice(-10)))
     console.error('CONSOLE ERRORS: '+JSON.stringify(consoleErrors.slice(-12)))
     console.error('NETWORK FAILURES: '+JSON.stringify(networkErrors.slice(-12)))
     console.error('ROOT HTML: '+(await page.locator('#root').innerHTML().catch(()=>'NOT_FOUND')).slice(0,1200))
     console.error('DOCUMENT: '+(await page.content().catch(()=>'' )).slice(0,1100))
     console.error('BODY: '+(await page.locator('body').innerText().catch(()=>'' )).slice(0,2000))
     console.error('VITE: '+server.logs().slice(-1500))
     throw error
    }finally{
     const folder=path.join(root,'artifacts',name)
     await mkdir(folder,{recursive:true})
     await page.screenshot({path:path.join(folder,viewport.width+'.png'),animations:'disabled'}).catch(()=>{})
    }
   }finally{await context.close()}
  }
 }finally{if(browser)await browser.close();server.child.kill('SIGTERM')}
}
export {assert}
