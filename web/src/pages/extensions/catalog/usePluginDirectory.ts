import {useEffect,useRef,useState} from 'react'
import {get} from '../../../api'
import {useRemote} from '../../../components/UI'
export type Category={slug:string;label:string;description:string;icon:string}
export type Plugin={name:string;displayName?:string;summary?:string;categories?:string[];ownerHandle?:string;ownerOfficial?:boolean;icon?:string|null;ownerImage?:string;latestVersion?:string;family?:string;runtimeId?:string;featured?:boolean;featuredRank?:number;trending?:boolean;trendingRank?:number;stats?:{downloads?:number};connection?:{packageName?:string;url?:string};installed?:boolean;enabled?:boolean}
type Page={items:Plugin[];nextCursor?:string|null}
type Overview=Page&{categories:Category[]}
type Mcp={name:string;description?:string;packageName?:string;url?:string;enabled?:boolean}
export function usePluginDirectory(filter:string,query:string){
 const overview=useRemote<Overview>('/api/plugins/catalog?view=overview',{items:[],categories:[]},false)
 const installed=useRemote<(Mcp&{id:string})[]>('/api/mcp',[])
 const mcp=useRemote<Mcp[]>('/api/mcp/catalog'+(query?'?q='+encodeURIComponent(query):''),[],false)
 const params=new URLSearchParams({view:query?'search':'list',sort:filter==='popular'?'trending':'recommended'})
 if(query)params.set('q',query)
 if(filter==='featured')params.set('featured','true')
 else if(!['all','popular'].includes(filter))params.set('category',filter)
 const endpoint='/api/plugins/catalog?'+params
 const isOverview=filter==='all'&&!query
 const page=useRemote<Page>(isOverview?null:endpoint,{items:[]},false)
 const [more,setMore]=useState<{key:string;items:Plugin[];cursor?:string|null}>({key:'',items:[]})
 const [moreBusy,setMoreBusy]=useState(false),[moreError,setMoreError]=useState('')
 const request=useRef<AbortController|null>(null)
 useEffect(()=>{request.current?.abort();request.current=null;setMoreBusy(false);setMoreError('');return()=>request.current?.abort()},[endpoint])
 const feed=isOverview?overview:page
 const extras=more.key===endpoint?more.items:[]
 const cursor=more.key===endpoint?more.cursor:page.value.nextCursor
 const remoteItems=[...new Map([...feed.value.items,...extras].map(item=>[item.name,item])).values()]
 const other=overview.value.categories.find(category=>category.slug==='other')?.slug||'integrations'
 const connections=[...new Map([...installed.value,...mcp.value].map(item=>[item.name,item])).values()]
 const mcpItems:Plugin[]=(filter==='all'||filter===other?connections:[]).filter(item=>!query||[item.name,item.description].join(' ').toLowerCase().includes(query.toLowerCase())).map(item=>{
  const row=installed.value.find(row=>row.name===item.name)
  return {name:item.name,displayName:item.name,summary:item.description,categories:[other],connection:{packageName:item.packageName,url:item.url},installed:Boolean(row),enabled:row?.enabled}
 })
 const items=[...remoteItems,...mcpItems.filter(item=>!remoteItems.some(other=>other.name===item.name))]
 const loadMore=async()=>{
  if(!cursor||request.current)return
  const controller=new AbortController();request.current=controller;setMoreBusy(true);setMoreError('')
  try{const next=await get<Page>(endpoint+'&cursor='+encodeURIComponent(cursor),controller.signal);if(!controller.signal.aborted)setMore(previous=>({key:endpoint,items:[...(previous.key===endpoint?previous.items:[]),...next.items],cursor:next.nextCursor}))}
  catch(e){if(!controller.signal.aborted)setMoreError((e as Error).message)}
  finally{if(request.current===controller){request.current=null;setMoreBusy(false)}}
 }
 return {categories:overview.value.categories,items,loading:feed.loading,error:feed.error||overview.error||mcp.error||installed.error||moreError,hasMore:!isOverview&&Boolean(cursor),moreBusy,loadMore,reload:()=>Promise.all([overview.reload(),page.reload(),mcp.reload(),installed.reload()]),installed}
}
