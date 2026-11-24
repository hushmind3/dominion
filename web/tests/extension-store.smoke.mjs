// Browser contract test of extension store. HTTP fixtures exercise UI only;
// the real Rust API/process lifecycle is separately covered by tests/e2e.
import assert from 'node:assert/strict'
import {spawn} from 'node:child_process'
import {fileURLToPath} from 'node:url'
import {dirname,resolve} from 'node:path'
import {chromium} from 'playwright'

const root=resolve(dirname(fileURLToPath(import.meta.url)),'..')
const origin='http://127.0.0.1:5196'
const server=spawn(process.execPath,[resolve(root,'node_modules/vite/bin/vite.js'),'--host','127.0.0.1','--port','5196','--strictPort'],{cwd:root,stdio:['ignore','pipe','pipe']})
let logs='',browser
for(const stream of [server.stdout,server.stderr])stream.on('data',data=>logs=(logs+data.toString()).slice(-3000))
async function ready(){
 for(let i=0;i<75;i++){
  if(server.exitCode!==null)throw Error('Vite quit: '+logs)
  try{if((await fetch(origin)).ok)return}catch{}
  await new Promise(done=>setTimeout(done,200))
 }
 throw Error('Vite did not start: '+logs)
}
try{
 await ready()
 browser=await chromium.launch({headless:true})
 const page=await browser.newPage({viewport:{width:1280,height:800},locale:'ko-KR'})
 const errors=[],calls=[]
 page.on('pageerror',e=>errors.push(e.message))
 const source='https://example.invalid/catalog.json'
 const manifest={sdkVersion:1,id:'fixture',name:'문서 도우미',version:'1.0.0',description:'문서 처리 도구',runtime:'python',artifact:{url:'https://example.invalid/entry.py',sha256:'a'.repeat(64)},tools:[{name:'echo',description:'Echo text',parameters:{type:'object'}}]}
 const entry={manifest,source,revision:'aaaa1111bbbb',compatible:true}
 const updated={...entry,manifest:{...manifest,version:'2.0.0'},revision:'bbbb2222cccc'}
 let installed=[],registries={urls:[source],githubDiscovery:false}
 await page.route(url=>new URL(url).pathname.startsWith('/api/'),async route=>{
  const req=route.request(),url=new URL(req.url()),path=url.pathname,method=req.method()
  if(path==='/api/events'){await route.fulfill({status:200,contentType:'text/event-stream',body:': fixture\n\n'});return}
  let body=[]
  if(path==='/api/health')body={branch:'dominion-sub2',commit:'extension-store-fixture'}
  else if(path==='/api/status')body={jobs:0,providers:0,agents:0}
  else if(path==='/api/settings/owner-profile')body={id:'owner',name:'나'}
  else if(path==='/api/policy'||path.startsWith('/api/settings/'))body={}
  else if(path==='/api/model-library/runtime')body={status:'idle'}
  else if(path==='/api/activity/teams')body={teams:[],leadJobs:[],remoteWorkers:[],remoteConfigured:false}
  else if(path.startsWith('/api/extensions')){
   const data=method==='POST'?req.postDataJSON():null
   calls.push({path,method,data,q:url.searchParams.get('q')})
   if(path==='/api/extensions/registries'){
    if(method==='POST')registries=data
    body=registries
   }else if(path==='/api/extensions/catalog')body={items:[entry],errors:[]}
   else if(path==='/api/extensions'){
    if(method==='POST'){
     assert.equal(data.confirmed,true);assert.equal(data.source,source)
     const item=data.manifest.version==='2.0.0'?updated:entry
     body={id:item.manifest.id,manifest:item.manifest,source,revision:item.revision,enabled:false}
     installed=[body]
    }else body=installed
   }else if(path==='/api/extensions/fixture/enable'){
    assert.equal(data.confirmed,data.enabled)
    installed=[{...installed[0],enabled:data.enabled}];body=installed[0]
   }else if(path==='/api/extensions/fixture/status')body={id:'fixture',enabled:installed[0].enabled,compatible:true,integrityOk:true,runtimeOk:true}
   else if(path==='/api/extensions/fixture/versions')body={installedRevision:installed[0].revision,items:[updated],reason:'checked'}
   else if(path==='/api/extensions/fixture'&&method==='DELETE'){installed=[];body={removed:true}}
  }
  await route.fulfill({status:200,contentType:'application/json',body:JSON.stringify(body)})
 })
 await page.goto(origin+'/#/connections',{waitUntil:'domcontentloaded'})
 await page.getByRole('button',{name:'확장 프로그램',exact:true}).click({timeout:15000})
 const store=page.locator('[data-extension-store]')
 await store.waitFor()
 const search=store.getByRole('textbox',{name:'확장 프로그램 검색'})
 await search.fill('문서')
 await store.getByRole('button',{name:'검색',exact:true}).click()
 await store.getByRole('button',{name:'설치',exact:true}).click()
 let dialog=page.getByRole('dialog',{name:'문서 도우미'})
 await dialog.getByText('설치 전 확인').waitFor()
 assert.equal(calls.filter(c=>c.path==='/api/extensions'&&c.method==='POST').length,0)
 await dialog.getByRole('button',{name:'확인하고 설치'}).click()
 await dialog.getByRole('button',{name:'활성화',exact:true}).waitFor()
 assert.equal(installed[0].enabled,false)
 await dialog.getByRole('button',{name:'활성화',exact:true}).click()
 await dialog.getByRole('button',{name:'확인하고 활성화'}).click()
 await dialog.getByRole('button',{name:'비활성화',exact:true}).waitFor()
 await dialog.getByRole('button',{name:'상태 확인'}).click()
 await dialog.getByText('파일 무결성 정상').waitFor()
 await dialog.getByRole('button',{name:'버전 확인'}).click()
 await dialog.getByRole('button',{name:'버전 검토'}).click()
 await dialog.getByRole('button',{name:'확인하고 설치'}).click()
 await dialog.getByRole('button',{name:'활성화',exact:true}).waitFor()
 assert.equal(installed[0].manifest.version,'2.0.0')
 assert.equal(installed[0].enabled,false,'updates require fresh activation')
 await dialog.getByRole('button',{name:'닫기',exact:true}).click()
 await store.getByRole('combobox',{name:'활성 상태 필터'}).selectOption('enabled')
 await store.getByText('조건에 맞는 확장이 없습니다',{exact:true}).waitFor()
 await store.getByRole('combobox',{name:'활성 상태 필터'}).selectOption('all')
 await page.setViewportSize({width:390,height:844})
 await store.getByRole('button',{name:'관리',exact:true}).click()
 dialog=page.getByRole('dialog',{name:'문서 도우미'})
 await dialog.getByRole('button',{name:'삭제',exact:true}).click()
 await dialog.getByRole('button',{name:'데이터 포함 삭제'}).click()
 await store.getByText('설치된 확장이 없습니다',{exact:true}).waitFor()
 assert.equal(installed.length,0)
 assert.ok(calls.some(c=>c.path==='/api/extensions/catalog'&&c.q==='문서'))
 assert.ok(calls.some(c=>c.path==='/api/extensions/fixture/versions'))
 assert.deepEqual(errors,[])
 console.log('PASS: extension store search, review, install, activation, status, publisher version update, filter, mobile removal')
}finally{await browser?.close();server.kill('SIGTERM')}
