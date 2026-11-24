// Real production frontend + real Rust HTTP server. No request interception,
// no live provider, no owner's database. History rows below are explicit fixtures.
import assert from 'node:assert/strict'
import {spawn} from 'node:child_process'
import {once} from 'node:events'
import {mkdtemp,mkdir,rm,writeFile} from 'node:fs/promises'
import {tmpdir} from 'node:os'
import {resolve,join} from 'node:path'
import {fileURLToPath} from 'node:url'
import {DatabaseSync} from 'node:sqlite'
import {chromium} from 'playwright'

const root=fileURLToPath(new URL('../../',import.meta.url))
const results=resolve(root,'web/test-results/native-ux')
await mkdir(results,{recursive:true})
const scratch=await mkdtemp(join(tmpdir(),'dominion-browser-'))
const workspace=join(scratch,'workspace'),data=join(scratch,'data')
await mkdir(workspace)
const env={...process.env,DOMINION_PORT:'0',DOMINION_HOME:scratch,DOMINION_DATA:data,
 DOMINION_WORKSPACE:workspace,DOMINION_WEB:resolve(root,'web/dist')}
delete env.DOMINION_ADMIN_TOKEN
delete env.DOMINION_DESKTOP_PARENT
let logs='',base='',browser,page,db
const child=spawn(resolve(root,'target/debug/dominion-rust'),[],{cwd:scratch,env,stdio:['ignore','pipe','pipe']})
child.stdout.on('data',chunk=>{logs+=chunk;const match=logs.match(/listening on (http:\/\/127\.0\.0\.1:\d+)/);if(match)base=match[1]})
child.stderr.on('data',chunk=>{logs+=chunk})
let spawnError
child.on('error',e=>{spawnError=e})
const delay=ms=>new Promise(r=>setTimeout(r,ms))
async function until(fn,description,timeout=20000){
 const end=Date.now()+timeout
 let last
 while(Date.now()<end){try{if(await fn())return}catch(e){last=e}await delay(100)}
 throw Error('Timed out: '+description+(last?' ('+last.message+')':''))
}
async function api(path,body){
 const response=await fetch(base+path,{...(body===undefined?{}:{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body)}),signal:AbortSignal.timeout(8000)})
 const value=await response.json()
 assert.ok(response.ok,path+' '+JSON.stringify(value))
 return value
}
const byText=text=>page.getByText(text,{exact:true})
const button=name=>page.getByRole('button',{name,exact:true})
const screenshot=name=>page.screenshot({path:join(results,name+'.png'),fullPage:true})
const navigate=async name=>{await page.goto(base+'/#/'+name,{waitUntil:'domcontentloaded'})}
const failures=[]
try{
 await until(async()=>{if(spawnError)throw spawnError;if(child.exitCode!==null)throw Error(logs);return base&&(await fetch(base+'/api/health')).ok},'isolated native server')
 // These read-only evidence fixtures test display fidelity. They are not
 // claimed as a real model execution or as a measured improvement.
 db=new DatabaseSync(join(data,'dominion.sqlite'))
 db.exec('PRAGMA busy_timeout=5000')
 const insert=db.prepare('INSERT INTO records(kind,id,data) VALUES(?,?,?)')
 const fixture=(kind,value)=>insert.run(kind,value.id,JSON.stringify(value))
 const now=new Date().toISOString()
 fixture('execution',{id:'browser-execution',sessionId:'browser-session',agentId:'fixture-agent',status:'completed',
  request:'Browser evidence fixture',startedAt:now,taskOutcome:'answered_unverified',verified:false,
  plan:'Inspect before writing',decomposition:{subtasks:[{id:'inspect',text:'Inspect workspace',status:'completed'},{id:'write',text:'Write after inspection',dependsOn:['inspect'],status:'pending'}]},
  adaptations:[{id:'fixture-repair',observation:{tool:'workspace_write',actual:'Fixture stale state'},hypotheses:[{id:'h1',claim:'Stale state'}],hypothesisResults:[{id:'h1',status:'refuted'}],selected:{tool:'workspace_read'}}],
  programCheckpoint:{programRevision:'fixture-v2',status:'paused',pendingOperationIds:['write']},
  acceptancePassed:false,acceptanceChecks:[{type:'file_exists',path:'fixture.txt'}]})
 fixture('strategy_decision',{id:'browser-execution',selected:'fixture-policy',policyRevision:'fixture-revision'})
 fixture('algorithm_usage',{id:'browser-usage',executionId:'browser-execution',kind:'memory.retrieval',revision:'fixture-revision',status:'unrated'})
 // Queued control record deliberately does not launch a worker. This tests
 // real steering admission + persistence, not application at a model boundary.
 fixture('session',{id:'browser-sdk-session',title:'Browser SDK control fixture',nativeSdkRunId:'browser-sdk'})
 fixture('sdk_run',{id:'browser-sdk',sessionId:'browser-sdk-session',status:'queued',instruction:'Browser SDK control fixture',createdAt:now})
 db.close();db=null
 browser=await chromium.launch({headless:true})
 page=await browser.newPage({viewport:{width:1440,height:1000}})
 page.setDefaultTimeout(15000)
 page.on('pageerror',e=>failures.push(e.message))
 page.on('dialog',d=>void d.accept())
 await page.addInitScript(()=>localStorage.setItem('dominion.locale','ko'))
 await navigate('memory')
 await byText('경험, 개선안, 알고리즘과 실험 기록').click()
 await button('알고리즘').click()
 await byText('알고리즘 관리 및 비교').waitFor()
 const category=page.getByLabel('알고리즘 종류')
 await category.waitFor()
 const groups=(await api('/api/algorithms')).kinds
 const group=groups.find(g=>g.kernels?.some(k=>k!=='expression'))
 assert.ok(group)
 await category.selectOption(group.kind)
 await button('실제 평가 실행').click()
 await byText('최근 평가 결과').waitFor()
 assert.ok((await api('/api/algorithms/evaluations')).items.length)
 await byText('알고리즘 후보 등록·생성').click()
 await page.getByLabel('후보 이름').fill('browser-ui-candidate')
 await page.getByLabel('실행 커널').selectOption(group.kernels.find(k=>k!=='expression'))
 await button('후보 등록').click()
 const candidate=page.locator('article').filter({has:byText('browser-ui-candidate')})
 await candidate.waitFor()
 const registered=(await api('/api/algorithms')).kinds.flatMap(g=>g.algorithms).find(r=>r.name==='browser-ui-candidate')
 assert.ok(registered)
 await candidate.getByRole('button',{name:'수동 전환',exact:true}).click()
 await until(async()=> (await api('/api/algorithms')).kinds.flatMap(g=>g.algorithms).find(r=>r.id===registered.id)?.status==='active','native activation')
 await candidate.getByRole('button',{name:'비활성화',exact:true}).click()
 await until(async()=> (await api('/api/algorithms')).kinds.flatMap(g=>g.algorithms).find(r=>r.id===registered.id)?.status==='disabled','native disable')
 await screenshot('01-algorithm-management')
 await navigate('activity')
 await page.getByRole('button').filter({has:byText('Browser evidence fixture')}).click()
 await button('실행·토큰 상세').click()
 await byText('실제 사용 알고리즘과 선택 근거').waitFor()
 await byText('작업 분해와 의존성').waitFor()
 await byText('프로그램 재구성 및 복구 지점').waitFor()
 await byText('실패 원인 분석과 대체 전략').waitFor()
 await byText('최종 완료 조건').waitFor()
 assert.ok(await byText('미충족').isVisible())
 await screenshot('02-native-execution-evidence')
 await navigate('connections')
 await button('플러그인').click()
 const sdkPanel=page.getByRole('region',{name:'SDK 연결 및 실행 기록',exact:true})
 await sdkPanel.getByRole('button').filter({has:byText('Browser SDK control fixture')}).click()
 await page.getByLabel('새 지시').fill('추가 지시 브라우저 검증')
 await button('지시 전달').click()
 await until(async()=> (await api('/api/sdk/runs/browser-sdk')).steering?.some(s=>s.instruction==='추가 지시 브라우저 검증'),'native steering receipt')
 await byText('추가 지시 브라우저 검증').waitFor()
 await screenshot('03-sdk-steering')
 await page.setViewportSize({width:390,height:844})
 await screenshot('04-sdk-mobile')
 assert.deepEqual(failures,[],'uncaught browser errors')
 console.log('PASS: real native algorithm evaluate/register/activate/disable, recorded execution evidence, SDK steering admission/persistence')
}catch(e){
 if(page)await screenshot('failure').catch(()=>{})
 throw e
}finally{
 db?.close()
 await browser?.close()
 if(child.exitCode===null&&child.signalCode===null){child.kill('SIGTERM');await Promise.race([once(child,'exit'),delay(3000)]);if(child.exitCode===null&&child.signalCode===null)child.kill('SIGKILL')}
 await writeFile(join(results,'server.log'),logs)
 await writeFile(join(results,'browser-errors.json'),JSON.stringify(failures,null,2))
 await rm(scratch,{recursive:true,force:true})
}
