'use strict';
const {contextBridge,ipcRenderer}=require('electron');
contextBridge.exposeInMainWorld('dominionDesktop',Object.freeze({
 pickModel:(format)=>ipcRenderer.invoke('dominion:pick-model',format==='mlx'?'mlx':'gguf'),
 pickVideo:()=>ipcRenderer.invoke('dominion:pick-video')
}));
