'use strict';
const {app,BrowserWindow,shell,dialog,session,ipcMain} = require('electron');
const {spawn} = require('node:child_process');
const fs = require('node:fs');
const path = require('node:path');
const net = require('node:net');
const ROOT=path.resolve(__dirname,'..');
const DEV_URL=process.env.DOMINION_DEV_URL||'';
const SMOKE=process.env.DOMINION_SMOKE==='1'||process.argv.includes('--smoke');
const SMOKE_OUT=process.env.DOMINION_SMOKE_OUT||'';
try{
 app.setName('dominion-desktop');
 app.setPath('userData',path.join(app.getPath('appData'),'dominion-desktop'));
}catch{}
let window=null,engine=null,quitting=false,enginePort=0;
if(!app.requestSingleInstanceLock()){app.quit();}
else{
 app.on('second-instance',()=>{window?.restore();window?.focus();});
 app.whenReady().then(start).catch(err=>{
  const message=String(err?.stack||err);
  if(SMOKE){
   try{if(SMOKE_OUT)fs.writeFileSync(SMOKE_OUT,JSON.stringify({ok:false,error:message,resources:ROOT},null,2));}catch{}
   console.error('DOMINION_SMOKE_FAIL '+message);
   app.exit(1);
   return;
  }
  dialog.showErrorBox('DOMINION 시작 실패',message);app.quit();
 });
}
const sleep=ms=>new Promise(resolve=>setTimeout(resolve,ms));
function freePort(){return new Promise((resolve,reject)=>{const s=net.createServer();s.once('error',reject);s.listen(0,'127.0.0.1',()=>{const p=s.address().port;s.close(()=>resolve(p))})})}
async function start(){
 ipcMain.removeHandler('dominion:pick-video');
 ipcMain.handle('dominion:pick-video',async()=>{
  const answer=await dialog.showOpenDialog(window||undefined,{title:'업로드할 영상 선택',properties:['openFile'],filters:[{name:'Video',extensions:['mp4','mov','webm','mkv','avi','m4v']}]});
  return answer.canceled?'':answer.filePaths[0]||'';
 });
 ipcMain.removeHandler('dominion:show-item');
 ipcMain.handle('dominion:show-item',async(_event,target)=>{
  if(typeof target!=='string'||!target||!fs.existsSync(target))return {ok:false,error:'파일을 찾을 수 없습니다.'};
  shell.showItemInFolder(path.resolve(target));
  return {ok:true};
 });
 ipcMain.removeHandler('dominion:open-path');
 ipcMain.handle('dominion:open-path',async(_event,target)=>{
  if(typeof target!=='string'||!target||!fs.existsSync(target))return {ok:false,error:'폴더를 찾을 수 없습니다.'};
  const error=await shell.openPath(path.resolve(target));
  return error?{ok:false,error}:{ok:true};
 });
 ipcMain.removeHandler('dominion:pick-model');
 ipcMain.handle('dominion:pick-model',async(_event,format)=>{
  const folder=format==='mlx';
  const answer=await dialog.showOpenDialog(window||undefined,{
   title:folder?'MLX 모델 폴더 선택':'GGUF 모델 파일 선택',
   properties:folder?['openDirectory']:['openFile'],
   ...(folder?{}:{filters:[{name:'GGUF models',extensions:['gguf']} ]})
  });
  return answer.canceled?'':answer.filePaths[0]||'';
 });
 if(!DEV_URL&&!fs.existsSync(path.join(ROOT,'dist','index.html')))throw Error('Missing compiled UI. Run npm run build in the repository root.');
 enginePort=0;
 let origin=DEV_URL?DEV_URL.replace(/\/$/,''):'';
 const data=path.join(app.getPath('userData'),'runtime');
 const workspace=path.join(app.getPath('documents'),'DOMINION Workspace');
 fs.mkdirSync(data,{recursive:true});fs.mkdirSync(workspace,{recursive:true});
 if(!DEV_URL){
  enginePort=await freePort();
  engine=spawn(process.execPath,[path.join(ROOT,'server/index.mjs')],{
   cwd:ROOT,stdio:['ignore','pipe','pipe'],windowsHide:true,
   env:{...process.env,ELECTRON_RUN_AS_NODE:'1',DOMINION_PORT:String(enginePort),DOMINION_HOST:'127.0.0.1',DOMINION_DATA:data,DOMINION_WORKSPACE:workspace}
  });
  let stderr='';
  engine.stderr.on('data',d=>{stderr=(stderr+d.toString()).slice(-6000);});
  engine.on('exit',(code,signal)=>{if(!quitting){dialog.showErrorBox('DOMINION Gateway 종료','Engine exited unexpectedly: '+code+' '+signal+'\n'+stderr);app.quit();}});
  origin='http://127.0.0.1:'+enginePort;
  let ready=false;
  for(let i=0;i<100;i++){
   if(engine.exitCode!==null)throw Error('Gateway exited: '+stderr);
   try{const r=await fetch(origin+'/api/health',{signal:AbortSignal.timeout(500)});if(r.ok){ready=true;break}}catch{}
   await sleep(160);
  }
  if(!ready)throw Error('DOMINION gateway startup timed out: '+stderr);
 }
 if(SMOKE){
  const health=await (await fetch(origin+'/api/health',{signal:AbortSignal.timeout(5000)})).json();
  const presets=await (await fetch(origin+'/api/mcp/presets',{signal:AbortSignal.timeout(5000)})).json();
  const result={ok:health.ok===true,version:health.version||'',mcpPresets:Array.isArray(presets)?presets.length:-1,resources:ROOT,userData:app.getPath('userData'),node:process.versions.node,electron:process.versions.electron};
  if(SMOKE_OUT)fs.writeFileSync(SMOKE_OUT,JSON.stringify(result,null,2));
  console.log('DOMINION_SMOKE_OK '+JSON.stringify(result));
  quitting=true;
  if(engine&&!engine.killed)engine.kill();
  app.exit(result.ok?0:1);
  return;
 }
 window=new BrowserWindow({title:'DOMINION',width:760,height:850,minWidth:740,minHeight:650,backgroundColor:'#f6f8fc',show:false,
   webPreferences:{nodeIntegration:false,contextIsolation:true,sandbox:true,webSecurity:true,preload:path.join(__dirname,'preload.cjs')}
 });
 session.defaultSession.setPermissionRequestHandler((_webContents,permission,callback)=>{
  callback(permission==='clipboard-sanitized-write');
 });
 window.webContents.setWindowOpenHandler(({url})=>{
  try{
   const u=new URL(url);
   const oauth=(u.hostname==='github.com'&&u.pathname==='/login/oauth/authorize')||
       (u.hostname==='accounts.google.com'&&u.pathname.startsWith('/o/oauth2/'));
   if(oauth&&u.protocol==='https:')return {action:'allow',overrideBrowserWindowOptions:{webPreferences:{nodeIntegration:false,contextIsolation:true,sandbox:true}}};
   if(u.protocol==='https:')void shell.openExternal(url);
  }catch{}
  return {action:'deny'};
 });
 window.webContents.on('will-navigate',(e,url)=>{
  if(!url.startsWith(origin+'/')){
   e.preventDefault();
   try{if(new URL(url).protocol==='https:')void shell.openExternal(url)}catch{}
  }
 });
 window.once('ready-to-show',()=>window.show());
 await window.loadURL(origin);
 app.on('activate',()=>{if(BrowserWindow.getAllWindows().length===0)void start();});
}
app.on('window-all-closed',()=>{if(process.platform!=='darwin')app.quit();});
let quitStopping=false;
app.on('before-quit',(event)=>{
 if(quitStopping)return;
 event.preventDefault();
 quitStopping=true;quitting=true;
 const done=()=>{if(engine&&!engine.killed)engine.kill();app.quit();};
 if(enginePort){
  fetch('http://127.0.0.1:'+enginePort+'/api/video/stop',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({force:true,onlyManaged:true}),signal:AbortSignal.timeout(2500)}).then(done,done);
 }else done();
});
