import {test} from 'node:test'
import assert from 'node:assert/strict'
import {DatabaseSync} from 'node:sqlite'
import {emailLoginReady,startEmailLogin,verifyEmailLogin,pollEmailLogin,emailAuthTest} from '../src/email_auth.js'
import worker from '../src/index.js'

// Node SQLite executes the exact prepared SQL used by the Cloudflare D1 adapter.
// batch() models D1's transaction boundary so replay assertions are meaningful.
function fixture(){
 const db=new DatabaseSync(':memory:')
 db.exec("CREATE TABLE accounts(id TEXT PRIMARY KEY,email TEXT NOT NULL,credits INTEGER NOT NULL DEFAULT 0,created_at INTEGER NOT NULL)")
 db.exec("CREATE TABLE sessions(token_hash TEXT PRIMARY KEY,account_id TEXT NOT NULL REFERENCES accounts(id),expires_at INTEGER NOT NULL)")
 db.exec("CREATE TABLE email_login_attempts(id TEXT PRIMARY KEY,email TEXT NOT NULL,email_hash TEXT NOT NULL,ip_hash TEXT NOT NULL,poll_hash TEXT NOT NULL,verify_hash TEXT NOT NULL,status TEXT NOT NULL,created_at INTEGER NOT NULL,expires_at INTEGER NOT NULL)")
 const adapter={
  prepare(sql){
   return {bind(...params){
    const statement=db.prepare(sql)
    return {
     async first(){return statement.get(...params)||null},
     async run(){const result=statement.run(...params);return {meta:{changes:Number(result.changes)}}}
    }
   }}
  },
  async batch(statements){
   db.exec('BEGIN')
   try{
    const results=[]
    for(const stmt of statements)results.push(await stmt.run())
    db.exec('COMMIT')
    return results
   }catch(e){db.exec('ROLLBACK');throw e}
  }
 }
 const sent=[]
 const env={DB:adapter,EMAIL_FROM:'DOMINION <noreply@example.com>',EMAIL:{async send(msg){sent.push(msg)}}}
 const endpoint='https://dominion.example/v1/auth/email/'
 const post=(name,body)=>new Request(endpoint+name,{method:'POST',headers:{'content-type':'application/json'},body:JSON.stringify(body)})
 const begin=email=>startEmailLogin(post('start',{email}),env)
 return {db,env,sent,post,begin}
}

test('email identities require configured outbound sending and conservative syntax',()=>{
 assert.equal(emailLoginReady({DB:{},EMAIL_FROM:'a@b.test'}),false)
 assert.equal(emailLoginReady({DB:{},EMAIL_FROM:'a@b.test',EMAIL:{send(){}}}),true)
 assert.equal(emailAuthTest.cleanEmail(' USER+One@Example.COM '),'user+one@example.com')
 for(const input of ['user@@example.com','a b@example.com','user@example..com','x@localhost']){
  assert.equal(emailAuthTest.cleanEmail(input),null,input)
 }
})

test('mail link is not a bearer credential, GET is inert and verified poll is single-use',async()=>{
 const {db,env,sent,post,begin}=fixture()
 const start=await begin('Alice@example.com')
 assert.equal(start.status,200)
 const params=await start.json()
 assert.match(params.id,/^[A-Za-z0-9_-]{43}$/)
 assert.match(params.poll_token,/^[A-Za-z0-9_-]{43}$/)
 assert.equal(sent.length,1)
 assert.equal(sent[0].to[0],'alice@example.com')
 const link=new URL(sent[0].text.match(/https:\/\/\S+/)[0])
 assert.equal(link.pathname,'/v1/auth/email/verify')
 assert.notEqual(link.searchParams.get('token'),params.poll_token)
 const dbRow=db.prepare('SELECT * FROM email_login_attempts WHERE id=?').get(params.id)
 assert.equal(dbRow.status,'pending')
 assert.notEqual(dbRow.poll_hash,params.poll_token)
 assert.notEqual(dbRow.verify_hash,link.searchParams.get('token'))
 assert.equal((await verifyEmailLogin(new Request(link),env)).status,200)
 assert.equal(db.prepare('SELECT status FROM email_login_attempts WHERE id=?').get(params.id).status,'pending')
 const early=await pollEmailLogin(post('status',{id:params.id,poll_token:params.poll_token}),env)
 assert.deepEqual(await early.json(),{status:'waiting'})
 const wrong=await pollEmailLogin(post('status',{id:params.id,poll_token:'a'.repeat(43)}),env)
 assert.equal(wrong.status,404)
 const confirm=new Request(link.origin+link.pathname,{
  method:'POST',headers:{'content-type':'application/x-www-form-urlencoded'},
  body:new URLSearchParams({id:params.id,token:link.searchParams.get('token')})
 })
 assert.equal((await verifyEmailLogin(confirm,env)).status,200)
 const finished=await pollEmailLogin(post('status',{id:params.id,poll_token:params.poll_token}),env)
 assert.equal(finished.status,200)
 const session=await finished.json()
 assert.equal(session.status,'done')
 assert.match(session.token,/^[A-Za-z0-9_-]{43}$/)
 assert.equal(db.prepare('SELECT COUNT(*) AS n FROM accounts').get().n,1)
 assert.equal(db.prepare('SELECT COUNT(*) AS n FROM sessions').get().n,1)
 const replay=await pollEmailLogin(post('status',{id:params.id,poll_token:params.poll_token}),env)
 assert.equal(replay.status,409)
 assert.equal(db.prepare('SELECT COUNT(*) AS n FROM sessions').get().n,1)
 db.close()
})

test('email sender failures revoke the login attempt and never return poll credentials',async()=>{
 const {db,env,begin}=fixture()
 env.EMAIL.send=async()=>{throw Error('mock sender down')}
 const result=await begin('demo@example.com')
 assert.equal(result.status,502)
 assert.equal(db.prepare('SELECT COUNT(*) AS n FROM email_login_attempts').get().n,0)
 db.close()
})

test('email request throttle applies before dispatching a sixth email',async()=>{
 const {db,sent,begin}=fixture()
 for(let i=0;i<5;i++)assert.equal((await begin('demo@example.com')).status,200)
 assert.equal((await begin('demo@example.com')).status,429)
 assert.equal(sent.length,5)
 db.close()
})

test('Worker allows its own email confirmation form but rejects foreign-origin posts',async()=>{
 const {db,env,sent,begin}=fixture()
 env.ALLOWED_ORIGIN='http://127.0.0.1:4317'
 const data=await (await begin('demo@example.com')).json()
 const link=new URL(sent[0].text.match(/https:\/\/\S+/)[0])
 const body=new URLSearchParams({id:data.id,token:link.searchParams.get('token')})
 const own=await worker.fetch(new Request(link.origin+link.pathname,{
  method:'POST',headers:{origin:link.origin,'content-type':'application/x-www-form-urlencoded'},body
 }),env)
 assert.equal(own.status,200)
 assert.equal(db.prepare('SELECT status FROM email_login_attempts WHERE id=?').get(data.id).status,'verified')
 const malicious=await worker.fetch(new Request(link.origin+link.pathname,{
  method:'POST',headers:{origin:'https://malicious.example','content-type':'application/x-www-form-urlencoded'},body
 }),env)
 assert.equal(malicious.status,403)
 db.close()
})

test('expired confirmation cannot issue a session',async()=>{
 const {db,env,sent,post,begin}=fixture()
 const data=await (await begin('demo@example.com')).json()
 const link=new URL(sent[0].text.match(/https:\/\/\S+/)[0])
 db.prepare('UPDATE email_login_attempts SET expires_at=0 WHERE id=?').run(data.id)
 assert.equal((await verifyEmailLogin(new Request(link),env)).status,200)
 assert.equal((await pollEmailLogin(post('status',{id:data.id,poll_token:data.poll_token}),env)).status,410)
 assert.equal(db.prepare('SELECT COUNT(*) AS n FROM sessions').get().n,0)
 db.close()
})
