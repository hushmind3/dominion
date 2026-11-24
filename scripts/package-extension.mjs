#!/usr/bin/env node
// Produces a portable, self-contained DOMINION package. No build/install
// hooks and no network access; the SDK/runtime validates the manifest.
import {readFile,writeFile,stat} from 'node:fs/promises';
import {createHash} from 'node:crypto';
const [manifestPath,artifactPath,outPath]=process.argv.slice(2);
if(!manifestPath||!artifactPath||!outPath){console.error('Usage: node scripts/package-extension.mjs manifest.json executable output.dominion.json');process.exit(2);}
const MAX=8*1024*1024;
if((await stat(manifestPath)).size>1024*1024)throw Error('manifest too large');
if((await stat(artifactPath)).size>MAX)throw Error('artifact exceeds 8 MiB local bundle limit');
const manifest=JSON.parse(await readFile(manifestPath,'utf8'));
const artifact=await readFile(artifactPath);
if(!artifact.length||artifact.length>MAX||manifest.sdkVersion!==1||!manifest.id||!Array.isArray(manifest.tools))throw Error('invalid manifest or artifact');
const digest=createHash('sha256').update(artifact).digest('hex');
if(manifest.artifact?.sha256!==digest)throw Error('SHA-256 mismatch; expected '+digest);
const bundle={bundleVersion:1,manifest,artifactBase64:artifact.toString('base64')};
const data=JSON.stringify(bundle);
if(Buffer.byteLength(data)>12*1024*1024)throw Error('bundle exceeds 12 MiB JSON limit');
await writeFile(outPath,data,{flag:'wx',mode:0o600});
console.log('Verified package: '+outPath+' ('+artifact.length+' bytes)');
