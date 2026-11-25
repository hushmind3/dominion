import test from 'node:test';
import assert from 'node:assert/strict';
import {playbookKey,extractPlaybook,playbookHints} from '../src/assistant_playbook.js';
import {modelMessages} from '../src/assistant_browser.js';
const at=1_700_000_000_000;
const completed={
 account:'member-A',root:'example.com',goal:'사업자 조회',status:'completed',result:{evidence:'상태 확인'},
 trace:[{action:'navigate',label:'조회 메뉴',page:'https://example.com/open?q=private'},{action:'click',label:'검색하기',page:'https://example.com/results'},{action:'fill',label:'주민등록번호',page:'https://example.com/id'},{action:'click',label:'a@example.com',page:'https://example.com/people'}]
};
test('a completed, evidenced job yields only non-sensitive, site-scoped path hints',async()=>{
 const key=await playbookKey(completed.root,completed.goal);
 const record=extractPlaybook(completed,key,at);
 assert.equal(record.key,key);
 assert.deepEqual(record.steps.map(s=>s.label),['조회 메뉴','검색하기']);
 assert.equal(record.steps[0].page,'https://example.com/open');
 assert.equal(JSON.stringify(record).includes('q=private'),false);
 assert.equal(JSON.stringify(record).includes('주민등록번호'),false);
 assert.equal(JSON.stringify(record).includes(completed.goal),false);
 assert.equal(playbookHints(record,{account:'member-A',site:completed.root,key,at:at+1000}).length,2);
 assert.equal(playbookHints(record,{account:'member-B',site:completed.root,key,at:at+1000}).length,0);
 assert.equal(playbookHints(record,{account:'member-A',site:'other.com',key,at:at+1000}).length,0);
 assert.equal(playbookHints(record,{account:'member-A',site:completed.root,key:'playbook:abc',at:at+1000}).length,0);
 assert.equal(playbookHints(record,{account:'member-A',site:completed.root,key,at:record.expiresAt}).length,0);
});
test('failed or unverified job never enters the playbook library',async()=>{
 const key=await playbookKey('example.com','업무');
 for(const status of ['failed','running','needs_review','cancelled'])assert.equal(extractPlaybook({...completed,status},key,at),null);
 assert.equal(extractPlaybook({...completed,result:null},key,at),null);
 assert.equal(extractPlaybook({...completed,trace:[{action:'fill',label:'계좌번호',page:'https://example.com'}]},key,at),null);
});
test('playbook hints are context, not actions; current-page evidence is still required',async()=>{
 const key=await playbookKey('example.com','사업자 조회');
 const record=extractPlaybook(completed,key,at);
 const model=modelMessages({goal:completed.goal,root:completed.root,trace:[],documents:[],playbookHints:playbookHints(record,{account:'member-A',site:'example.com',key,at:at+500})},{title:'test',url:'https://example.com',text:'조회 페이지',elements:[]});
 const input=JSON.parse(model[1].content);
 assert.equal(input.priorSuccess.length,2);
 assert.match(model[0].content,/현재 화면/);
 assert.equal(input.page.url,'https://example.com/');
});
