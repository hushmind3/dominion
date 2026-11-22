import React from 'react'
import {createRoot} from 'react-dom/client'
import App from './App'
import './theme.css'
import './styles/studio.css'
import './styles/native-command.css'
const root=document.getElementById('root')
if(root)createRoot(root).render(<React.StrictMode><App/></React.StrictMode>)
