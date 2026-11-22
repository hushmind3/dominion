import { lazy, type ReactNode } from 'react'
import {
  Activity,
  Blocks,
  BookOpenText,
  Bot,
  Cpu,
  House,
  RadioTower,
  Settings2,
  ShieldCheck,
  type LucideIcon,
} from 'lucide-react'
import type { LocaleKey } from './i18n'

const HomePage = lazy(() =>
  import('./pages/Home').then((module) => ({ default: module.HomePage })),
)
const CapabilitiesPage = lazy(() =>
  import('./pages/Capabilities').then((module) => ({
    default: module.CapabilitiesPage,
  })),
)
const MemoryPage = lazy(() =>
  import('./pages/Memory').then((module) => ({ default: module.MemoryPage })),
)
const ModelHub = lazy(() =>
  import('./pages/Models').then((module) => ({ default: module.ModelHub })),
)
const AgentsPage = lazy(() =>
  import('./pages/Agents').then((module) => ({ default: module.AgentsPage })),
)
const ExtensionsPage = lazy(() =>
  import('./pages/Extensions').then((module) => ({
    default: module.ExtensionsPage,
  })),
)
const ConnectionsPage = lazy(() =>
  import('./pages/Connections').then((module) => ({
    default: module.ConnectionsPage,
  })),
)
const OperationsPage = lazy(() =>
  import('./pages/Operations').then((module) => ({
    default: module.OperationsPage,
  })),
)

export type PageId = 'chat' | 'rooms' | SettingsPage
export type SettingsPage = keyof typeof settingsPages
type Navigate = (page: PageId, sessionId?: string) => void
type SettingsEntry = {
  label: LocaleKey
  description: LocaleKey
  group: LocaleKey
  icon: LucideIcon
  /** Shown in the always-visible part of the rail; everything else sits under
   *  the 'more' section. Adding a page means adding one entry here. */
  primary?: boolean
  render: (navigate: Navigate) => ReactNode
}

export const settingsPages = {
  home: {
    label: 'nav.home',
    description: 'workspace.description.home',
    group: 'nav.group.workspace',
    icon: House,
    primary: true,
    render: (navigate: Navigate) => <HomePage navigate={navigate} />,
  },
  models: {
    label: 'nav.models',
    description: 'workspace.description.models',
    group: 'nav.group.workspace',
    icon: Cpu,
    primary: true,
    render: () => <ModelHub />,
  },
  agents: {
    label: 'nav.agents',
    description: 'workspace.description.agents',
    group: 'nav.group.workspace',
    icon: Bot,
    render: () => <AgentsPage />,
  },
  connections: {
    label: 'nav.connections',
    description: 'workspace.description.connections',
    group: 'nav.group.extensions',
    icon: RadioTower,
    render: () => <ConnectionsPage />,
  },
  capabilities: {
    label: 'nav.capabilities',
    description: 'workspace.description.capabilities',
    group: 'nav.group.extensions',
    icon: ShieldCheck,
    render: (navigate: Navigate) => <CapabilitiesPage navigate={navigate} />,
  },
  extensions: {
    label: 'nav.extensions',
    description: 'workspace.description.extensions',
    group: 'nav.group.extensions',
    icon: Blocks,
    render: () => <ExtensionsPage />,
  },
  memory: {
    label: 'nav.memory',
    description: 'workspace.description.memory',
    group: 'nav.group.extensions',
    icon: BookOpenText,
    primary: true,
    render: () => <MemoryPage />,
  },
  activity: {
    label: 'nav.activity',
    description: 'workspace.description.activity',
    group: 'nav.group.operations',
    icon: Activity,
    render: () => <OperationsPage section="activity" />,
  },
  settings: {
    label: 'nav.settings',
    description: 'workspace.description.settings',
    group: 'nav.group.operations',
    icon: Settings2,
    primary: true,
    render: () => <OperationsPage section="settings" />,
  },
} satisfies Record<string, SettingsEntry>

export function isSettingsPage(value: string): value is SettingsPage {
  return Object.hasOwn(settingsPages, value)
}

export const settingsGroups = new Map<
  LocaleKey,
  Array<SettingsEntry & { id: SettingsPage }>
>()
for (const id of Object.keys(settingsPages).filter(isSettingsPage)) {
  const entry = settingsPages[id]
  const group = settingsGroups.get(entry.group) ?? []
  group.push({ ...entry, id })
  settingsGroups.set(entry.group, group)
}

/** One source for the rail: primary pages stay visible, the rest are grouped. */
export const primarySettingsPages = Object.keys(settingsPages)
  .filter(isSettingsPage)
  .filter((id) => (settingsPages[id] as SettingsEntry).primary === true)

export const secondarySettingsPages = Object.keys(settingsPages)
  .filter(isSettingsPage)
  .filter((id) => (settingsPages[id] as SettingsEntry).primary !== true)
