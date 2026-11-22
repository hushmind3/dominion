// A deploy replaces hashed chunks. Open tabs can still ask for the previous
// build and fail a lazy import; reload once so the menu keeps working.
window.addEventListener('vite:preloadError', (event) => {
  event.preventDefault()
  const key = 'dominion.preload-retry'
  try {
    if (sessionStorage.getItem(key)) return
    sessionStorage.setItem(key, '1')
  } catch {}
  window.location.reload()
})
window.addEventListener('load', () => {
  try { sessionStorage.removeItem('dominion.preload-retry') } catch {}
})

import React from 'react'
import {createRoot} from 'react-dom/client'
import App from './App'
import './theme.css'
const root=document.getElementById('root')
if(root)createRoot(root).render(<React.StrictMode><App/></React.StrictMode>)
