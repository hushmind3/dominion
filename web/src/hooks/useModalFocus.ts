import {useLayoutEffect, useRef, type RefObject} from 'react'

const scopes: symbol[] = []
let previousOverflow = ''
const controls = 'button:not([disabled]),a[href],input:not([disabled]),select:not([disabled]),textarea:not([disabled]),[tabindex]:not([tabindex="-1"])'

/** Dialogs and mobile drawers share focus, Escape and scroll ownership. */
export function useModalFocus<T extends HTMLElement>(container: RefObject<T | null>, onDismiss: () => void, active = true) {
  const dismiss = useRef(onDismiss)
  dismiss.current = onDismiss
  useLayoutEffect(() => {
    if (!active) return
    const id = Symbol()
    const root = document.documentElement
    if (!scopes.length) {previousOverflow = root.style.overflow; root.style.overflow = 'hidden'}
    scopes.push(id)
    const origin = document.activeElement
    const focusable = () => Array.from(container.current?.querySelectorAll<HTMLElement>(controls) || [])
      .filter(element => element.getClientRects().length > 0 && !element.closest('[inert]') && element.getAttribute('aria-hidden') !== 'true')
    const focusInside = () => {
      const panel = container.current
      if (scopes.at(-1) === id && panel && !panel.contains(document.activeElement)) (focusable()[0] ?? panel).focus({preventScroll: true})
    }
    focusInside()
    const frame = requestAnimationFrame(focusInside)
    const onKeyDown = (event: KeyboardEvent) => {
      if (scopes.at(-1) !== id || event.defaultPrevented) return
      if (event.key === 'Escape' && !event.isComposing) {event.preventDefault(); dismiss.current(); return}
      if (event.key !== 'Tab') return
      const panel = container.current
      if (!panel) return
      const items = focusable()
      if (!items.length) {event.preventDefault(); panel.focus(); return}
      const first = items[0], last = items[items.length - 1], current = document.activeElement
      if (!panel.contains(current)) {event.preventDefault(); (event.shiftKey ? last : first).focus()}
      else if (event.shiftKey && current === first) {event.preventDefault(); last.focus()}
      else if (!event.shiftKey && current === last) {event.preventDefault(); first.focus()}
    }
    document.addEventListener('keydown', onKeyDown)
    return () => {
      cancelAnimationFrame(frame)
      document.removeEventListener('keydown', onKeyDown)
      const top = scopes.at(-1) === id
      const index = scopes.indexOf(id)
      if (index >= 0) scopes.splice(index, 1)
      if (!scopes.length) root.style.overflow = previousOverflow
      if (top && origin instanceof HTMLElement && origin.isConnected && origin.getClientRects().length>0 && !origin.closest('[inert]')) origin.focus({preventScroll: true})
    }
  }, [active, container])
}
