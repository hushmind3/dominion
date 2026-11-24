import React,{useEffect,useRef,useState} from 'react';
import {AppState,Platform,PermissionsAndroid,SafeAreaView,ScrollView,View,Text,TextInput,Pressable,StyleSheet,useColorScheme,KeyboardAvoidingView} from 'react-native';
import {ExpoSpeechRecognitionModule,useSpeechRecognitionEvent} from 'expo-speech-recognition';
import * as Speech from 'expo-speech';
type Link={url:string;token:string};
type Result={status:string;requestId?:string;question?:string;answer?:string;error?:string;remaining?:number};
function parseCode(raw:string){
 const m=raw.trim().match(/^(\d{1,3})\.(\d{1,3})\.(\d{1,3})\.(\d{1,3}):(\d{1,5})\/(\d{6})$/);
 if(!m)throw Error('PC 화면의 연결 코드 전체를 입력하세요.');
 const [a,b,c,d,port]=m.slice(1,6).map(Number);
 if([a,b,c,d].some(n=>n<0||n>255)||!((a===10)||(a===172&&b>=16&&b<=31)||(a===192&&b===168))||port<1||port>65535)throw Error('개인 Wi-Fi 주소만 연결할 수 있습니다.');
 return {url:'http://'+[a,b,c,d].join('.')+':'+port,code:m[6]};
}
async function api(url:string,path:string,body?:unknown,token?:string){
 const controller=new AbortController();const timer=setTimeout(()=>controller.abort(),10000);
 try{
  const r=await fetch(url+path,{method:body===undefined?'GET':'POST',headers:{...(body===undefined?{}:{'Content-Type':'application/json'}),...(token?{'Authorization':'Bearer '+token}:{})},...(body===undefined?{}:{body:JSON.stringify(body)}),signal:controller.signal,redirect:'error'});
  const data=await r.json();if(!r.ok)throw Error(data.error||'PC 연결 오류');return data;
 }finally{clearTimeout(timer)}
}
export default function PersonalChat({onAdvanced}:{onAdvanced:()=>void}){
 const dark=useColorScheme()==='dark',ink=dark?'#f1f4fa':'#172638';
 const [code,setCode]=useState(''),[link,setLink]=useState<Link|null>(null),[text,setText]=useState(''),[result,setResult]=useState<Result|null>(null),[err,setErr]=useState(''),[busy,setBusy]=useState(false),[listening,setListening]=useState(false),[speaking,setSpeaking]=useState(false),[readAloud,setReadAloud]=useState(true);
 const linkRef=useRef<Link|null>(null),generation=useRef(0),sending=useRef(false),spoken=useRef(''),recording=useRef(false),foreground=useRef(true);
 const pending=result?.status==='queued'||result?.status==='running'||result?.status==='awaiting_approval'||result?.status==='in_doubt';
 const speak=(answer:string)=>{
  if(!foreground.current||!answer.trim())return;
  void Speech.stop();setSpeaking(true);
  Speech.speak(answer.slice(0,12000),{language:'ko-KR',onDone:()=>setSpeaking(false),onStopped:()=>setSpeaking(false),onError:()=>{setSpeaking(false);setErr('휴대폰 음성 출력 설정을 확인하세요.')}});
 };
 const apply=(r:Result)=>{
  setResult(r);
  if(r.error)setErr(r.error);
  if(r.status==='completed'&&r.answer&&r.requestId&&spoken.current!==r.requestId){
   spoken.current=r.requestId;if(readAloud)speak(r.answer);
  }
 };
 useEffect(()=>{
  if(!link)return;let active=true,checking=false;
  const load=async()=>{if(checking||!foreground.current||sending.current)return;checking=true;try{const r=await api(link.url,'/status',undefined,link.token);if(active)apply(r)}catch(e){if(active)setErr((e as Error).message)}finally{checking=false}};
  void load();const timer=setInterval(()=>void load(),1500);return()=>{active=false;clearInterval(timer)};
 },[link,readAloud]);
 useEffect(()=>{
  const sub=AppState.addEventListener('change',s=>{foreground.current=s==='active';if(s!=='active'){recording.current=false;ExpoSpeechRecognitionModule.abort();void Speech.stop()}});
  return()=>{sub.remove();recording.current=false;ExpoSpeechRecognitionModule.abort();void Speech.stop()};
 },[]);
 const connect=async()=>{
  if(sending.current)return;sending.current=true;setBusy(true);setErr('');
  const g=++generation.current;
  try{
   const p=parseCode(code);
   if(Platform.OS==='android'&&Number(Platform.Version)>=37){
    const permission='android.permission.ACCESS_LOCAL_NETWORK' as Parameters<typeof PermissionsAndroid.request>[0];
    if(await PermissionsAndroid.request(permission)!==PermissionsAndroid.RESULTS.GRANTED)throw Error('로컬 네트워크 접근 권한을 허용하세요.');
   }
   const r=await api(p.url,'/pair',{code:p.code});
   if(typeof r.token!=='string'||!r.token)throw Error('PC 인증 응답이 올바르지 않습니다.');
   if(g!==generation.current)return;
   const l={url:p.url,token:r.token};linkRef.current=l;setLink(l);setCode('');setResult({status:'ready',remaining:r.remaining});spoken.current='';
  }catch(e){if(g===generation.current)setErr((e as Error).message==='Aborted'?'PC와 같은 Wi-Fi인지 확인하세요.':(e as Error).message)}
  finally{sending.current=false;setBusy(false)}
 };
 const send=async(value:string)=>{
  const l=linkRef.current;const msg=value.trim();if(!l||!msg||sending.current||pending)return;
  sending.current=true;setBusy(true);setErr('');setText(msg);void Speech.stop();
  const g=generation.current;
  try{
   const r=await api(l.url,'/ask',{requestId:Date.now().toString(36)+'-'+Math.random().toString(36).slice(2),text:msg},l.token);
   if(g===generation.current){setText('');apply(r)}
  }catch(e){if(g===generation.current)setErr((e as Error).message)}
  finally{sending.current=false;setBusy(false)}
 };
 useSpeechRecognitionEvent('start',()=>setListening(true));
 useSpeechRecognitionEvent('end',()=>{recording.current=false;setListening(false)});
 useSpeechRecognitionEvent('error',e=>{recording.current=false;setListening(false);if(e.error!=='aborted')setErr('음성 인식: '+e.message)});
 useSpeechRecognitionEvent('result',e=>{
  if(!recording.current||!foreground.current)return;
  const value=e.results[0]?.transcript||'';if(value)setText(value);
  if(e.isFinal&&value){recording.current=false;void send(value)}
 });
 const mic=async()=>{
  if(recording.current){ExpoSpeechRecognitionModule.stop();return}
  if(!linkRef.current||sending.current||pending)return;
  setErr('');void Speech.stop();
  try{const p=await ExpoSpeechRecognitionModule.requestPermissionsAsync();if(!p.granted){setErr('마이크 권한을 허용하세요.');return}
   if(!ExpoSpeechRecognitionModule.isRecognitionAvailable()){setErr('휴대폰 음성 인식을 사용할 수 없습니다. 시스템 음성 인식을 설정하거나 직접 입력하세요.');return}
   recording.current=true;ExpoSpeechRecognitionModule.start({lang:'ko-KR',interimResults:true,continuous:false,maxAlternatives:1});
  }catch(e){recording.current=false;setErr((e as Error).message)}
 };
 const disconnect=()=>{
  const l=linkRef.current;generation.current++;linkRef.current=null;setLink(null);setResult(null);setText('');setErr('');recording.current=false;ExpoSpeechRecognitionModule.abort();void Speech.stop();
  if(l)void api(l.url,'/disconnect',{},l.token).catch(()=>{});
 };
 const button=(label:string,fn:()=>void,disabled=false)=><Pressable accessibilityRole="button" disabled={disabled} onPress={fn} style={[styles.button,{opacity:disabled ? 0.45 : 1}]}><Text style={styles.white}>{label}</Text></Pressable>;
 return <SafeAreaView style={[styles.page,{backgroundColor:dark?'#101827':'#eff4fb'}]}><KeyboardAvoidingView style={{flex:1}} behavior={Platform.OS==='ios'?'padding':undefined}><ScrollView keyboardShouldPersistTaps="handled" contentContainerStyle={styles.body}>
  <Text style={[styles.title,{color:ink}]}>DOMINION Voice</Text><Text style={{color:ink}}>개인 음성 비서 · 0.3.0</Text>
  <View style={[styles.card,{backgroundColor:dark?'#202b3c':'white'}]}>
   <Text style={[styles.heading,{color:ink}]}>{link?'PC 연결됨':'간편 연결'}</Text>
   {!link?<><Text style={{color:ink}}>PC 도미니온 → 서비스 연결 → 플러그인 → 휴대폰 연결 켜기. 같은 Wi-Fi에서 표시된 코드를 입력하세요.</Text>
    <TextInput accessibilityLabel="PC 연결 코드" autoCapitalize="none" autoCorrect={false} value={code} onChangeText={setCode} placeholder="192.168.0.10:4323/123456" placeholderTextColor="#8291a3" style={[styles.input,{color:ink}]} editable={!busy}/>
    {button(busy?'연결 중…':'연결',()=>void connect(),busy||!code.trim())}
    <Text style={{color:ink,fontSize:13}}>별도 음성 API 키는 필요 없습니다. 개인 Wi-Fi에서만 사용하세요.</Text></>:<>
    <Text style={{color:ink}}>남은 대화 {result?.remaining??'–'}회 · {pending?'도미니온이 답변 중':speaking?'답변 읽는 중':'대화 준비 완료'}</Text>
    <Pressable accessibilityRole="button" disabled={busy||!!pending} onPress={()=>void mic()} style={[styles.mic,{backgroundColor:listening?'#bc4049':'#285bca',opacity:busy||pending ? 0.5 : 1}]}><Text style={[styles.white,{fontSize:20}]}>{listening?'듣는 중 · 중지':'🎙 말하기'}</Text></Pressable>
    <TextInput accessibilityLabel="직접 입력" multiline value={text} onChangeText={setText} placeholder="직접 입력도 가능합니다" placeholderTextColor="#8291a3" style={[styles.input,{color:ink,minHeight:70}]} editable={!busy&&!pending}/>
    {button('보내기',()=>void send(text),busy||!!pending||!text.trim())}
    {result?.question&&<Text style={{color:ink,fontWeight:'600'}}>{result.question}</Text>}
    {result?.answer&&<Text selectable style={{color:ink,fontSize:17,lineHeight:26}}>{result.answer}</Text>}
    <View style={styles.row}>{result?.answer&&button(speaking?'읽기 중지':'다시 듣기',()=>speaking?void Speech.stop():speak(result.answer||''))}
    <Pressable onPress={()=>{setReadAloud(!readAloud);if(readAloud)void Speech.stop()}}><Text style={{color:'#3976d7'}}>{readAloud?'자동 읽기 끄기':'자동 읽기 켜기'}</Text></Pressable></View>
    <Pressable onPress={disconnect}><Text style={{color:'#3976d7',textAlign:'center',padding:10}}>연결 종료</Text></Pressable>
   </>}
   {!!err&&<Text accessibilityRole="alert" style={{color:'#df6860'}}>{err}</Text>}
  </View>
  {!link&&<Pressable onPress={onAdvanced} disabled={busy}><Text style={{color:'#3976d7',textAlign:'center'}}>고급 실시간 음성 / LiveKit</Text></Pressable>}
 </ScrollView></KeyboardAvoidingView></SafeAreaView>
}
const styles=StyleSheet.create({page:{flex:1},body:{padding:24,gap:16},title:{fontSize:30,fontWeight:'700'},heading:{fontSize:22,fontWeight:'700'},card:{borderRadius:20,padding:20,gap:14},input:{borderWidth:1,borderColor:'#8392a5',borderRadius:12,padding:14},button:{padding:14,backgroundColor:'#285bca',borderRadius:12,alignItems:'center'},white:{color:'white',fontSize:16,fontWeight:'700'},mic:{padding:28,borderRadius:18,alignItems:'center'},row:{flexDirection:'row',alignItems:'center',flexWrap:'wrap',gap:12}});
