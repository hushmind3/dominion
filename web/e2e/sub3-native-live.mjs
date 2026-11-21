// Optional end-to-end DOMINION Sub3 smoke: real Rust HTTP + mock paid model
// + Chromium + a temporary workspace + durable native tool receipts.
// Prerequisites: cargo build --locked; npm ci --prefix web;
// npm run build --prefix web; npx playwright install chromium.
// Run: npm run test:sub3-browser --prefix web
import assert from 'node:assert/strict'
import {createServer} from 'node:http'
import {createServer as createTcpServer} from 'node:net'
import {spawn} from 'node:child_process'
import {existsSync, mkdtempSync, mkdirSync, readFileSync, rmSync} from 'node:fs'
import {tmpdir} from 'node:os'
import path from 'node:path'
import {fileURLToPath} from 'node:url'
import {chromium} from 'playwright'

const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'../..')
const webDir=path.join(root,'web/dist')
const bin=process.env.DOMINION_BIN || path.join(root,'target/debug/dominion-rust')
assert.ok(existsSync(bin),`Build the Rust binary first: ${bin}`)
assert.ok(existsSync(path.join(webDir,'index.html')),'Build web/dist first')
const temp=mkdtempSync(path.join(tmpdir(),'dominion-sub3-browser-'))
const workspace=path.join(temp,'workspace')
mkdirSync(workspace,{recursive:true})
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms))
const listen=server=>new Promise((resolve,reject)=>{
  server.once('error',reject)
  server.listen(0,'127.0.0.1',()=>resolve(server.address().port))
})
const freePort=async()=>{
  const server=createTcpServer()
  const port=await listen(server)
  await new Promise(resolve=>server.close(resolve))
  return port
}
const json=async(response)=>{
  // Response bodies are single-use: read text only on HTTP failure.
  if(!response.ok)throw Error(`HTTP ${response.status}: ${await response.text()}`)
  return response.json()
}
const httpJson=async(url,options={})=>json(await fetch(url,options))
async function poll(label,fn,timeout=35000){
  const end=Date.now()+timeout
  let last
  while(Date.now()<end){
    try {
      last=await fn()
      if(last?.done)return last.value
    } catch(e){last=e}
    await sleep(250)
  }
  throw Error(`${label} did not reach expected state: ${JSON.stringify(last)}`)
}
let child
let browser
const requests=[]
const mock=createServer(async(req,res)=>{
  if(req.url!=='/v1/chat/completions'||req.method!=='POST'){
    res.writeHead(404).end();return
  }
  let raw=''
  for await (const chunk of req)raw+=chunk
  let body
  try{body=JSON.parse(raw)}catch{res.writeHead(400).end();return}
  requests.push(body)
  const history=body.messages||[]
  const historyText=JSON.stringify(history)
  const isWriter=historyText.includes('sub3-smoke.txt')||
    historyText.includes('sub3-deny.txt')
  const targetFile=historyText.includes('sub3-deny.txt')?
    'sub3-deny.txt':'sub3-smoke.txt'
  const hasToolReceipt=history.some(msg=>msg.role==='tool')
  let result
  if(isWriter&&!hasToolReceipt){
    result={choices:[{index:0,finish_reason:'tool_calls',message:{
      role:'assistant',content:'',tool_calls:[{id:'sub3-workspace-once',
        type:'function',function:{name:'workspace_write',
          arguments:JSON.stringify({path:targetFile,content:'SUB3_VERIFIED'})}}]
    }}]}
  } else {
    result={choices:[{index:0,finish_reason:'stop',message:{
      role:'assistant',content:isWriter?'파일 기록 후 결과를 확인했습니다.':'설명을 완료했습니다.'
    }}]}
  }
  const usage={choices:[],usage:{prompt_tokens:21,completion_tokens:9,total_tokens:30}}
  res.writeHead(200,{'content-type':'text/event-stream; charset=utf-8',
    'cache-control':'no-cache'})
  // Settled final message, usage-only packet and explicit terminal marker.
  res.end(`data: ${JSON.stringify(result)}\n\ndata: ${JSON.stringify(usage)}\n\ndata: [DONE]\n\n`)
})
try {
  const mockPort=await listen(mock)
  const port=await freePort()
  const base=`http://127.0.0.1:${port}`
  child=spawn(bin,[],{cwd:root,env:{...process.env,
    DOMINION_PORT:String(port),DOMINION_DATA:path.join(temp,'runtime'),
    DOMINION_WEB:webDir,DOMINION_WORKSPACE:workspace},
    stdio:['ignore','ignore','pipe']})
  let stderr=''
  child.stderr.on('data',data=>{stderr=(stderr+data.toString()).slice(-16000)})
  await poll('Rust health',async()=>{
    if(child.exitCode!==null)throw Error(stderr||'Rust process exited')
    const response=await fetch(`${base}/api/health`).catch(()=>null)
    return {done:response?.ok,value:response?.status}
  })
  const provider=await httpJson(`${base}/api/providers`,{
    method:'POST',headers:{'content-type':'application/json'},
    body:JSON.stringify({name:'sub3-browser-mock',kind:'openai',
      baseUrl:`http://127.0.0.1:${mockPort}/v1`,
      apiKey:'test',model:'mock-native',enabled:true,toolMode:'native',local:true})
  })
  assert.ok(provider.id,'Provider was not persisted')
  browser=await chromium.launch({headless:true})
  const page=await browser.newPage({viewport:{width:390,height:780}})
  await page.goto(base,{waitUntil:'domcontentloaded'})
  await page.locator('#sdk-goal').fill('한 문장으로 일반 개념을 설명해 주세요.')
  await page.getByRole('button',{name:/작업 시작|Start task/}).click()
  const first=await poll('Native text answer',async()=>{
    const jobs=await httpJson(`${base}/api/sdk/runs`)
    const run=jobs.find(job=>job.instruction.includes('한 문장으로'))
    if(run&&['failed','needs_review','denied'].includes(run.status))
      throw Error(`Native text run failed: ${JSON.stringify(run)}`)
    return {done:run?.status==='completed',value:run}
  })
  assert.equal(first.phase,'succeeded')
  const durable=await httpJson(`${base}/api/jobs/${first.id}`)
  assert.ok((durable.nativeState?.decisionTape||[]).length>0,
    'The native model decision must survive in the SQLite job checkpoint')
  await page.reload({waitUntil:'domcontentloaded'})
  await page.locator('#sdk-history').waitFor()
  assert.ok(await page.locator('#sdk-history').inputValue(),
    'Refresh must restore the selected native task')
  // A second run exercises the real approval gate and filesystem receipt.
  await page.locator('#sdk-goal').fill('sub3-smoke.txt 파일에 SUB3_VERIFIED를 작성해 주세요.')
  await page.getByRole('button',{name:/작업 시작|Start task/}).click()
  const writer=await poll('Writer approval',async()=>{
    const jobs=await httpJson(`${base}/api/sdk/runs`)
    const run=jobs.find(job=>job.instruction.includes('sub3-smoke.txt'))
    if(run&&['failed','denied','needs_review'].includes(run.status))
      throw Error(`Native writer failed: ${JSON.stringify(run)}`)
    return {done:run?.status==='awaiting_approval',value:run}
  })
  assert.equal(writer.checkpoint?.pendingCall?.name,'workspace_write')
  assert.ok(!existsSync(path.join(workspace,'sub3-smoke.txt')),
    'Write was executed before approval')
  await page.reload({waitUntil:'domcontentloaded'})
  await page.getByRole('button',
    {name:/이 작업 1회 승인|Approve this action only/}).waitFor()
  const stillPending=await httpJson(`${base}/api/sdk/runs/${writer.id}`)
  assert.equal(stillPending.checkpoint.pendingCall.id,
    writer.checkpoint.pendingCall.id,'Approval target changed across refresh')
  await page.getByRole('button',{name:/이 작업 1회 승인|Approve this action only/}).click()
  const finished=await poll('Writer completion',async()=>{
    const run=await httpJson(`${base}/api/sdk/runs/${writer.id}`)
    if(['failed','denied','needs_review'].includes(run.status))
      throw Error(`Writer did not settle: ${JSON.stringify(run)}`)
    return {done:run.status==='completed',value:run}
  })
  assert.equal(readFileSync(path.join(workspace,'sub3-smoke.txt'),'utf8'),'SUB3_VERIFIED')
  const job=await httpJson(`${base}/api/jobs/${finished.id}`)
  const receipts=job.nativeState?.receipts||[]
  assert.ok(receipts.some(receipt=>receipt.tool==='workspace_write'&&receipt.success),
    'A real durable successful tool receipt is missing')
  assert.ok(requests.length>=3,'The model must observe a real tool receipt before finalizing')
  await page.reload({waitUntil:'domcontentloaded'})
  const afterRefresh=await httpJson(`${base}/api/sdk/runs/${writer.id}`)
  assert.equal(afterRefresh.phase,'succeeded')
  // A denied exact approval cannot be re-used to authorize the rejected write.
  await page.locator('#sdk-goal').fill('sub3-deny.txt 파일에 SUB3_VERIFIED를 작성해 주세요.')
  await page.getByRole('button',{name:/작업 시작|Start task/}).click()
  const deniedTarget=await poll('Writer denial target',async()=>{
    const jobs=await httpJson(`${base}/api/sdk/runs`)
    const run=jobs.find(job=>job.instruction.includes('sub3-deny.txt'))
    return {done:run?.status==='awaiting_approval',value:run}
  })
  const rejectedCall=deniedTarget.checkpoint.pendingCall.id
  await page.getByRole('button',{name:/이 도구 실행 거부|Refuse this action/}).click()
  const stale=await fetch(`${base}/api/sdk/runs/${deniedTarget.id}/approve`,{
    method:'POST',headers:{'content-type':'application/json'},
    body:JSON.stringify({callId:rejectedCall})
  })
  assert.equal(stale.status,409,'A refused approval was silently reused')
  assert.ok(!existsSync(path.join(workspace,'sub3-deny.txt')),
    'The denied write changed the workspace')
  console.log('Sub3 native Chromium + Rust HTTP + approve/deny + durable receipt: PASS')
} finally {
  await browser?.close().catch(()=>{})
  if(child?.exitCode===null){
    child.kill('SIGTERM')
    await sleep(300)
    if(child.exitCode===null)child.kill('SIGKILL')
  }
  await new Promise(resolve=>mock.close(resolve))
  rmSync(temp,{recursive:true,force:true})
}
