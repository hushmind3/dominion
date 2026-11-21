import assert from 'node:assert/strict'
import {spawn} from 'node:child_process'
import {chromium} from 'playwright'
import path from 'node:path'
import {fileURLToPath} from 'node:url'

const root=fileURLToPath(new URL('../../',import.meta.url))
const port=Number(process.env.DOMINION_MEMORY_VISUAL_PORT||4389)
const url='http://127.0.0.1:'+port
const server=spawn(process.execPath,['node_modules/vite/bin/vite.js',
 '--host','127.0.0.1','--port',String(port),'--strictPort'],{
 cwd:path.join(root,'web'),stdio:['ignore','pipe','pipe']
})
let logs=''
server.stdout.on('data',chunk=>{logs+=chunk})
server.stderr.on('data',chunk=>{logs+=chunk})
let memories=[
 {id:'owner-memory',scope:'shared',status:'active',source:'user',
  text:'버스로 이동하는 것을 선호합니다.',retrievalEligible:true,importance:.8},
 {id:'learned-evidence',scope:'session',status:'active',source:'learn',
  text:'원본 채팅에서 배운 내용',retrievalEligible:true,
  sourceMessageIds:['message-1']}
]
const edits=[]
const json=(route,body,status=200)=>route.fulfill({
 status,contentType:'application/json',body:JSON.stringify(body)
})
async function mock(route){
 const request=route.request()
 const path=new URL(request.url()).pathname
 const method=request.method()
 if(path==='/api/events')return route.fulfill({status:200,contentType:'text/event-stream',body:':ready\n\n'})
 if(path==='/api/memories'&&method==='GET')return json(route,memories)
 if(path==='/api/agents')return json(route,[])
 if(path==='/api/status')return json(route,{jobs:0,agents:0,providers:0})
 if(path==='/api/memories/owner-memory/edit'&&method==='POST'){
  const body=JSON.parse(request.postData()||'{}')
  edits.push(body)
  if(body.expectedText!==memories[0].text)return json(route,{
   error:'기억이 이미 변경되었습니다. 새로고침 후 다시 확인하세요.'
  },409)
  memories[0]={
   ...memories[0],text:body.text,editedAt:new Date().toISOString(),
   editHistory:[...(memories[0].editHistory||[]),{
    text:body.expectedText,editedAt:new Date().toISOString(),by:'owner'
   }]
  }
  return json(route,memories[0])
 }
 return json(route,[])
}
async function ready(){
 for(let n=0;n<90;n++){
  if(server.exitCode!==null)throw Error('Vite stopped: '+logs)
  try{if((await fetch(url)).ok)return}catch{}
  await new Promise(resolve=>setTimeout(resolve,200))
 }
 throw Error('Vite not ready: '+logs)
}
let browser
try{
 await ready()
 browser=await chromium.launch({headless:true,args:['--no-sandbox']})
 const page=await browser.newPage({viewport:{width:390,height:844},locale:'ko-KR'})
 const pageErrors=[]
 page.on('pageerror',error=>pageErrors.push(error.message))
 await page.addInitScript(()=>localStorage.setItem('dominion.locale','ko'))
 await page.route('**/api/**',mock)
 await page.goto(url+'/#/memory',{waitUntil:'domcontentloaded'})
 await page.getByText('버스로 이동하는 것을 선호합니다.',{exact:true}).waitFor()
 assert.equal(await page.getByText('원본 채팅에서 배운 내용',{exact:true}).count(),1)
 assert.equal(await page.getByRole('button',{name:'기억 수정'}).count(),1,
  'Learned evidence cannot be rewritten as an owner-authored memory')
 await page.getByRole('button',{name:'기억 수정'}).click()
 const editor=page.getByRole('textbox',{name:'기억 수정'})
 await editor.fill('지하철로 이동하는 것을 선호합니다.')
 await page.getByRole('button',{name:'기억 저장'}).last().click()
 await page.getByText('지하철로 이동하는 것을 선호합니다.',{exact:true}).waitFor()
 assert.deepEqual(edits[0],{
  text:'지하철로 이동하는 것을 선호합니다.',
  expectedText:'버스로 이동하는 것을 선호합니다.'
 })
 await page.getByText('기억을 수정했습니다.',{exact:true}).waitFor()
 assert.equal(await page.getByText('원본 채팅에서 배운 내용',{exact:true}).count(),1)
 await page.getByRole('button',{name:'기억 수정'}).click()
 await editor.fill('클라이언트의 오래된 수정')
 memories[0].text='다른 요청의 최신 수정'
 await page.getByRole('button',{name:'기억 저장'}).last().click()
 await page.getByText('기억이 이미 변경되었습니다. 새로고침 후 다시 확인하세요.',{exact:true}).waitFor()
 assert.equal(await editor.inputValue(),'클라이언트의 오래된 수정',
  'A conflict must not discard the owner draft')
 assert.equal(memories[0].text,'다른 요청의 최신 수정')
 await page.getByRole('button',{name:'취소'}).click()
 await page.reload({waitUntil:'domcontentloaded'})
 await page.getByText('다른 요청의 최신 수정',{exact:true}).waitFor()
 assert.deepEqual(pageErrors,[],'Memory editor should not throw in React')
 assert.equal(edits.length,2)
 console.log('Sub3 memory owner edit + stale conflict + learned evidence isolation: PASS')
} finally {
 await browser?.close().catch(()=>{})
 server.kill('SIGTERM')
}
