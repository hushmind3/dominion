import path from 'node:path';
import { DatabaseSync } from 'node:sqlite';
import { EventEmitter } from 'node:events';
import { DATA, uid } from './config.mjs';
export const db = new DatabaseSync(path.join(DATA,'dominion.sqlite'));
db.exec("PRAGMA busy_timeout=5000;");
db.exec("PRAGMA journal_mode=WAL; PRAGMA foreign_keys=ON;");
db.exec("CREATE TABLE IF NOT EXISTS records (kind TEXT NOT NULL,id TEXT NOT NULL,data TEXT NOT NULL,created_at TEXT NOT NULL DEFAULT (datetime('now')),updated_at TEXT NOT NULL DEFAULT (datetime('now')),PRIMARY KEY(kind,id));");
db.exec("CREATE TABLE IF NOT EXISTS messages (id TEXT PRIMARY KEY,session_id TEXT NOT NULL,role TEXT NOT NULL,content TEXT NOT NULL DEFAULT '',reasoning TEXT NOT NULL DEFAULT '',agent_id TEXT,tool_calls TEXT,tool_call_id TEXT,created_at TEXT NOT NULL DEFAULT (datetime('now')));");
db.exec("CREATE INDEX IF NOT EXISTS idx_messages_session ON messages(session_id,created_at,rowid);".replace(',rowid',''));
db.exec("CREATE TABLE IF NOT EXISTS audit (id TEXT PRIMARY KEY,event TEXT NOT NULL,detail TEXT NOT NULL,created_at TEXT NOT NULL DEFAULT (datetime('now')));");
export const bus = new EventEmitter(); bus.setMaxListeners(200);
const parse = row => row ? {...JSON.parse(row.data),id:row.id,createdAt:row.created_at,updatedAt:row.updated_at} : null;
export function all(kind) { return db.prepare("SELECT * FROM records WHERE kind=? ORDER BY updated_at DESC").all(kind).map(parse); }
export function get(kind,id) { return parse(db.prepare("SELECT * FROM records WHERE kind=? AND id=?").get(kind,id)); }
export function put(kind,value) {
  if (!value || typeof value!=='object') throw Error('Expected record object');
  const id=String(value.id||uid());
  const data=JSON.stringify({...value,id});
  db.prepare("INSERT INTO records(kind,id,data) VALUES(?,?,?) ON CONFLICT(kind,id) DO UPDATE SET data=excluded.data,updated_at=datetime('now')").run(kind,id,data);
  const r=get(kind,id); bus.emit('record',{kind,record:r}); return r;
}
export function remove(kind,id){ db.prepare("DELETE FROM records WHERE kind=? AND id=?").run(kind,id); bus.emit('record',{kind,removed:id});}
export function writeMessage(value){
  const v={id:value.id||uid(),session_id:String(value.session_id),role:String(value.role),content:String(value.content||''),reasoning:String(value.reasoning||''),agent_id:value.agent_id||null,tool_calls:value.tool_calls||null,tool_call_id:value.tool_call_id||null};
  db.prepare("INSERT INTO messages(id,session_id,role,content,reasoning,agent_id,tool_calls,tool_call_id) VALUES(?,?,?,?,?,?,?,?)").run(v.id,v.session_id,v.role,v.content,v.reasoning,v.agent_id,v.tool_calls?JSON.stringify(v.tool_calls):null,v.tool_call_id);
  bus.emit('message',v);return v;
}
export function messages(sessionId){return db.prepare("SELECT * FROM messages WHERE session_id=? ORDER BY created_at ASC,rowid ASC").all(sessionId).map(x=>({...x,tool_calls:x.tool_calls?JSON.parse(x.tool_calls):null}));}
export function log(event,detail={}) { const rec={id:uid(),event,detail,createdAt:new Date().toISOString()}; db.prepare("INSERT INTO audit(id,event,detail) VALUES(?,?,?)").run(rec.id,rec.event,JSON.stringify(detail));bus.emit('audit',rec);return rec;}
export function recentAudit(limit=100){return db.prepare('SELECT * FROM audit ORDER BY rowid DESC LIMIT ?').all(Math.min(limit,500)).map(x=>({...x,detail:JSON.parse(x.detail)}));}
