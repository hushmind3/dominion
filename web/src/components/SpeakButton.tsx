import {useCallback,useEffect,useRef,useState} from 'react'
import {Square, Volume2} from 'lucide-react'
import {Button} from './Primitives'
import {useLocale} from '../i18n'
import {apiFetch} from '../api'

type SpeakButtonProps={text:string}

/// Reads an assistant message aloud. Uses the local Piper runtime when it is
/// installed and falls back to the browser voice otherwise.
export function SpeakButton({text}:SpeakButtonProps){
 const locale=useLocale()
 const label=locale==='ko'?'읽어주기':'Read aloud'
 const stopLabel=locale==='ko'?'읽기 중지':'Stop reading'
 const [playing,setPlaying]=useState(false)
 const [busy,setBusy]=useState(false)
 const [serverReady,setServerReady]=useState(false)
 const audioRef=useRef<HTMLAudioElement|null>(null)
 const urlRef=useRef<string>('')

 useEffect(()=>{
  let alive=true
  void apiFetch('/api/voice/status').then(response=>response.json()).then((status:any)=>{
   if(alive)setServerReady(Boolean(status?.piper?.available))
  }).catch(()=>{if(alive)setServerReady(false)})
  return ()=>{alive=false}
 },[])

 const stop=useCallback(()=>{
  if(audioRef.current){audioRef.current.pause();audioRef.current=null}
  if(urlRef.current){URL.revokeObjectURL(urlRef.current);urlRef.current=''}
  if(typeof window!=='undefined'&&'speechSynthesis' in window)window.speechSynthesis.cancel()
  setPlaying(false)
 },[])

 useEffect(()=>()=>{stop()},[stop])

 const speak=useCallback(async()=>{
  if(playing){stop();return}
  const body=text.trim()
  if(!body)return
  setBusy(true)
  try{
   if(serverReady){
    const response=await apiFetch('/api/voice/speak',{method:'POST',headers:{'Content-Type':'application/json'},body:JSON.stringify({text:body})})
    if(!response.ok){
     const payload=await response.json().catch(()=>({}))
     throw new Error(String(payload?.error||response.status))
    }
    const blob=await response.blob()
    const url=URL.createObjectURL(blob)
    urlRef.current=url
    const audio=new Audio(url)
    audioRef.current=audio
    audio.onended=()=>{stop()}
    audio.onerror=()=>{stop()}
    setPlaying(true)
    await audio.play()
    return
   }
   if(typeof window==='undefined'||!('speechSynthesis' in window))return
   const utterance=new SpeechSynthesisUtterance(body)
   utterance.lang=locale==='ko'?'ko-KR':'en-US'
   utterance.onend=()=>setPlaying(false)
   utterance.onerror=()=>setPlaying(false)
   window.speechSynthesis.cancel()
   window.speechSynthesis.speak(utterance)
   setPlaying(true)
  }catch{
   setPlaying(false)
  }finally{
   setBusy(false)
  }
 },[locale,playing,serverReady,stop,text])

 return <Button className="inline-flex items-center gap-1.5 rounded-lg px-2 py-1 text-[11px] text-[#7C7469] hover:bg-[#F3EEE5]"
  onClick={()=>void speak()} disabled={busy} title={playing?stopLabel:label} aria-label={playing?stopLabel:label} aria-pressed={playing}>
  {playing?<Square size={14} fill="currentColor"/>:<Volume2 size={14}/>}
 </Button>
}
