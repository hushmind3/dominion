import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import worker from '../src/index.js';

const ORIGIN='https://staging.dominion.example';
const TOKEN='a'.repeat(48);
const headers=(extra={})=>({origin:ORIGIN,...extra});
const request=(path,init={})=>new Request('https://sub3.worker.test'+path,init);

function dbFixture({verified=false,rejectReservation=null}={}){
  const quota=new Map();
  const calls=[];
  const db={
    calls,
    prepare(query){
      return {
        bind(...parameters){
          calls.push({query,parameters});
          return {
            async first(){
              if(query.includes('FROM sessions s JOIN accounts a'))return {id:'google:isolated-test',credits:5000};
              if(query.includes('FROM identity_verifications'))return verified?{verified_at:1700000000,provider:'portone'}:null;
              if(query.includes('FROM subscriptions'))return null;
              if(query.includes('FROM free_quota'))return {used:quota.get(String(parameters[0]))||0};
              if(query.includes('INSERT INTO free_quota')){
                // Mirror D1's ON CONFLICT ... WHERE used < ? RETURNING used.
                // A rejected reservation must not increment the stored counter.
                const key=String(parameters[0]),limit=Number(parameters[1]);
                const used=quota.get(key)||0;
                if(!Number.isSafeInteger(limit)||used>=limit)return null;
                quota.set(key,used+1);
                return {used:used+1};
              }
              return null;
            },
            async run(){
              if(query.startsWith('INSERT INTO usage(') && rejectReservation)
                throw Error(rejectReservation);
              return {success:true};
            },
            async all(){return {results:[]};},
          };
        },
      };
    },
  };
  return db;
}
function env(extra={}){
  return {ALLOWED_ORIGIN:ORIGIN,DB:dbFixture(),FREE_MODELS_JSON:'[]',...extra};
}
const bearer=()=>headers({authorization:'Bearer '+TOKEN});

test('public health is safe without configured identity or upstream credentials',async()=>{
  const response=await worker.fetch(request('/health'),{});
  assert.equal(response.status,200);
  assert.equal((await response.json()).service,'DOMINION Cloud');
  assert.equal(response.headers.get('cache-control'),'no-store');
});
test('CORS exact-origin checks reject untrusted origins',async()=>{
  const good=await worker.fetch(request('/v1/me',{method:'OPTIONS',headers:headers()}),env());
  assert.equal(good.status,204);
  assert.equal(good.headers.get('access-control-allow-origin'),ORIGIN);
  const bad=await worker.fetch(request('/v1/me',{headers:{origin:'https://evil.example'}}),env());
  assert.equal(bad.status,403);
  assert.notEqual(bad.headers.get('access-control-allow-origin'),'https://evil.example');
});
test('unconfigured gateway and unauthenticated account routes fail closed',async()=>{
  assert.equal((await worker.fetch(request('/v1/models'),{})).status,503);
  for(const [method,path] of [
    ['GET','/v1/models'],['GET','/v1/me'],['GET','/v1/billing'],
    ['GET','/v1/identity'],['GET','/v1/billing/payments'],
    ['POST','/v1/billing/checkout'],['POST','/v1/chat/completions'],
  ]){
    const response=await worker.fetch(request(path,{method,headers:headers()}),env());
    assert.equal(response.status,401,path+' must require an authenticated member');
  }
});
test('unconfigured Google client cannot create account sessions',async()=>{
  const environment=env();
  const response=await worker.fetch(request('/v1/auth/google',{
    method:'POST',headers:headers({'content-type':'application/json'}),
    body:JSON.stringify({credential:'not-a-google-jwt'}),
  }),environment);
  assert.equal(response.status,503);
  assert.equal(environment.DB.calls.length,0);
});
test('account model catalog projects allowlisted metadata without provider secrets',async()=>{
  const environment=env({
    AI:{run:async()=>({response:'hello'})},
    GROQ_API_KEY:'worker-only-provider-secret',
    PAID_MODELS_JSON:JSON.stringify([{
      id:'paid-demo',provider:'groq',upstream_model:'private-provider-model',
      input_micro_krw:3,output_micro_krw:6,
    }]),
  });
  const response=await worker.fetch(request('/v1/models',{headers:bearer()}),environment);
  assert.equal(response.status,200);
  const body=await response.json();
  assert.ok(body.data.some(m=>m.id==='dominion/free-auto'&&m.free));
  assert.ok(body.data.some(m=>m.id==='@cf/meta/llama-3.1-8b-instruct-fast'&&m.free));
  assert.ok(body.data.some(m=>m.id==='paid-demo'&&!m.free));
  assert.ok(!JSON.stringify(body).includes('worker-only-provider-secret'));
});
test('authenticated free streaming uses Worker-owned inference with quota accounting',async()=>{
  const environment=env({
    AI:{run:async(_model,options)=>{
      assert.equal(options.messages[0].content,'Hello');
      return {response:'Hello from sub3'};
    }},
    FREE_DAILY_LIMIT:'1',
  });
  const body={model:'@cf/meta/llama-3.1-8b-instruct-fast',
    messages:[{role:'user',content:'Hello'}],stream:true,max_tokens:16};
  const send=()=>worker.fetch(request('/v1/chat/completions',{
    method:'POST',headers:bearer(),body:JSON.stringify(body),
  }),environment);
  const first=await send();
  assert.equal(first.status,200);
  assert.match(first.headers.get('content-type')||'',/text\/event-stream/);
  const streamed=await first.text();
  assert.match(streamed,/Hello from sub3/);
  assert.match(streamed,/data: \[DONE\]/);
  const second=await send();
  assert.equal(second.status,429);
});
test('identity verification gates both paid inference and checkout',async()=>{
  const environment=env({
    GROQ_API_KEY:'worker-only-secret',
    PAID_MODELS_JSON:JSON.stringify([{
      id:'paid-demo',provider:'groq',upstream_model:'private',
      input_micro_krw:2,output_micro_krw:4,
    }]),
    TOSS_CLIENT_KEY:'test-public-client',TOSS_SECRET_KEY:'worker-only-payment-secret',
    PLAN_PRO_KRW:'9990',
  });
  const paid=await worker.fetch(request('/v1/chat/completions',{
    method:'POST',headers:bearer(),
    body:JSON.stringify({model:'paid-demo',messages:[{role:'user',content:'hi'}]})
  }),environment);
  assert.equal(paid.status,403);
  const checkout=await worker.fetch(request('/v1/billing/checkout',{
    method:'POST',headers:bearer(),body:JSON.stringify({plan:'pro'})
  }),environment);
  assert.equal(checkout.status,403);
  assert.ok(!environment.DB.calls.some(c=>c.query.startsWith('INSERT INTO checkout_orders')));
});
test('React cloud account actions target routes exposed by imported Worker',()=>{
  const react=readFileSync(new URL('../../web/src/components/DominionCloudAccount.tsx',import.meta.url),'utf8');
  const workerSource=readFileSync(new URL('../src/index.js',import.meta.url),'utf8');
  for(const route of [
    '/v1/auth/google','/v1/models','/v1/billing','/v1/identity',
    '/v1/identity/start','/v1/identity/confirm',
    '/v1/billing/checkout','/v1/billing/confirm','/v1/chat/completions'
  ]){
    assert.ok(workerSource.includes(route),'Worker missing API '+route);
    if(route!=='/v1/chat/completions')assert.ok(react.includes(route),'React account card missing '+route);
  }
});

test('paid reservations reject concurrent-budget overflow and insufficient credit without upstream calls',async()=>{
  for(const [reason,status] of [
    ['paid_daily_budget_reached',429],
    ['insufficient_prepaid_credits',402],
  ]){
    const environment=env({
      DB:dbFixture({verified:true,rejectReservation:reason}),
      GROQ_API_KEY:'test-only-provider-secret',
      PAID_DAILY_BUDGET_MICRO_KRW:'200000',
      PAID_MODELS_JSON:JSON.stringify([{
        id:'paid-demo',provider:'groq',upstream_model:'private',
        input_micro_krw:2,output_micro_krw:4,
      }]),
    });
    const response=await worker.fetch(request('/v1/chat/completions',{
      method:'POST',headers:bearer(),
      body:JSON.stringify({model:'paid-demo',messages:[{role:'user',content:'hi'}]}),
    }),environment);
    assert.equal(response.status,status,reason);
    assert.equal(environment.DB.calls.filter(x=>x.query.startsWith('INSERT INTO usage(')).length,1);
    assert.ok(!environment.DB.calls.some(x=>x.query.startsWith('UPDATE accounts SET credits=credits-')),
      'credit debit must happen within the D1 trigger, not in a separate Worker statement');
  }
});
