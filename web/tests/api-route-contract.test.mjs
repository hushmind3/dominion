import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync,readdirSync} from 'node:fs'
import {join,relative} from 'node:path'
import {fileURLToPath} from 'node:url'
import ts from 'typescript'

const root=fileURLToPath(new URL('../../',import.meta.url))
const read=p=>readFileSync(p,'utf8')
function files(dir,pattern){
 return readdirSync(dir,{withFileTypes:true}).flatMap(e=>{
  const p=join(dir,e.name)
  return e.isDirectory()?files(p,pattern):pattern.test(e.name)?[p]:[]
 })
}
const routes=[]
for(const p of files(join(root,'src/api/routes'),/\.rs$/)){
 const s=read(p)
 const matches=[...s.matchAll(/\.route\(\s*"([^"]+)"\s*,/g)]
 for(let i=0;i<matches.length;i++){
  const m=matches[i]
  const body=s.slice(m.index+m[0].length,matches[i+1]?.index??s.length)
  for(const [,method] of body.matchAll(/\b(get|post|delete|put|patch|any)\s*\(/g))
   routes.push({path:m[1],method:method.toUpperCase()})
 }
}
const collections=read(join(root,'src/api/collections.rs')).match(/pub const COLLECTIONS:[^=]+=\s*&\[([\s\S]*?)\];/)
assert.ok(collections,'Rust collection registry not found')
for(const [,kind] of collections[1].matchAll(/"([^"]+)"/g)){
 for(const method of ['GET','POST'])routes.push({path:'/api/'+kind,method})
 for(const method of ['GET','POST','DELETE'])routes.push({path:'/api/'+kind+'/:id',method})
 routes.push({path:'/api/'+kind+'/:id/:action',method:'POST'})
}
const DYNAMIC='__DYNAMIC__'
function resolve(node,bindings,seen=new Set()){
 if(!node||seen.has(node))return [DYNAMIC]
 const next=new Set(seen).add(node)
 if(ts.isStringLiteralLike(node))return [node.text]
 if(node.kind===ts.SyntaxKind.NullKeyword)return []
 if(ts.isParenthesizedExpression(node)||ts.isAsExpression(node)||ts.isNonNullExpression(node))return resolve(node.expression,bindings,next)
 if(ts.isIdentifier(node)&&bindings.has(node.text))return resolve(bindings.get(node.text),bindings,next)
 if(ts.isConditionalExpression(node))return [...resolve(node.whenTrue,bindings,next),...resolve(node.whenFalse,bindings,next)]
 if(ts.isTemplateExpression(node))return [node.head.text+node.templateSpans.map(span=>DYNAMIC+span.literal.text).join('')]
 if(ts.isBinaryExpression(node)&&node.operatorToken.kind===ts.SyntaxKind.PlusToken){
  const left=resolve(node.left,bindings,next),right=resolve(node.right,bindings,next)
  return left.flatMap(a=>right.map(b=>a+b)).slice(0,64)
 }
 return [DYNAMIC]
}
function methodFor(node,name){
 if(name==='get'||name==='useRemote'||name==='EventSource')return 'GET'
 if(name==='post')return 'POST'
 if(name==='del')return 'DELETE'
 const opts=node.arguments?.[1]
 if(!opts)return 'GET'
 if(!ts.isObjectLiteralExpression(opts))return null
 const entry=opts.properties.find(p=>ts.isPropertyAssignment(p)&&(p.name.getText()==='method'||p.name.getText()==='"method"'||p.name.getText()==="'method'"))
 return entry?ts.isStringLiteralLike(entry.initializer)?entry.initializer.text.toUpperCase():null:'GET'
}
function pathMatches(pattern,actual){
 const a=actual.split('/'),b=pattern.split('/')
 return a.length===b.length&&a.every((segment,i)=>b[i].startsWith(':')||segment.includes(DYNAMIC)||segment===b[i])
}
test('frontend API calls resolve against Rust router and collection methods',t=>{
 const src=files(join(root,'web/src'),/\.tsx?$/)
 const calls=new Set(),failures=[],unresolved=[]
 for(const p of src){
  const source=ts.createSourceFile(p,read(p),ts.ScriptTarget.Latest,true,p.endsWith('.tsx')?ts.ScriptKind.TSX:ts.ScriptKind.TS)
  const bindings=new Map(),duplicate=new Set()
  function bind(n){
   if(ts.isVariableDeclaration(n)&&ts.isIdentifier(n.name)&&n.initializer){
    if(bindings.has(n.name.text))duplicate.add(n.name.text)
    else bindings.set(n.name.text,n.initializer)
   }
   ts.forEachChild(n,bind)
  }
  bind(source)
  for(const name of duplicate)bindings.delete(name)
  function visit(n){
   if((ts.isCallExpression(n)||ts.isNewExpression(n))&&ts.isIdentifier(n.expression)){
    const name=n.expression.text
    if(['get','post','del','request','apiFetch','fetch','useRemote','EventSource'].includes(name)){
     const line=source.getLineAndCharacterOfPosition(n.getStart(source)).line+1
     const location=relative(root,p)+':'+line
     const shapes=resolve(n.arguments?.[0],bindings)
     const method=methodFor(n,name)
     for(const shape of shapes){
      const path=shape.split('?')[0].split('#')[0]
      if(!path.startsWith('/api/')){
       if(shape===DYNAMIC)unresolved.push(location+' '+name)
       continue
      }
      const matching=routes.filter(r=>pathMatches(r.path,path))
      // Unknown computed suffixes may contain multiple path segments. Do not
      // silently certify them; report as unresolved, not as a concrete route.
      if(path.endsWith(DYNAMIC)&&(!matching.length||path==='/api/'+DYNAMIC)){
       unresolved.push(location+' '+path)
       continue
      }
      const label=location+' '+(method||'dynamic method')+' '+path
      if(!method)unresolved.push(label)
      if(!matching.some(r=>!method||r.method===method||r.method==='ANY'))failures.push(label)
      else calls.add(label)
     }
    }
   }
   ts.forEachChild(n,visit)
  }
  visit(source)
 }
 t.diagnostic('Scanned '+src.length+' frontend source files; '+routes.length+' Rust route/method pairs; '+calls.size+' matched call sites; '+unresolved.length+' unresolved dynamic call expressions/methods.')
 if(unresolved.length)t.diagnostic('Dynamic cases: '+unresolved.join('; '))
 assert.ok(calls.size>30,'route scanner must exercise real product calls')
 assert.deepEqual(failures,[],'frontend calls without a matching Rust route/method')
})
