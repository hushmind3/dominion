import { OwnerProfileEditor } from './OwnerProfile'
import { MessageSquare, UsersRound, X, Settings, Menu } from 'lucide-react'
import { Button } from './Primitives'
import { t, useLocale } from '../i18n'

import {
  settingsGroups,
  settingsPages,
  isSettingsPage,
} from '../workspacePages'

type SettingsId = keyof typeof settingsPages
type SidebarProps = {
  page: SettingsId
  navOpen: boolean
  online: boolean
  running: number
  pending: number
  onNavigate: (page: SettingsId) => void
  onToggleNav: () => void
}
export function WorkspaceSidebar({
  page,
  navOpen,
  online,
  running,
  pending,
  onNavigate,
  onToggleNav,
}: SidebarProps) {
  useLocale()
  return (
    <aside
      aria-label={t('shell.mainNav')}
      className={
        'flex w-[216px] shrink-0 flex-col border-r border-divider bg-canvas text-ink ' +
        (navOpen ? '' : 'settings-nav-collapsed')
      }
    >
      <div className="flex h-16 shrink-0 items-center gap-3 border-b border-[#E0D8CC] px-5">
        <div className="grid size-8 shrink-0 place-items-center rounded-xl bg-brand text-[17px] font-black text-white shadow-[0_4px_12px_rgba(201,100,66,.28)]">
          D
        </div>
        <div className="min-w-0 flex-1 truncate text-[14px] font-extrabold tracking-[.14em] text-ink">
          DOMINION
        </div>
        <Button
          variant="ghost"
          size="icon"
          aria-label={navOpen ? t('shell.hideMenu') : t('shell.showMenu')}
          aria-expanded={navOpen}
          onClick={onToggleNav}
        >
          <Menu size={18} />
        </Button>
      </div>
      <nav className="mt-4 min-h-0 flex-1 overflow-y-auto px-2.5 pb-4">
        {Array.from(settingsGroups, ([title, items]) => (
          <section key={title} className="mt-5">
            <h2 className="mb-2 px-3 text-[11px] font-semibold text-[#8A8176]">
              {t(title)}
            </h2>
            <div className="space-y-0.5">
              {items.map((item) => {
                const Icon = item.icon
                const count =
                  item.id === 'activity'
                    ? running
                    : item.id === 'settings'
                      ? pending
                      : 0
                return (
                  <Button
                    variant="bare"
                    key={item.id}
                    type="button"
                    aria-current={page === item.id ? 'page' : undefined}
                    onClick={() => onNavigate(item.id)}
                    className={
                      'group flex h-9 w-full items-center gap-3 rounded-xl px-3 text-left text-[13px] transition-colors ' +
                      (page === item.id
                        ? 'bg-surface font-semibold text-ink shadow-[0_2px_8px_rgba(70,58,42,.07)]'
                        : 'text-[#6F675D] hover:bg-[#E8E0D3] hover:text-ink')
                    }
                  >
                    <Icon size={15} strokeWidth={1.8} />
                    <span className="min-w-0 flex-1">{t(item.label)}</span>
                    {count > 0 && (
                      <span className="text-[10px] font-semibold text-brand">
                        {count}
                      </span>
                    )}
                  </Button>
                )
              })}
            </div>
          </section>
        ))}
      </nav>
      <OwnerProfileEditor />
      <div className="flex h-14 shrink-0 items-center gap-2.5 border-t border-[#E0D8CC] px-5">
        <span
          className={
            'size-2 rounded-full ' + (online ? 'bg-emerald-500' : 'bg-rose-400')
          }
        />
        <span className="text-[11px] text-[#6F675D]">
          {online ? t('shell.engineConnected') : t('shell.engineDisconnected')}
        </span>
      </div>
    </aside>
  )
}
export function SettingsHeader({
  page,
  onNavigate,
  online,
  build,
  onClose,
}: {
  page: SettingsId
  onNavigate: (page: SettingsId) => void
  online: boolean
  build?: { branch: string; commit: string } | null
  onClose: () => void
}) {
  useLocale()
  const selected = settingsPages[page]
  return (
    <header className="flex h-13 shrink-0 items-center gap-3 border-b border-divider bg-surface px-4">
      <Settings size={17} className="text-[#8A8176]" />
      <select
        className="min-w-0 flex-1 rounded-control border border-divider bg-surface p-2 md:hidden"
        aria-label={t('shell.mainNav')}
        value={page}
        onChange={(event) => {
          if (isSettingsPage(event.target.value)) onNavigate(event.target.value)
        }}
      >
        {Array.from(settingsGroups, ([group, entries]) => (
          <optgroup key={group} label={t(group)}>
            {entries.map((entry) => (
              <option key={entry.id} value={entry.id}>
                {t(entry.label)}
              </option>
            ))}
          </optgroup>
        ))}
      </select>
      <h1 className="hidden md:block min-w-0 flex-1 truncate text-[14px] font-semibold text-ink">
        {t(selected.label)}
      </h1>
      <span
        className={
          'hidden items-center gap-1.5 text-[11px] sm:inline-flex ' +
          (online ? 'text-emerald-700' : 'text-rose-600')
        }
      >
        <i
          className={
            'size-1.5 rounded-full ' +
            (online ? 'bg-emerald-500' : 'bg-rose-500')
          }
        />
        {online ? t('shell.connected') : t('shell.disconnected')}
      </span>
      {build?.branch && build.branch !== 'unknown' && (
        <span
          className="hidden max-w-[185px] truncate rounded-md border border-divider bg-[#F7F3EC] px-2 py-1 text-[10px] font-mono text-[#8A8176] lg:block"
          title={build.branch + ' · ' + build.commit}
        >
          {build.branch.replace(/^dominion-/, '')} · {build.commit.slice(0, 7)}
        </span>
      )}
      <Button
        variant="ghost"
        size="icon"
        aria-label={t('rooms.close')}
        onClick={onClose}
      >
        <X size={18} />
      </Button>
    </header>
  )
}
/** Persistent toolbar navigation; independent of either session list. */
export function ConversationSwitch({
  mode,
  onChange,
}: {
  mode: 'chat' | 'rooms'
  onChange: (mode: 'chat' | 'rooms') => void
}) {
  useLocale()
  return (
    <nav
      className="conversation-switch"
      aria-label={t('shell.conversationSwitch')}
    >
      <Button
        variant="ghost"
        aria-current={mode === 'chat' ? 'page' : undefined}
        onClick={() => onChange('chat')}
      >
        <MessageSquare size={16} />
        <span>{t('nav.chat')}</span>
      </Button>
      <Button
        variant="ghost"
        aria-current={mode === 'rooms' ? 'page' : undefined}
        onClick={() => onChange('rooms')}
      >
        <UsersRound size={16} />
        <span>{t('rooms.listTitle')}</span>
      </Button>
    </nav>
  )
}
