import {UserRound} from 'lucide-react'
import type {Agent} from '../../api'
import {useOwnerProfile} from '../OwnerProfile'
import {cn} from '../Primitives'

type Props={agent?:Agent;name?:string;owner?:boolean;size?:'small'|'normal';index?:number}
export function RoomAvatar({agent,name,owner=false,size='normal'}:Props) {
 const {profile}=useOwnerProfile()
 const source=owner?profile.avatar:String(agent?.avatar||agent?.avatarUrl||'')
 const customColor=!owner&&/^#[0-9a-f]{3,8}$/i.test(agent?.color||'')?agent?.color:undefined
 return <span title={owner?profile.name:agent?.name||name} className={cn('room-avatar inline-flex shrink-0 items-center justify-center overflow-hidden rounded-xl bg-brand-soft text-sm font-semibold text-brand-hover',size==='small'?'size-6 rounded-lg text-xs':'size-8',owner&&'bg-canvas text-muted',Boolean(customColor)&&'text-white')} style={customColor?{backgroundColor:customColor}:undefined}>
  {source?<img src={source} alt="" className="size-full object-cover"/>:owner?<UserRound size={size==='small'?14:18}/>:<span>{(agent?.name||name||'?').slice(0,1)}</span>}
 </span>
}
