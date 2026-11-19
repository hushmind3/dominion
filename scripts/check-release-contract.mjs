// Fail the release before consuming native build runners when package locks
// and desktop/app manifests disagree about the source revision.
import {readFileSync} from 'node:fs';
import path from 'node:path';
import {fileURLToPath} from 'node:url';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const read=p=>readFileSync(path.join(root,p),'utf8');
const asJson=p=>JSON.parse(read(p));
const match=(p,kind)=>{
  const source=read(p);
  const pattern=new RegExp('\\[\\[package\\]\\]\\s*name = "'+kind+'"\\s*version = "([^"]+)"');
  return source.match(pattern)?.[1];
};
const cargo=read('Cargo.toml').match(/name = "dominion-rust"\s+version = "([^"]+)"/)?.[1];
const version=asJson('src-tauri/tauri.conf.json').version;
const others={
  'Cargo.toml':cargo,
  'Cargo.lock':match('Cargo.lock','dominion-rust'),
  'src-tauri/Cargo.toml':read('src-tauri/Cargo.toml').match(/name = "dominion-desktop"\s+version = "([^"]+)"/)?.[1],
  'src-tauri/Cargo.lock':match('src-tauri/Cargo.lock','dominion-desktop'),
  'web/package.json':asJson('web/package.json').version,
  'web/package-lock.json':asJson('web/package-lock.json').version,
  'web/package-lock.json root package':asJson('web/package-lock.json').packages[''].version
};
const errors=Object.entries(others).filter(([,value])=>value!==version)
  .map(([file,value])=>file+': '+String(value)+' != '+version);
const tauri=asJson('src-tauri/tauri.conf.json');
if(!tauri.bundle.externalBin?.includes('binaries/dominion-engine'))errors.push('Native engine sidecar missing from Tauri bundle');
if(!tauri.bundle.resources?.['resources/web/'])errors.push('Production React bundle missing from Tauri resources');
if(errors.length){console.error(errors.join('\n'));process.exitCode=1;}
else console.log('DOMINION '+version+': Rust, Tauri and npm package locks agree');
