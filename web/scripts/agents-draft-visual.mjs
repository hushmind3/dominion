import assert from 'node:assert/strict'
import {spawn} from 'node:child_process'
import {chromium} from 'playwright'
import path from 'node:path'
import {fileURLToPath} from 'node:url'

const root=fileURLToPath(new URL('../../',import.meta.url))
const port=Number(process.env.DOMINION_AGENT_DRAFT_VISUAL_PORT||4391)
const base='http://127.0.0.1:'+port
const server=spawn(process.execPath,['node_modules/vite/bin/vite.js',
 '--host','127.0.0.1','--port',String(port),'--strictPort'],{
 cwd:path.join(root,'web'),stdio:['ignore','pipe','pipe']
})
let logs=''
server.stdout.on('data',chunk=>{logs+=chunk})
server.stderr.on('data',chunk=>{logs+=chunk})
const reply=(route,data)=>route.fulfill({
 status:200,contentType:'application/json',body:JSON.stringify(data)
})
async function mock(route){
 const target=new URL(route.request().url()).pathname
 if(target==='/api/events')return route.fulfill({
  status:200,contentType:'text/event-stream',body:':ready\n\n'
 })
 if(target==='/api/providers'||target==='/api/agents')return reply(route,[])
 if(target==='/api/status')return reply(route,{agents:0,jobs:0,providers:0})
 if(target==='/api/model-library/runtime')return reply(route,{status:'idle'})
 return reply(route,[])
}
async function ready(){
 for(let attempt=0;attempt<90;attempt++){
  if(server.exitCode!==null)throw Error('Vite stopped: '+logs)
  try{if((await fetch(base)).ok)return}catch{}
  await new Promise(resolve=>setTimeout(resolve,200))
 }
 throw Error('Vite not ready: '+logs)
}
let browser
try{
 await ready()
 browser=await chromium.launch({headless:true,args:['--no-sandbox']})
 const page=await browser.newPage({viewport:{width:390,height:844},locale:'ko-KR'})
 const errors=[]
 page.on('pageerror',e=>errors.push(e.message))
 await page.addInitScript(()=>localStorage.setItem('dominion.locale','ko'))
 await page.route('**/api/**',mock)
 await page.goto(base+'/#/agents',{waitUntil:'domcontentloaded'})
 await page.getByRole('button',{name:'에이전트 만들기'}).first().click()
 const name=page.getByRole('textbox',{name:'에이전트 이름'})
 const prompt=page.getByRole('textbox',{name:'역할 지침'})
 await name.fill('저장 전 작성 중인 설계자')
 await prompt.fill('도구 실행 전 사용자 승인과 검증을 요구합니다.')
 await page.waitForFunction(()=>!!sessionStorage.getItem('dominion.agent.unsavedEditor'))
 await page.reload({waitUntil:'domcontentloaded'})
 await name.waitFor()
 assert.equal(await name.inputValue(),'저장 전 작성 중인 설계자')
 assert.equal(await prompt.inputValue(),'도구 실행 전 사용자 승인과 검증을 요구합니다.')
 await page.goto(base+'/#/models',{waitUntil:'domcontentloaded'})
 await page.goto(base+'/#/agents',{waitUntil:'domcontentloaded'})
 await name.waitFor()
 assert.equal(await prompt.inputValue(),'도구 실행 전 사용자 승인과 검증을 요구합니다.')
 await page.getByRole('button',{name:'취소'}).last().click()
 await page.waitForFunction(()=>sessionStorage.getItem('dominion.agent.unsavedEditor')===null)
 await page.reload({waitUntil:'domcontentloaded'})
 assert.equal(await prompt.count(),0,'Discarded agent draft must not reopen after refresh')
 assert.deepEqual(errors,[],'Agent editor threw React errors')
 console.log('Sub3 Agent unsaved persona + refresh + route restore + explicit discard: PASS')
}finally{
 await browser?.close().catch(()=>{})
 server.kill('SIGTERM')
}
