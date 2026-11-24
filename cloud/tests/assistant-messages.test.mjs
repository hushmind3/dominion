import {test} from 'node:test';import assert from 'node:assert/strict';import {DatabaseSync} from 'node:sqlite';import {readFileSync} from 'node:fs';import {messages,publish} from '../src/assistant_messages.js';
test('inbox scope, opt-in generic push, duplicate suppression',async()=>{
 const db=new DatabaseSync(':memory:');db.exec(readFileSync(new URL('../schema.sql',import.meta.url),'utf8'));
 const t=Math.floor(Date.now()/1000);
 for(const a of ['a','b']){db.prepare('INSERT INTO accounts(id,created_at) VALUES(?,?)').run(a,t);db.prepare("INSERT INTO app_sessions(id,token_hash,app_name,capability,ip_hash,account_id,status,created_at,expires_at) VALUES(?,?,?,'assistant_remote','ip',?,'approved',?,?)").run(a,a,'app',a,t,t+1000)}
 db.prepare("INSERT INTO assistant_devices(id,account_id,name,token_hash,last_seen) VALUES('pc','a','PC','key',0)").run();
 const DB={prepare(sql){return{bind(...a){return{first:async()=>db.prepare(sql).get(...a)||null,all:async()=>({results:db.prepare(sql).all(...a)}),run:async()=>({meta:{changes:Number(db.prepare(sql).run(...a).changes)}})}}}}};
 const env={DB,ASSISTANT_PUSH_ENABLED:'1'},a={id:'a',scope:'app',capability:'assistant_remote',app_session_id:'a'},b={...a,id:'b',app_session_id:'b'};
 const token='ExpoPushToken[abcdefghijklmnop]';
 const register=u=>messages(new Request('https://test',{method:'POST',body:JSON.stringify({token})}),env,u,'push/register');
 assert.equal((await register(a)).status,200);assert.equal((await register(b)).status,409);
 let pushes=0;const send=async(url,o)=>{pushes++;assert.ok(!o.body.includes('private answer'));return new Response('{}')};
 const d={id:'pc',account_id:'a'},msg={id:'task:1',text:'private answer'};
 assert.equal((await publish(env,d,msg,send)).status,200);assert.equal((await publish(env,d,msg,send)).status,200);assert.equal(pushes,1);
 assert.equal((await (await messages(new Request('https://test'),env,b,'messages')).json()).messages.length,0);
 assert.equal((await (await messages(new Request('https://test'),env,a,'messages')).json()).messages[0].text,'private answer');
});
