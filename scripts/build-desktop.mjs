// Native desktop build, on macOS or Windows. No Node files are bundled.
import {spawnSync} from 'node:child_process';
import {existsSync,readFileSync,readdirSync} from 'node:fs';
import {packageDmg} from './package-dmg.mjs';
import {fileURLToPath} from 'node:url';
import path from 'node:path';
const root=path.resolve(path.dirname(fileURLToPath(import.meta.url)),'..');
const args=process.argv.slice(2);
function run(command,argv,env=process.env){const result=spawnSync(command,argv,{cwd:root,env,stdio:'inherit',shell:false});if(result.error)throw result.error;if(result.status!==0)process.exit(result.status||1)}
const prepare=[];for(const key of ['--target']){const i=args.indexOf(key);if(i>=0)prepare.push(key,args[i+1]);}
for(const key of ['--skip-web','--skip-engine'])if(args.includes(key))prepare.push(key);
run(process.execPath,[path.join(root,'scripts','prepare-desktop.mjs'),...prepare]);
const cli=path.join(root,'web','node_modules','@tauri-apps','cli','tauri.js');
if(!existsSync(cli))throw Error('Install web build tools first: npm ci --prefix web');
const command=args.includes('--dev')?'dev':'build';
const cliArgs=[command,'--config',path.join(root,'src-tauri','tauri.conf.json')];
for(const key of ['--target','--bundles']){const i=args.indexOf(key);if(i>=0)cliArgs.push(key,args[i+1]);}
// On CI, build the app with Tauri and create its DMG without mounting it.
const bundlesIndex=args.indexOf('--bundles');
const requestedBundles=bundlesIndex<0?['app','dmg']:args[bundlesIndex+1].split(',');
const ciDmg=process.platform==='darwin' && process.env.CI==='true' && command==='build' && requestedBundles.some(bundle=>bundle==='dmg'||bundle==='all');
if(ciDmg){
  const index=cliArgs.indexOf('--bundles');
  if(index>=0)cliArgs.splice(index,2);
  cliArgs.push('--bundles','app');
}
if(args.includes('--debug') && command==='build')cliArgs.push('--debug');
run(process.execPath,[cli,...cliArgs],{...process.env,CARGO_TARGET_DIR:path.join(root,'src-tauri','target')});

if(ciDmg){
  const targetIndex=args.indexOf('--target');
  const target=targetIndex<0?'':args[targetIndex+1];
  const profile=args.includes('--debug')?'debug':'release';
  const bundleDir=path.join(root,'src-tauri','target',target,profile,'bundle');
  const apps=readdirSync(path.join(bundleDir,'macos')).filter(name=>name.endsWith('.app'));
  if(apps.length!==1)throw Error(`Expected one macOS app bundle, found ${apps.length}`);
  const config=JSON.parse(readFileSync(path.join(root,'src-tauri','tauri.conf.json'),'utf8'));
  const architecture=target.startsWith('x86_64-')?'x64':target.startsWith('aarch64-')?'aarch64':process.arch;
  packageDmg(path.join(bundleDir,'macos',apps[0]),path.join(bundleDir,'dmg',`${config.productName}_${config.version}_${architecture}.dmg`),config.productName);
}
