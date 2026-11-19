import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';
import {spawnSync} from 'node:child_process';

// Real SQLite exercises the same trigger and transaction semantics used by D1.
test('paid reservation ledger is atomic under budget, balance and replay conflicts',()=>{
  const python=String.raw\`
import sqlite3,sys
db=sqlite3.connect(':memory:')
db.executescript(sys.stdin.read())
db.executemany('INSERT INTO accounts(id,email,credits,created_at) VALUES(?,?,?,?)',[
 ('acct-a','',1000,1),('acct-b','',1000,1)
])
t=1780000000
def reserve(id,account,amount,cap,when=t):
 db.execute("INSERT INTO usage(id,account_id,model,reserved,budget_cap,status,created_at) VALUES(?,?,?,?,?,'reserved',?)",
            (id,account,'model-a',amount,cap,when))
def balance(account):
 return db.execute('SELECT credits FROM accounts WHERE id=?',(account,)).fetchone()[0]
reserve('first','acct-a',300,500)
assert balance('acct-a')==700
for id,account,amount,cap,reason in [
 ('over-budget','acct-b',250,500,'paid_daily_budget_reached'),
 ('over-balance','acct-a',900,3000,'insufficient_prepaid_credits'),
 ('first','acct-b',100,500,'UNIQUE constraint failed')
]:
 before=(balance('acct-a'),balance('acct-b'),db.execute('SELECT COUNT(*) FROM usage').fetchone()[0])
 try: reserve(id,account,amount,cap)
 except sqlite3.IntegrityError as exc:
  assert reason in str(exc),(reason,str(exc))
 else: raise AssertionError('invalid reservation succeeded: '+id)
 after=(balance('acct-a'),balance('acct-b'),db.execute('SELECT COUNT(*) FROM usage').fetchone()[0])
 assert before==after,(id,before,after)
# Once a reservation settles, only the true charge counts toward daily spend.
with db:
 db.execute('UPDATE accounts SET credits=credits+180 WHERE id=?',('acct-a',))
 db.execute("UPDATE usage SET charged=120,status='completed' WHERE id='first'")
reserve('second','acct-b',250,500)
assert balance('acct-a')==880 and balance('acct-b')==750
# New UTC day starts a separate spend budget without resetting account credits.
reserve('next-day','acct-a',400,500,t+86400)
assert balance('acct-a')==480
print('paid ledger: atomic debit, budget, replay, settlement and day rollover OK')
\`;
  const result=spawnSync('python3',['-c',python],{
    input:readFileSync(new URL('../schema.sql',import.meta.url),'utf8'),
    encoding:'utf8',
  });
  assert.equal(result.status,0,result.stderr||result.stdout||result.error?.message);
});
