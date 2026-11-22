import {Activity,BookOpenText,Bot,Cpu,House,MessageSquare,RadioTower,Settings2,UsersRound} from 'lucide-react'
import type {LocaleKey} from './i18n'

/** Every route the shell can render. Chat and rooms are full-canvas trees;
 *  the rest render inside the settings workspace. */
export type PageId =
  | 'home'
  | 'chat'
  | 'models'
  | 'agents'
  | 'rooms'
  | 'memory'
  | 'capabilities'
  | 'extensions'
  | 'connections'
  | 'activity'
  | 'settings'

export const pageIds: readonly PageId[] = [
  'home','chat','models','agents','rooms','memory',
  'capabilities','extensions','connections','activity','settings',
]

export const pageLabels: Record<PageId, LocaleKey> = {
  home:'nav.home', chat:'nav.chat', models:'nav.models', agents:'nav.agents',
  rooms:'nav.rooms', memory:'nav.memory', capabilities:'nav.capabilities',
  extensions:'nav.extensions', connections:'nav.connections',
  activity:'nav.activity', settings:'nav.settings',
}

export function isPageId(value: string): value is PageId {
  return (pageIds as readonly string[]).includes(value)
}

export type NavItem = {id: PageId; label: LocaleKey; icon: typeof House}
export type NavGroup = {title: LocaleKey; items: NavItem[]; collapsible?: boolean}

/** The only menu definition. Sidebar, search and the hash router all read it. */
export const navigation: NavGroup[] = [
  {title:'nav.group.workspace',items:[
    {id:'home',label:'nav.home',icon:House},
    {id:'chat',label:'nav.chat',icon:MessageSquare},
    {id:'rooms',label:'nav.rooms',icon:UsersRound},
    {id:'models',label:'nav.models',icon:Cpu},
    {id:'agents',label:'nav.agents',icon:Bot},
  ]},
  {title:'nav.group.extensions',items:[
    {id:'connections',label:'nav.connections',icon:RadioTower},
    {id:'memory',label:'nav.memory',icon:BookOpenText},
  ]},
  {title:'nav.group.operations',items:[
    {id:'activity',label:'nav.activity',icon:Activity},
    {id:'settings',label:'nav.settings',icon:Settings2},
  ]},
]

export const navigationItems: NavItem[] = navigation.flatMap(group => group.items)
