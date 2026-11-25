import {useCallback,useEffect,useRef,useState} from 'react'
import {Mic} from 'lucide-react'
import {Button} from './Primitives'
import {useLocale} from '../i18n'
import {apiFetch} from '../api'

type VoiceButtonProps={onText:(text:string)=>void;disabled?:boolean}

type VoiceStatus={whisper?:{available?:boolean}}

/// Mic button with two backends. When the local Whisper runtime and model are
/// installed the app records 16 kHz mono WAV in the browser and posts it to
/// /api/voice/transcribe; otherwise it falls back to the browser speech engine.
export function VoiceButton({onText,disabled}:VoiceButtonProps){
 const locale=useLocale()
 const label=locale==='ko'?'음성 입력':'Voice input'
 const listeningLabel=locale==='ko'?'듣는 중':'Listening'
 const unsupportedLabel=locale==='ko'?'이 브라우저에서는 음성 입력을 쓸 수 없습니다':'Voice input is unavailable in this browser'
 const [listening,setListening]=useState(false)
 const [busy,setBusy]=useState(false)
 const [error,setError]=useState('')
 const [serverReady,setServerReady]=useState(false)
 const recognitionRef=useRef<any>(null)
 const recorderRef=useRef<{context:AudioContext;stream:MediaStream;node:ScriptProcessorNode;chunks:Float32Array[]}|null>(null)
 const supported=typeof window!=='undefined'&&(Boolean((window as any).SpeechRecognition||(window as any).webkitSpeechRecognition)
  ||Boolean(navigator.mediaDevices?.getUserMedia))

 useEffect(()=>{
  let alive=true
  void apiFetch('/api/voice/status').then(response=>response.json() as Promise<VoiceStatus>).then(status=>{
   if(alive)setServerReady(Boolean(status?.whisper?.available))
  }).catch(()=>{if(alive)setServerReady(false)})
  return ()=>{alive=false}
 },[])

 const stopRecording=useCallback(async()=>{
  const active=recorderRef.current
  recorderRef.current=null
  setListening(false)
  if(!active)return
  active.node.disconnect()
  active.stream.getTracks().forEach(track=>track.stop())
  await active.context.close().catch(()=>{})
  const total=active.chunks.reduce((sum,chunk)=>sum+chunk.length,0)
  if(total<1600)return
  const samples=new Float32Array(total)
  let offset=0
  for(const chunk of active.chunks){samples.set(chunk,offset);offset+=chunk.length}
  const wav=encodeWav(samples,active.context.sampleRate)
  setBusy(true)
  try{
   const response=await apiFetch('/api/voice/transcribe',{method:'POST',headers:{'Content-Type':'audio/wav'},body:wav})
   const payload=await response.json().catch(()=>({}))
   if(!response.ok)throw new Error(String(payload?.error||response.status))
   const text=String(payload?.text||'').trim()
   if(text)onText(text)
   else setError(locale==='ko'?'인식된 말이 없습니다.':'No speech recognised.')
  }catch(reason){
   setError(reason instanceof Error?reason.message:String(reason))
  }finally{
   setBusy(false)
  }
 },[locale,onText])

 const startRecording=useCallback(async()=>{
  const stream=await navigator.mediaDevices.getUserMedia({audio:true})
  const context=new AudioContext()
  const source=context.createMediaStreamSource(stream)
  const node=context.createScriptProcessor(4096,1,1)
  const chunks:Float32Array[]=[]
  node.onaudioprocess=event=>{chunks.push(new Float32Array(event.inputBuffer.getChannelData(0)))}
  source.connect(node)
  node.connect(context.destination)
  recorderRef.current={context,stream,node,chunks}
  setListening(true)
 },[])

 const startBrowserRecognition=useCallback(()=>{
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
 },[locale,onText])

 const toggle=useCallback(()=>{
  if(listening){
   if(recorderRef.current)void stopRecording()
   else{try{recognitionRef.current?.stop()}catch{/* already stopped */}}
   return
  }
  setError('')
  if(serverReady){
   void startRecording().catch(reason=>setError(reason instanceof Error?reason.message:String(reason)))
  }else{
   startBrowserRecognition()
  }
 },[listening,serverReady,startRecording,startBrowserRecognition,stopRecording])

 useEffect(()=>()=>{try{recognitionRef.current?.stop()}catch{/* already stopped */}},[])

 if(!supported)return null
 const active=listening||busy
 return <Button type="button" variant="ghost" size="icon"
  className={'!h-7 !w-7 !border-0 !bg-transparent '+(active?'!text-rose-600':'!text-[#5C544A]')}
  title={error||(error?unsupportedLabel:(listening?listeningLabel:label))}
  aria-label={listening?listeningLabel:label}
  aria-pressed={active}
  disabled={disabled||busy}
  onClick={toggle}><Mic size={16}/></Button>
}

function encodeWav(samples: Float32Array, inputRate: number): Blob {
 const targetRate=16000
 const ratio=inputRate/targetRate
 const length=Math.max(1,Math.floor(samples.length/ratio))
 const buffer=new ArrayBuffer(44+length*2)
 const view=new DataView(buffer)
 const text=(offset:number,value:string)=>{for(let index=0;index<value.length;index+=1)view.setUint8(offset+index,value.charCodeAt(index))}
 text(0,'RIFF'); view.setUint32(4,36+length*2,true); text(8,'WAVE'); text(12,'fmt ')
 view.setUint32(16,16,true); view.setUint16(20,1,true); view.setUint16(22,1,true)
 view.setUint32(24,targetRate,true); view.setUint32(28,targetRate*2,true)
 view.setUint16(32,2,true); view.setUint16(34,16,true); text(36,'data'); view.setUint32(40,length*2,true)
 let offset=44
 for(let index=0;index<length;index+=1){
  const sample=samples[Math.floor(index*ratio)]||0
  view.setInt16(offset,Math.max(-1,Math.min(1,sample))*0x7fff,true)
  offset+=2
 }
 return new Blob([view],{type:'audio/wav'})
}
