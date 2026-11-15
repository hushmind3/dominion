// Check the resolved build graph rather than a runtime label.
import {execFileSync} from 'node:child_process'
import {existsSync, readFileSync} from 'node:fs'
import path from 'node:path'
import {fileURLToPath} from 'node:url'

const root = path.resolve(path.dirname(fileURLToPath(import.meta.url)), '..')
const metadata = JSON.parse(execFileSync('cargo', [
  'metadata', '--format-version', '1', '--no-deps', '--locked',
], {cwd: root, encoding: 'utf8'}))
const members = metadata.packages.filter(pkg => metadata.workspace_members.includes(pkg.id))
const product = members.find(pkg => pkg.name === 'dominion-rust')
const core = members.find(pkg => pkg.name === 'dominion-core')
if (!product || !core) throw new Error('Product and native core must both be workspace members')
const retired = new Set(['dominion-harness', 'dominion-agent-sdk'])
if (members.some(pkg => retired.has(pkg.name))) throw new Error('Retired execution package in workspace')
if (product.dependencies.some(dep => retired.has(dep.name))) throw new Error('Product depends on a retired runtime')
if (!product.dependencies.some(dep => dep.name === core.name && dep.path === path.dirname(core.manifest_path))) {
  throw new Error('Product must embed the workspace native core directly')
}
for (const pkg of members) {
  if (!existsSync(pkg.manifest_path)) throw new Error(`Missing workspace manifest: ${pkg.manifest_path}`)
}
const source = readFileSync(path.join(root, 'src/engine/rsi_system.rs'), 'utf8')
const map = source.match(/\("harness",\s*&\[([^\]]+)\]\)/s)?.[1]
if (!map) throw new Error('RSI harness source map is missing')
const sources = [...map.matchAll(/"([^"]+)"/g)].map(match => match[1])
if (!sources.some(file => file.startsWith('crates/dominion-core/src/'))) {
  throw new Error('RSI is not inspecting the native execution core')
}
for (const file of sources) {
  if (!existsSync(path.join(root, file))) throw new Error(`Missing RSI runtime source: ${file}`)
}
console.log(`Runtime architecture OK: ${members.map(pkg => pkg.name).join(', ')}; ${sources.length} live RSI harness sources`)
