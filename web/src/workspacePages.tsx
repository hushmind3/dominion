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
type SettingsPage = keyof typeof settingsPages
type Navigate = (page: PageId, sessionId?: string) => void
type SettingsEntry = {
  label: LocaleKey
  group: LocaleKey
  icon: LucideIcon
  render: (navigate: Navigate) => ReactNode
}

export const settingsPages = {
  home: {
    label: 'nav.home',
    group: 'nav.group.workspace',
    icon: House,
    render: (navigate: Navigate) => <HomePage navigate={navigate} />,
  },
  models: {
    label: 'nav.models',
    group: 'nav.group.workspace',
    icon: Cpu,
    render: () => <ModelHub />,
  },
  agents: {
    label: 'nav.agents',
    group: 'nav.group.workspace',
    icon: Bot,
    render: () => <AgentsPage />,
  },
  connections: {
    label: 'nav.connections',
    group: 'nav.group.extensions',
    icon: RadioTower,
    render: () => <ConnectionsPage />,
  },
  capabilities: {
    label: 'nav.capabilities',
    group: 'nav.group.extensions',
    icon: ShieldCheck,
    render: (navigate: Navigate) => <CapabilitiesPage navigate={navigate} />,
  },
  extensions: {
    label: 'nav.extensions',
    group: 'nav.group.extensions',
    icon: Blocks,
    render: () => <ExtensionsPage />,
  },
  memory: {
    label: 'nav.memory',
    group: 'nav.group.extensions',
    icon: BookOpenText,
    render: () => <MemoryPage />,
  },
  activity: {
    label: 'nav.activity',
    group: 'nav.group.operations',
    icon: Activity,
    render: () => <OperationsPage section="activity" />,
  },
  settings: {
    label: 'nav.settings',
    group: 'nav.group.operations',
    icon: Settings2,
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
