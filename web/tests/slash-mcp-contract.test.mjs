import test from 'node:test'
import assert from 'node:assert/strict'
import {readFileSync} from 'node:fs'
import {fileURLToPath} from 'node:url'
import {dirname,resolve} from 'node:path'
import {parseMcpQuickCommand,validRemoteMcpUrl,findInstalledMcp,discoveryForMcp,registryLaunch} from '../src/features/mcp/quickCommands.ts'

const dir=dirname(fileURLToPath(import.meta.url))
const chat=readFileSync(resolve(dir,'../src/pages/Chat.tsx'),'utf8')
const component=readFileSync(resolve(dir,'../src/components/chat/McpQuickConnect.tsx'),'utf8')
const menu=readFileSync(resolve(dir,'../src/components/SlashMenu.tsx'),'utf8')
const catalog=JSON.parse(readFileSync(resolve(dir,'../shared/commands.json'),'utf8'))

test('MCP quick commands parse remote registration, search and installed actions',()=>{
 assert.deepEqual(parseMcpQuickCommand(''),{kind:'open'})
 assert.deepEqual(parseMcpQuickCommand('list'),{kind:'list'})
 assert.deepEqual(parseMcpQuickCommand('search Parallel MCP 2.0'),{kind:'search',query:'Parallel MCP 2.0'})
 assert.deepEqual(parseMcpQuickCommand('add Parallel MCP 2.0 https://mcp.parallel.ai/mcp'),{kind:'add',name:'Parallel MCP 2.0',url:'https://mcp.parallel.ai/mcp'})
 assert.deepEqual(parseMcpQuickCommand('disable Parallel MCP 2.0'),{kind:'disable',name:'Parallel MCP 2.0'})
 assert.deepEqual(parseMcpQuickCommand('enable Parallel MCP 2.0'),{kind:'enable',name:'Parallel MCP 2.0'})
 assert.deepEqual(parseMcpQuickCommand('remove Parallel MCP 2.0'),{kind:'remove',name:'Parallel MCP 2.0'})
 assert.equal(parseMcpQuickCommand('add only-a-name').kind,'invalid')
 assert.equal(parseMcpQuickCommand('search').kind,'invalid')
 assert.equal(parseMcpQuickCommand('enable').kind,'invalid')
 assert.equal(parseMcpQuickCommand('list unused').kind,'invalid')
 assert.equal(parseMcpQuickCommand('run echo hello').kind,'invalid')
})
test('remote MCP registration excludes plaintext and credential-bearing URLs',()=>{
 for(const url of ['https://mcp.parallel.ai/mcp','https://search.example.org/mcp?v=2026'])
  assert.equal(validRemoteMcpUrl(url),true,url)
 for(const url of ['http://mcp.parallel.ai/mcp','https://user:pass@example.org/mcp','https://example.org/mcp#secret','file:///etc/passwd','not-a-url'])
  assert.equal(validRemoteMcpUrl(url),false,url)
})
test('MCP identity matches exact registration without substring ambiguity',()=>{
 const rows=[
  {id:'parallel',name:'Parallel MCP',transport:'http',url:'https://mcp.parallel.ai/mcp'},
  {id:'search-2',name:'Parallel MCP 2.0',transport:'http',url:'https://other.example/mcp'}
 ]
 assert.equal(findInstalledMcp(rows,'Parallel MCP 2.0')?.id,'search-2')
 assert.equal(findInstalledMcp(rows,'PARALLEL MCP')?.id,'parallel')
 assert.equal(findInstalledMcp(rows,'search-2')?.id,'search-2')
 assert.equal(findInstalledMcp(rows,'Parallel')?.id,undefined)
})
test('ambiguous MCP names and ID/name collisions never pick a destructive target',()=>{
 const rows=[
  {id:'alpha-id',name:'Shared',transport:'http',url:'https://a.example/mcp'},
  {id:'beta-id',name:'shared',transport:'http',url:'https://b.example/mcp'},
  {id:'shared-id',name:'Unique',transport:'http',url:'https://c.example/mcp'},
  {id:'unique-id',name:'shared-id',transport:'http',url:'https://d.example/mcp'},
 ]
 assert.equal(findInstalledMcp(rows,'shared'),undefined)
 assert.equal(findInstalledMcp(rows,'shared-id'),undefined)
 assert.equal(findInstalledMcp(rows,'Unique')?.id,'shared-id')
 assert.equal(findInstalledMcp(rows,'beta-id')?.id,'beta-id')
})

test('MCP discovery correlates each healthy or broken server by stable installed ID',()=>{
 const rows=[
  {mcpId:'healthy',name:'mcp__healthy__search'},
  {mcpId:'other',name:'mcp__other__write'},
  {mcpId:'broken',name:'Broken',error:'unauthorized'},
  {mcpId:'healthy',name:''},
 ]
 assert.deepEqual(discoveryForMcp(rows,'healthy').tools.map(x=>x.name),['mcp__healthy__search'])
 assert.equal(discoveryForMcp(rows,'broken').error,'unauthorized')
 assert.equal(discoveryForMcp(rows,'other').tools.length,1)
 assert.equal(discoveryForMcp(rows,'missing').error,undefined)
})

test('MCP slash palette has actual actionable commands and backend calls',()=>{
 assert.ok(catalog.some(item=>item.key==='mcp'))
 assert.match(menu,/key:'mcp:search'/)
 assert.match(menu,/key:'mcp:add'/)
 assert.match(menu,/key:'mcp:disable'/)
 assert.match(chat,/const executeMcpCommand=async\(input:string\)=>/)
 assert.match(chat,/await get<RegistryMcp\[\]>\('\/api\/mcp\/catalog\?q='/)
 assert.match(chat,/await post<InstalledMcp>\('\/api\/mcp',payload\)/)
 assert.match(chat,/await get<McpAvailableTool\[\]>\('\/api\/mcp\/discover'\)/)
 assert.match(chat,/await del\('\/api\/mcp\/'\+encodeURIComponent/)
 assert.match(chat,/await post\('\/api\/mcp',\{\.\.\.record,enabled\}\)/)
 assert.match(chat,/if\(name==='mcp'\)\{await executeMcpCommand\(arg\);return;\}/)
 assert.match(component,/role="region" aria-label="MCP 빠른 연결"/)
 assert.match(component,/onClick=\{\(\)=>onToggle\(entry,entry.enabled===false\)\}/)
})
test('every advertised slash command dispatches to an implemented backend operation or route',()=>{
 const statement=chat.slice(chat.indexOf(' const executeCommand=async(text:string)=>{'),chat.indexOf(' const chooseSlash='))
 const single=[...statement.matchAll(/name==='([a-z]+)'/g)].map(m=>m[1])
 const arrays=[...statement.matchAll(/\[([^\]]+)\]\.includes\(name\)/g)].flatMap(m=>[...m[1].matchAll(/'([a-z]+)'/g)].map(z=>z[1]))
 const dispatched=new Set([...single,...arrays])
 const missing=catalog.map(item=>item.key).filter(key=>!dispatched.has(key))
 assert.deepEqual(missing,[],'slash entries without an execution branch')
 assert.match(statement,/if\(name==='goal'\)\{/)
 assert.match(statement,/await post<\{id:string\}>\('\/api\/jobs',\{kind:'prompt'/)
 assert.match(statement,/if\(name==='workflows'\)\{window.location.hash='#\/tasks'/)
})

 test('registry launches only explicit supported versions and prefers a valid remote',()=>{
 const npm={name:'example',packageRegistry:'npm',packageName:'@scope/server',packageVersion:'1.2.3'}
 assert.deepEqual(registryLaunch(npm),{transport:'stdio',command:'npx',args:['-y','@scope/server@1.2.3']})
 assert.deepEqual(registryLaunch({name:'python',packageRegistry:'pypi',packageName:'mcp-server',packageVersion:'2.1.0'}),{transport:'stdio',command:'uvx',args:['mcp-server==2.1.0']})
 for(const bad of ['latest','1','^1.2.3','1.2.3;echo',''])assert.equal(registryLaunch({...npm,packageVersion:bad}),undefined)
 assert.equal(registryLaunch({...npm,packageName:'--evil'}),undefined)
 assert.equal(registryLaunch({name:'legacy',packageName:'unversioned'}),undefined)
 assert.deepEqual(registryLaunch({...npm,url:'https://example.org/mcp'}),{transport:'http',url:'https://example.org/mcp'})
 assert.equal(registryLaunch({name:'invalid',url:'http://example.org'}),undefined)
 })
