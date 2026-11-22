import {UserRound} from 'lucide-react'
import type {Agent} from '../../api'
import {useOwnerProfile} from '../OwnerProfile'

const palette = ['#668373', '#9A745F', '#6D7FA4', '#967AA0', '#B18C47', '#6C9297', '#AB7180', '#7A8456', '#8B7B69']
type Props = {agent?: Agent; name?: string; owner?: boolean; size?: 'small' | 'normal'; index?: number}

export function RoomAvatar({agent, name, owner = false, size = 'normal', index = 0}: Props) {
 const {profile} = useOwnerProfile()
 const source = owner ? profile.avatar : String(agent?.avatar || agent?.avatarUrl || '')
 const color = /^#[0-9a-f]{3,8}$/i.test(agent?.color || '') ? agent!.color! : palette[index % palette.length]
 return <span title={owner ? profile.name : agent?.name || name}
  className={'room-avatar ' + (size === 'small' ? 'room-avatar-small ' : '') + (owner ? 'room-avatar-owner' : '')}
  style={owner ? undefined : {backgroundColor: color}}>
  {source ? <img src={source} alt=""/>
   : owner ? <UserRound size={size === 'small' ? 14 : 18}/>
   : <span>{(agent?.name || name || '?').slice(0, 1)}</span>}
 </span>
}
