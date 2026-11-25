import {useCallback,useEffect,useRef,useState} from 'react'
import {Mic} from 'lucide-react'
import {Button} from './Primitives'
import {useLocale} from '../i18n'

type VoiceButtonProps={onText:(text:string)=>void;disabled?:boolean}

/// Dictation button backed by the browser speech engine. The server boundary
/// stays at text: only the finished transcript is handed to the composer, so no
/// audio leaves the device and no voice runtime has to be installed first.
export function VoiceButton({onText,disabled}:VoiceButtonProps){
 const locale=useLocale()
 const label=locale==='ko'?'음성 입력':'Voice input'
 const listeningLabel=locale==='ko'?'듣는 중':'Listening'
 const [listening,setListening]=useState(false)
 const [error,setError]=useState('')
 const recognitionRef=useRef<any>(null)
 const supported=typeof window!=='undefined'&&Boolean((window as any).SpeechRecognition||(window as any).webkitSpeechRecognition)
 useEffect(()=>()=>{try{recognitionRef.current?.stop()}catch{/* already stopped */}},[])
 const toggle=useCallback(()=>{
  if(listening){try{recognitionRef.current?.stop()}catch{/* already stopped */}return}
  const Recognition=(window as any).SpeechRecognition||(window as any).webkitSpeechRecognition
  if(!Recognition)return
  const recognition=new Recognition()
  recognition.lang=locale==='ko'?'ko-KR':'en-US'
  recognition.interimResults=false
  recognition.continuous=false
  recognition.onresult=(event:any)=>{
   let text=''
   for(let index=event.resultIndex;index<event.results.length;index+=1){
    text+=event.results[index][0]?.transcript||''
   }
   const trimmed=text.trim()
   if(trimmed)onText(trimmed)
  }
  recognition.onerror=(event:any)=>{setError(String(event?.error||'voice error'));setListening(false)}
  recognition.onend=()=>setListening(false)
  recognitionRef.current=recognition
  setError('')
  try{recognition.start();setListening(true)}catch{setError('voice start failed')}
 },[listening,locale,onText])
 if(!supported)return null
 return <Button type="button" variant="ghost" size="icon"
  className={'!h-7 !w-7 !border-0 !bg-transparent '+(listening?'!text-rose-600':'!text-[#5C544A]')}
  title={error||(listening?listeningLabel:label)}
  aria-label={listening?listeningLabel:label}
  aria-pressed={listening}
  disabled={disabled}
  onClick={toggle}><Mic size={16}/></Button>
}
