import {useEffect,useRef,useState} from 'react'
import {get} from '../../../api'
import {useRemote} from '../../../components/UI'
export type Category={slug:string;label:string;description:string;icon:string}
export type Connection={id?:string;name?:string;description?:string;packageName?:string;url?:string;transport?:'http'|'stdio';command?:string;args?:string[];enabled?:boolean}
export type HostPlugin={id:string;name:string;description?:string;installed:boolean;enabled:boolean;state?:string;packageName?:string;clawhubPackage?:string;categories?:string[];version?:string;removable?:boolean;hasIcon?:boolean}
export type Plugin={name:string;displayName?:string;summary?:string;categories?:string[];ownerHandle?:string;ownerOfficial?:boolean;icon?:string|null;ownerImage?:string;latestVersion?:string;family?:string;runtimeId?:string;featured?:boolean;featuredRank?:number;trending?:boolean;trendingRank?:number;stats?:{downloads?:number};connection?:Connection;installed?:boolean;enabled?:boolean;host?:HostPlugin;appId?:string;nativeKind?:string;nativeTools?:{name:string;description:string}[]}
type Page={items:Plugin[];nextCursor?:string|null}
type Overview=Page&{categories:Category[]}
export function usePluginDirectory(filter:string,query:string,installedOnly=false){
 const overview=useRemote<Overview>('/api/plugins/catalog?view=overview',{items:[],categories:[]},false)
 const installed=useRemote<Connection[]>('/api/mcp',[])
 const host=useRemote<{plugins:HostPlugin[]}>('/api/plugins/host/plugins',{plugins:[]})
 const apps=useRemote<{id:string;manifest:{name:string;version:string;description:string};enabled:boolean}[]>('/api/extensions/apps',[])
 const native=useRemote<{items:{id:string;name:string;description:string;namespace:string}[]}>('/api/capabilities',{items:[]},false)
 const channels=useRemote<{id:string;name:string;description:string;available:boolean}[]>('/api/channels/catalog',[],false)
 const bindings=useRemote<{kind:string;enabled:boolean}[]>('/api/channels',[])
 const mcp=useRemote<Connection[]>('/api/mcp/catalog'+(query?'?q='+encodeURIComponent(query):''),[],false)
 const params=new URLSearchParams({view:query?'search':'list',sort:filter==='popular'?'trending':'recommended'})
 if(query)params.set('q',query)
 if(filter==='featured')params.set('featured','true');else if(!['all','popular'].includes(filter))params.set('category',filter)
 const endpoint='/api/plugins/catalog?'+params,isOverview=filter==='all'&&!query
 const page=useRemote<Page>(isOverview||installedOnly?null:endpoint,{items:[]},false)
 const [more,setMore]=useState<{key:string;items:Plugin[];cursor?:string|null}>({key:'',items:[]})
 const [moreBusy,setMoreBusy]=useState(false),[moreError,setMoreError]=useState('')
 const request=useRef<AbortController|null>(null)
 useEffect(()=>{request.current?.abort();request.current=null;setMoreBusy(false);setMoreError('');return()=>request.current?.abort()},[endpoint])
 const feed=isOverview?overview:page,extras=more.key===endpoint?more.items:[],cursor=more.key===endpoint?more.cursor:page.value.nextCursor
 const hosts=host.value.plugins.filter(item=>item.installed)
 const remoteItems=[...new Map([...feed.value.items,...extras].map(item=>[item.name,item])).values()].map(item=>{
  const local=hosts.find(row=>[row.packageName,row.clawhubPackage].includes(item.name))
  return local?{...item,host:local,installed:true,enabled:local.enabled}:item
 })
 const other=overview.value.categories.find(category=>category.slug==='other')?.slug||'integrations'
 const connections=[...new Map([...installed.value,...mcp.value].map(item=>[item.name,item])).values()]
 const localItems:Plugin[]=[
  ...hosts.filter(row=>!remoteItems.some(item=>item.host?.id===row.id)).map(row=>({name:row.packageName||'host:'+row.id,displayName:row.name,summary:row.description,categories:row.categories||[other],latestVersion:row.version,host:row,installed:true,enabled:row.enabled})),
  ...connections.map(item=>{const row=installed.value.find(row=>row.name===item.name);return {name:'mcp:'+(item.name||''),displayName:item.name,summary:item.description,categories:[other],connection:{...item,...row},installed:Boolean(row),enabled:row?.enabled}}),
  ...apps.value.map(app=>({name:'sdk:'+app.id,displayName:app.manifest.name,summary:app.manifest.description,latestVersion:app.manifest.version,categories:[other],appId:app.id,installed:true,enabled:app.enabled})),
  ...channels.value.filter(channel=>channel.available&&!remoteItems.some(item=>item.runtimeId===channel.id)&&!hosts.some(item=>item.id===channel.id)).map(channel=>({name:'channel:'+channel.id,displayName:channel.name,summary:channel.description,categories:['channels'],nativeKind:channel.id,installed:bindings.value.some(row=>row.kind===channel.id),enabled:bindings.value.some(row=>row.kind===channel.id&&row.enabled)})),
  ...(native.value.items.length?[{name:'native:dominion',displayName:'Dominion',summary:'도미니온의 파일·코드·검색·계정 연결과 기본 도구',ownerHandle:'dominion',categories:['developer-tools'],nativeTools:native.value.items.filter(item=>item.namespace==='builtin').map(item=>({name:item.name,description:item.description})),installed:true,enabled:true}]:[]),
 ]
 const items=[...remoteItems,...localItems].filter(item=>(!installedOnly||item.installed)&&(!query||[item.displayName,item.name,item.summary].join(' ').toLowerCase().includes(query.toLowerCase()))&&(filter==='all'||filter==='featured'||filter==='popular'||item.categories?.includes(filter)))
 const loadMore=async()=>{if(!cursor||request.current)return;const controller=new AbortController();request.current=controller;setMoreBusy(true);setMoreError('');try{const next=await get<Page>(endpoint+'&cursor='+encodeURIComponent(cursor),controller.signal);if(!controller.signal.aborted)setMore(previous=>({key:endpoint,items:[...(previous.key===endpoint?previous.items:[]),...next.items],cursor:next.nextCursor}))}catch(e){if(!controller.signal.aborted)setMoreError((e as Error).message)}finally{if(request.current===controller){request.current=null;setMoreBusy(false)}}}
 const reload=()=>Promise.all([overview.reload(),page.reload(),mcp.reload(),installed.reload(),host.reload(),apps.reload(),bindings.reload()])
 return {categories:overview.value.categories,items,loading:feed.loading,error:feed.error||overview.error||mcp.error||installed.error||host.error||apps.error||moreError,hasMore:!isOverview&&!installedOnly&&Boolean(cursor),moreBusy,loadMore,reload,installed}
}
