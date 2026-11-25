import test from 'node:test';import assert from 'node:assert/strict';import {safeUrl,browserScope,decodePlan,needsHumanApproval,createBrowserObject,browserRoute} from '../src/assistant_browser.js';
test('approved HTTPS site boundary and sensitive fields',()=>{for(const u of ['http://example.com','https://localhost','https://127.0.0.1','https://[::1]','https://a:b@example.com','https://example.com:444','https://foo.internal'])assert.equal(safeUrl(u),null);assert.equal(browserScope('https://www.hometax.go.kr').root,'hometax.go.kr');const snap={text:'접수가 완료되었습니다.',elements:[{index:0,tag:'a',href:'https://evil.test',label:'외부'},{index:1,tag:'input',type:'password',label:'비밀번호',auth:true},{index:2,tag:'button',type:'submit',label:'신청'}]};assert.throws(()=>decodePlan('{"action":"navigate","index":0}',snap,'hometax.go.kr'));assert.throws(()=>decodePlan('{"action":"fill","index":1,"value":"secret"}',snap,'hometax.go.kr'));assert.equal(decodePlan('{"action":"click","index":2}',snap,'hometax.go.kr').action,'click');assert.throws(()=>decodePlan('{"action":"done","result":"성공","evidence":"없는 근거"}',snap,'hometax.go.kr'));assert.equal(decodePlan('{"action":"done","result":"접수됨","evidence":"접수가 완료되었습니다."}',snap,'hometax.go.kr').action,'done')});
test('durable job waits for exact approval, performs one real action then verifies evidence',async()=>{
 const data=new Map();const storage={get:async k=>data.get(k),put:async(k,v)=>data.set(k,structuredClone(v)),delete:async k=>data.delete(k),list:async({prefix=''}={})=>new Map([...data].filter(([k])=>k.startsWith(prefix))),setAlarm:async()=>{},deleteAlarm:async()=>{}};
 let clicked=0,closed=0,models=0;
 const env={ASSISTANT_BROWSER_ENABLED:'1',ASSISTANT_BROWSER_DAILY_LIMIT:'2',ASSISTANT_BROWSER:{},DB:{prepare(){return{bind(){return{first:async()=>({id:'s',used:1})}}}}},BROWSER:{acquire:async opts=>{assert.ok(opts.guardrails.allowedDomains.includes('example.com'));assert.equal(opts.recording,false);return {sessionId:'browser'}},connectSession(){},getLiveView:async()=>({devtoolsFrontendUrl:'https://live.browser.run/ui/view?token=test'}),closeSession:async()=>{closed++},devtools:{newTarget:async()=>({id:'tab'})}}};
 const driver=async()=>({evaluate:async expr=>expr.includes('function applyElement')?{ok:true,x:10,y:20}:{url:'https://example.com/',title:'신청',auth:false,text:clicked?'접수가 완료되었습니다.':'신청 준비',elements:[{index:0,tag:'button',type:'submit',label:'신청',href:'',auth:false}]},send:async(method,params)=>{if(method==='Input.dispatchMouseEvent'&&params.type==='mouseReleased')clicked++;return{}},close(){}});
 const Cls=createBrowserObject(async()=>{models++;return clicked?'{"action":"done","result":"접수됨","evidence":"접수가 완료되었습니다."}':'{"action":"click","index":0}'},driver),obj=new Cls({storage},env);
 const call=(p,b)=>obj.fetch(new Request('https://internal/'+p,{method:'POST',body:JSON.stringify({account:'google:owner',appSession:'s',id:'task-1234',...b})}));
 assert.equal((await call('start',{confirmed:true,url:'https://example.com/',goal:'신청'})).status,202);await obj.alarm();await obj.alarm();let j=await (await call('jobs',{})).json();assert.equal(j.status,'awaiting_approval');assert.equal(clicked,0);
 assert.equal((await call('approve',{approvalId:'incorrect'})).status,409);assert.equal((await call('approve',{approvalId:j.approval.id})).status,200);await obj.alarm();await obj.alarm();j=await (await call('jobs',{})).json();assert.equal(j.status,'completed');assert.equal(j.result.evidence,'접수가 완료되었습니다.');assert.equal(clicked,1);assert.equal(models,2);assert.equal(closed,1);assert.ok(!('session'in j));await obj.alarm();assert.equal(clicked,1);
});
test('routine navigation and reading run without button-by-button approval, but submitted actions wait',()=>{
 const snap={url:'https://www.example.com/start',text:'검색 화면',elements:[]};
 const target=(tag,type,label,href='')=>({index:0,tag,type,label,href,auth:false});
 assert.equal(needsHumanApproval({action:'click',target:target('a','','목록','https://www.example.com/list')},snap,'example.com'),false);
 assert.equal(needsHumanApproval({action:'click',target:target('button','button','검색')},snap,'example.com'),false);
 assert.equal(needsHumanApproval({action:'click',target:target('button','button','더 보기')},snap,'example.com'),false);
 assert.equal(needsHumanApproval({action:'download_pdf'},snap,'example.com'),false);
 assert.equal(needsHumanApproval({action:'save_pdf'},snap,'example.com'),false);
 assert.equal(needsHumanApproval({action:'click',target:target('button','submit','사업자등록 신청')},snap,'example.com'),true);
 assert.equal(needsHumanApproval({action:'click',target:target('a','','장바구니 구매','https://www.example.com/buy')},snap,'example.com'),true);
 assert.equal(needsHumanApproval({action:'click',target:target('a','','이동','https://other.example.net/list')},snap,'example.com'),true);
 assert.equal(needsHumanApproval({action:'click',target:target('button','button','확인')},{...snap,text:'최종 제출 확인'},'example.com'),true);
 assert.equal(needsHumanApproval({action:'click',target:target('button','button','알 수 없는 버튼')},snap,'example.com'),true);
});
test('durable worker autonomously clicks a read-only lookup and verifies its result',async()=>{
 const data=new Map(),storage={get:async k=>data.get(k),put:async(k,v)=>data.set(k,structuredClone(v)),delete:async k=>data.delete(k),list:async({prefix=''}={})=>new Map([...data].filter(([k])=>k.startsWith(prefix))),setAlarm:async()=>{},deleteAlarm:async()=>{}};
 let clicks=0,closed=0;
 const env={ASSISTANT_BROWSER_ENABLED:'1',ASSISTANT_BROWSER_DAILY_LIMIT:'2',ASSISTANT_BROWSER:{},DB:{prepare(){return{bind(){return{first:async()=>({id:'s',used:1})}}}}},BROWSER:{acquire:async()=>({sessionId:'browser'}),connectSession(){},getLiveView:async()=>({devtoolsFrontendUrl:'https://live.browser.run/ui/view?token=test'}),closeSession:async()=>{closed++},devtools:{newTarget:async()=>({id:'tab'})}}};
 const driver=async()=>({evaluate:async expression=>expression.includes('function applyElement')?{ok:true,x:10,y:20}:{url:'https://example.com/',title:'조회',auth:false,text:clicks?'조회 결과입니다.':'조회 준비',elements:[{index:0,tag:'button',type:'button',label:'조회',href:'',auth:false}]},send:async(method,params)=>{if(method==='Input.dispatchMouseEvent'&&params.type==='mouseReleased')clicks++;return{}},close(){}});
 const Cls=createBrowserObject(async()=>clicks?'{"action":"done","result":"조회 완료","evidence":"조회 결과입니다."}':'{"action":"click","index":0}',driver),obj=new Cls({storage},env);
 const call=(p,b={})=>obj.fetch(new Request('https://internal/'+p,{method:'POST',body:JSON.stringify({account:'google:owner',appSession:'s',id:'task-readonly-1234',...b})}));
 assert.equal((await call('start',{confirmed:true,url:'https://example.com/',goal:'조회'})).status,202);
 await obj.alarm();await obj.alarm();await obj.alarm();
 const job=await (await call('jobs')).json();
 assert.equal(job.status,'completed');assert.equal(clicks,1);assert.equal(closed,1);
 assert.equal(job.result.evidence,'조회 결과입니다.');
});
test('wrong app capability cannot access browser',async()=>{assert.equal((await browserRoute(new Request('https://example.com/v1/assistant/browser/jobs'),{},{account:async()=>({scope:'app',capability:'inference'})})).status,403)});

test('human handoff pauses model decisions until live view completes and reinspects',async()=>{
 const data=new Map(),storage={get:async k=>data.get(k),put:async(k,v)=>data.set(k,structuredClone(v)),delete:async k=>data.delete(k),list:async({prefix=''}={})=>new Map([...data].filter(([k])=>k.startsWith(prefix))),setAlarm:async()=>{},deleteAlarm:async()=>{}};
 let auth=true,active=false,models=0,closed=0;
 const env={ASSISTANT_BROWSER_ENABLED:'1',ASSISTANT_BROWSER_DAILY_LIMIT:'2',ASSISTANT_BROWSER:{},DB:{prepare(){return{bind(){return{first:async()=>({id:'s',used:1})}}}}},BROWSER:{acquire:async()=>({sessionId:'browser'}),connectSession(){},getLiveView:async()=>({devtoolsFrontendUrl:'https://live.browser.run/ui/view?token=ephemeral'}),closeSession:async()=>{closed++},devtools:{newTarget:async()=>({id:'tab'})}}};
 const driver=async()=>({evaluate:async()=>({url:'https://example.com/',title:'테스트',auth,text:auth?'':'로그인 후 조회 결과입니다.',elements:[]}),send:async(method)=>{if(method==='Cloudflare.handoff'){active=true;return{handoffId:'h'}}if(method==='Cloudflare.getHandoffState')return {active};return{}},close(){}});
 const Cls=createBrowserObject(async()=>{models++;return '{"action":"done","result":"조회 결과 확인","evidence":"로그인 후 조회 결과입니다."}'},driver),obj=new Cls({storage},env);
 const call=(p,b={})=>obj.fetch(new Request('https://internal/'+p,{method:'POST',body:JSON.stringify({account:'google:owner',appSession:'s',id:'task-2345',...b})}));
 assert.equal((await call('start',{confirmed:true,url:'https://example.com/',goal:'조회'})).status,202);await obj.alarm();await obj.alarm();assert.equal((await (await call('jobs')).json()).status,'awaiting_human');assert.equal(models,0);
 assert.equal((await (await call('view')).json()).url,'https://live.browser.run/ui/view?token=ephemeral');assert.ok(!JSON.stringify([...data.values()]).includes('ephemeral'));
 await obj.alarm();assert.equal(models,0);auth=false;active=false;await obj.alarm();await obj.alarm();assert.equal((await (await call('jobs')).json()).status,'completed');assert.equal(models,1);assert.equal(closed,1);
});

import {protect,reveal,reusableCookies,pdfBytes,storeDocument,loadDocument} from '../src/browser_vault.js';
test('vault binds cookies to site and account; rejects other domains and plaintext',async()=>{
 const env={BROWSER_VAULT_SECRET:'secret'.repeat(8)};
 const c={name:'session',value:'private-cookie',domain:'www.example.com',path:'/',secure:true,httpOnly:true,expires:-1};
 const cookies=reusableCookies([c,{...c,domain:'other.com'},{...c,secure:false},{...c,partitionKey:{}}],'example.com');
 assert.equal(cookies.length,1);assert.equal(cookies[0].url,'https://www.example.com/');assert.equal(cookies[0].domain,undefined);
 const sealed=await protect(env,'profile:a:example.com',cookies);assert.ok(!sealed.includes('private-cookie'));assert.deepEqual(await reveal(env,'profile:a:example.com',sealed),cookies);await assert.rejects(reveal(env,'profile:b:example.com',sealed));
});
test('PDF vault checks signature, encrypts file and authorizes read by owner and job',async()=>{
 const env={BROWSER_VAULT_SECRET:'secret'.repeat(8)},data=new Map(),storage={get:async k=>data.get(k),put:async v=>{for(const [k,x] of Object.entries(v))data.set(k,x)}};
 const raw=btoa('%PDF-1.7\nsample'),j={id:'job-1234',account:'google:a'};assert.throws(()=>pdfBytes(btoa('<html>fake</html>')));
 const m=await storeDocument(storage,env,j,raw,'original','https://example.com/document');assert.equal(m.kind,'original');assert.equal(m.size,15);assert.ok(!JSON.stringify([...data.values()]).includes(raw));
 assert.equal(await loadDocument(storage,env,'google:b',j.id,m.id),null);assert.equal(await loadDocument(storage,env,j.account,'wrong',m.id),null);assert.equal((await loadDocument(storage,env,j.account,j.id,m.id)).base64,raw);
});
test('download action only accepts observed same-origin links; page printing is distinct',()=>{
 const snap={url:'https://example.com/page',text:'문서',elements:[{index:0,tag:'a',href:'https://example.com/a.pdf',label:'PDF'},{index:1,tag:'a',href:'https://files.example.com/a.pdf',label:'외부'}]};
 assert.equal(decodePlan('{"action":"download_pdf","index":0}',snap,'example.com').action,'download_pdf');assert.throws(()=>decodePlan('{"action":"download_pdf","index":1}',snap,'example.com'));assert.equal(decodePlan('{"action":"save_pdf"}',snap,'example.com').action,'save_pdf');
});

import {validUpload,acceptsFile} from '../src/browser_upload.js';
test('upload checks file signature and limits selected site input type',()=>{
 const f=validUpload({name:'sample.pdf',mime:'application/pdf',base64:btoa('%PDF-1.7\nsample')});
 assert.equal(f.size,15);assert.ok(acceptsFile('.pdf',f));assert.ok(!acceptsFile('image/*',f));
 assert.throws(()=>validUpload({...f,name:'../sample.pdf'}));assert.throws(()=>validUpload({...f,base64:btoa('not a pdf')}));
 const snap={url:'https://example.com/page',text:'첨부',elements:[{index:0,tag:'input',type:'file',label:'자료',accept:'application/pdf',filled:false,auth:false},{index:1,tag:'input',type:'file',filled:true}]};
 assert.equal(decodePlan('{"action":"upload","index":0}',snap,'example.com').action,'upload');assert.throws(()=>decodePlan('{"action":"upload","index":1}',snap,'example.com'));
});
