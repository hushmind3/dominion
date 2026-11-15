// Native source/module hygiene gate. Checks REAL tracked files, not a whitelist
// of warnings or a fake success status. Run before expensive Cargo tests.
import {existsSync, readFileSync, statSync} from 'node:fs'
import {execFileSync} from 'node:child_process'
import path from 'node:path'
import {fileURLToPath} from 'node:url'

export const RETIRED_ENTRYPOINTS = [
  path.posix.join('src','engine','goals.rs'),
  path.posix.join('src','api','goals.rs'),
]

// A split module cannot silently grow back into its old monolith.
// Remaining submodules may evolve; this targets only the public entry shells.
const MAX_ENTRY_BYTES = new Map([
  ['src/engine/goals/mod.rs', 4096],
  ['src/api/goals/mod.rs', 4096],
  ['src/api/providers.rs', 45000],
])

const SCANNED = /^(?:src|scripts|tests|sdk|web|\.github)\//
const TRASH = /(?:^|\/)(?:node_modules|target|dist|__pycache__)(?:\/|$)|\.(?:bak|orig|rej|tmp|swp|swo|pyc)$/
const CODE = /\.(?:rs|mjs|cjs|js|jsx|ts|tsx|py|sh|yaml|yml)$/
const STATIC_REPO_READ = /\b(?:source|file|check)\s*\(\s*(['"])((?:src|scripts|sdk|web|tests|\.github)\/[^'"\r\n]+)\1/g
const RUST_PATH_MODULE = /#\[\s*path\s*=\s*"([^"]+)"\s*\]\s*(?:#\[[^\]]+\]\s*)*(?:(?:pub(?:\([^)]*\))?)\s+)?mod\s+[A-Za-z_]\w*\s*;/g

export function auditSourceHygiene(root, suppliedPaths) {
  const listed = suppliedPaths ?? execFileSync('git',['ls-files','-z'], {cwd:root})
    .toString('utf8').split('\0').filter(Boolean)
  const files = listed.map(p=>p.replaceAll('\\','/'))
    .filter(p=>existsSync(path.join(root,p)))
  const names = new Set(files)
  const violations = []
  const push = (p,detail)=>violations.push(p+': '+detail)

  for (const file of files) {
    if (TRASH.test(file)) push(file,'tracked build or abandoned backup artifact')
  }
  for (const retired of RETIRED_ENTRYPOINTS) {
    if (names.has(retired)) push(retired,'obsolete goals.rs restored; use goals/mod.rs')
  }
  for (const file of files.filter(p=>p.endsWith('.rs'))) {
    const shadow=file.slice(0,-3)+'/mod.rs'
    if (names.has(shadow)) push(file,'duplicate Rust module entrypoint also exists: '+shadow)
  }
  for (const [entry,ceiling] of MAX_ENTRY_BYTES) {
    if (!names.has(entry)) {
      push(entry,'missing public module entrypoint')
    } else if (statSync(path.join(root,entry)).size>ceiling) {
      push(entry,'entrypoint exceeds '+ceiling+' bytes; move implementation into submodules')
    }
  }

  for (const file of files) {
    if (!SCANNED.test(file)||!CODE.test(file)) continue
    // Exclude the scanner and its fixture tests from literal bans, so the
    // guard can declare what it forbids without flagging itself.
    const isHygieneTest=file.endsWith('/source-hygiene.test.mjs')
    const isHygieneSelf=file.endsWith('/source-hygiene.mjs')
    const body=readFileSync(path.join(root,file),'utf8')
    if (!isHygieneTest && !isHygieneSelf) {
      // Old SDK mocks once covered an HTTP endpoint the Rust server never
      // provided. Do not let a route rename revive that stale contract.
      if ((file.startsWith('sdk/')||file.startsWith('web/')) &&
          (/\/api\/goals\/[^\s"'\x60]*\/plan|\["goals",\s*id,\s*"plan"\]/.test(body)
            || (body.includes('"/api/goals/"')&&body.includes('"/plan"')))) {
        push(file,'SDK still targets deleted /api/goals/:id/plan; use native /steer');
      }
      for (const retired of RETIRED_ENTRYPOINTS) {
        if (body.includes(retired)) push(file,'stale reference to deleted source: '+retired)
      }
    }
    if (isHygieneTest||isHygieneSelf) continue
    if (file.endsWith('.rs')) {
      for (const hit of body.matchAll(RUST_PATH_MODULE)) {
        const modulePath=path.posix.normalize(path.posix.join(path.posix.dirname(file),hit[1]))
        if (!names.has(modulePath)) push(file,'#[path] module target missing: '+modulePath)
      }
    }
    if (/\.(?:mjs|cjs|js|ts|tsx)$/.test(file)) {
      for (const hit of body.matchAll(STATIC_REPO_READ)) {
        if (!names.has(hit[2])) push(file,'static repository source loader targets missing file: '+hit[2])
      }
    }
  }
  return violations
}

if (process.argv[1] && path.resolve(process.argv[1])===fileURLToPath(import.meta.url)) {
  const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..')
  const violations=auditSourceHygiene(root)
  if (violations.length) {
    console.error('DOMINION source hygiene gate failed ('+violations.length+'):\n'+violations.join('\n'))
    process.exitCode=1
  } else {
    console.log('DOMINION source hygiene: no shadow Rust modules, retired goals paths, broken static source loaders or oversized goals entrypoints')
  }
}
