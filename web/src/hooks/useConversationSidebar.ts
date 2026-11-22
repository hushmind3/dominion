import {useModalFocus} from './useModalFocus'
import {useEffect, useRef, useState, type KeyboardEvent, type PointerEvent} from 'react'

const defaultWidth = 260
const minWidth = 220
const maxWidth = 400
const clampWidth = (width: number) => Math.max(minWidth, Math.min(maxWidth, width))

/** A sidebar owns its visibility, size and drag lifecycle in one place. */
export function useConversationSidebar(storageKey='dominion.chat.sidebar', active=true) {
  const [open, setOpen] = useState(() => {
    try {return window.innerWidth >= 768 && localStorage.getItem(storageKey) !== '0'} catch {return window.innerWidth>=768}
  })
  const [width, setWidth] = useState(() => {
    try {
      const saved = Number(localStorage.getItem(storageKey+'.width'))
      return Number.isFinite(saved) && saved >= minWidth ? clampWidth(saved) : defaultWidth
    } catch {return defaultWidth}
  })
  const [mobile,setMobile]=useState(()=>window.innerWidth<768)
  const panel=useRef<HTMLElement>(null)
  useModalFocus(panel,()=>setOpen(false),active&&open&&mobile)
  const drag = useRef<{id: number; x: number; width: number; rawWidth: number} | null>(null)
  const finishDrag = () => {
    drag.current = null
    document.documentElement.classList.remove('chat-sidebar-resizing')
  }
  useEffect(() => {
    const mobile = window.matchMedia('(max-width: 767px)')
    const sync = () => {setMobile(mobile.matches);if (mobile.matches) {finishDrag(); setOpen(false)}}
    mobile.addEventListener('change', sync)
    return () => {mobile.removeEventListener('change', sync); finishDrag()}
  }, [])
  useEffect(() => {
    if (window.innerWidth < 768) return
    try {localStorage.setItem(storageKey, open ? '1' : '0')} catch {}
  }, [open,storageKey])
  useEffect(() => {
    const timer = setTimeout(() => {
      try {localStorage.setItem(storageKey+'.width', String(width))} catch {}
    }, 200)
    return () => clearTimeout(timer)
  }, [width,storageKey])

  const resize = {
    role: 'separator' as const,
    tabIndex: 0,
    'aria-orientation': 'vertical' as const,
    'aria-valuemin': minWidth,
    'aria-valuemax': maxWidth,
    'aria-valuenow': width,
    onDoubleClick: () => setWidth(defaultWidth),
    onPointerDown: (event: PointerEvent<HTMLDivElement>) => {
      if (event.button !== 0) return
      event.preventDefault()
      event.currentTarget.setPointerCapture(event.pointerId)
      drag.current = {id: event.pointerId, x: event.clientX, width, rawWidth: width}
      document.documentElement.classList.add('chat-sidebar-resizing')
    },
    onPointerMove: (event: PointerEvent<HTMLDivElement>) => {
      if (drag.current?.id !== event.pointerId) return
      drag.current.rawWidth = drag.current.width + event.clientX - drag.current.x
      setWidth(clampWidth(drag.current.rawWidth))
    },
    onPointerUp: (event: PointerEvent<HTMLDivElement>) => {
      if (drag.current?.id !== event.pointerId) return
      const collapse = drag.current.rawWidth < 150
      finishDrag()
      event.currentTarget.releasePointerCapture(event.pointerId)
      if (collapse) setOpen(false)
    },
    onPointerCancel: finishDrag,
    onLostPointerCapture: finishDrag,
    onKeyDown: (event: KeyboardEvent<HTMLDivElement>) => {
      if (event.key === 'ArrowLeft' || event.key === 'ArrowRight') {
        event.preventDefault()
        setWidth(value => clampWidth(value + (event.key === 'ArrowLeft' ? -20 : 20)))
      } else if (event.key === 'Home') {
        event.preventDefault()
        setWidth(defaultWidth)
      } else if (event.key === 'Enter') {
        event.preventDefault()
        setOpen(false)
      }
    },
  }
  return {open, setOpen, width, resize, mobile, panel}
}
