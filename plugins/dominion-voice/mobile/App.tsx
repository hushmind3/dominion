import React,{useEffect,useRef,useState} from 'react';
import {AppState, SafeAreaView, View, Text, TextInput, Pressable, StyleSheet, useColorScheme} from 'react-native';
import {AudioSession,registerGlobals} from '@livekit/react-native';
import {Room,RoomEvent,ConnectionState} from 'livekit-client';

registerGlobals();
type Pairing={url:string;token:string;room:string;expiresAt:number};
function parse(raw:string):Pairing{
  const p=JSON.parse(raw);
  if(!p||typeof p.url!=='string'||!p.url.startsWith('wss://')||
    typeof p.token!=='string'||p.token.split('.').length!==3||
    typeof p.room!=='string'||!/^dominion-[a-f0-9]{32}$/.test(p.room)||
    !Number.isFinite(p.expiresAt)||p.expiresAt<=Date.now()/1000)throw new Error('연결 정보가 올바르지 않거나 만료되었습니다.');
  return p;
}
export default function App(){
  const dark=useColorScheme()==='dark',ink=dark?'#f1f4fa':'#182233';
  const [raw,setRaw]=useState(''),[status,setStatus]=useState('연결 대기'),[error,setError]=useState(''),[mic,setMic]=useState(false),[connected,setConnected]=useState(false),[busy,setBusy]=useState(false);
  const roomRef=useRef<Room|null>(null),generation=useRef(0),inFlight=useRef(false);
  async function disconnect(){
    generation.current++;
    const room=roomRef.current;roomRef.current=null;
    setConnected(false);setMic(false);setStatus('연결 종료');
    if(room){try{await room.disconnect()}catch{}}
    try{await AudioSession.stopAudioSession()}catch{}
  }
  useEffect(()=>{
    const sub=AppState.addEventListener('change',state=>{if(state==='background')void disconnect()});
    return()=>{sub.remove();void disconnect()};
  },[]);
  async function connect(){
    if(inFlight.current||roomRef.current)return;
    inFlight.current=true;setBusy(true);setError('');
    const id=++generation.current;
    try{
      const pair=parse(raw.trim());
      setStatus('음성 연결 중');
      await AudioSession.startAudioSession();
      if(id!==generation.current){await AudioSession.stopAudioSession();return}
      const room=new Room();
      roomRef.current=room;
      room.on(RoomEvent.ConnectionStateChanged,state=>{
        if(roomRef.current!==room)return;
        setConnected(state===ConnectionState.Connected);
        setStatus(state===ConnectionState.Connected?'연결됨 · 마이크를 켜세요':state===ConnectionState.Reconnecting?'재연결 중':'연결 상태: '+state);
      });
      room.on(RoomEvent.Disconnected,()=>{if(roomRef.current===room)void disconnect()});
      await room.connect(pair.url,pair.token);
      if(id!==generation.current||roomRef.current!==room){await room.disconnect();return}
      if(room.name!==pair.room)throw new Error('연결 방이 일치하지 않습니다.');
      setConnected(true);setStatus('연결됨 · 마이크를 켜세요');setRaw('');
    }catch(e){if(id===generation.current){await disconnect();setError('연결 실패. 접속 정보, 네트워크, PC 음성 비서 상태를 확인하세요.');}}
    finally{inFlight.current=false;setBusy(false)}
  }
  async function toggle(){
    const room=roomRef.current;if(!room||inFlight.current)return;
    inFlight.current=true;setBusy(true);
    try{await room.localParticipant.setMicrophoneEnabled(!mic);if(roomRef.current===room){setMic(!mic);setStatus(mic?'마이크 꺼짐':'음성 대화 중');}}catch{setError('마이크 권한 또는 연결을 확인하세요.')}
    finally{inFlight.current=false;setBusy(false)}
  }
  return <SafeAreaView style={[styles.page,{backgroundColor:dark?'#111827':'#eef2f8'}]}><View style={styles.body}>
    <Text style={[styles.title,{color:ink}]}>DOMINION Voice</Text><Text style={[styles.sub,{color:ink}]}>PC의 도미니온과 연결하는 개인 음성 비서</Text>
    <View style={[styles.card,{backgroundColor:dark?'#202b3c':'white'}]}>
    <Text accessibilityLiveRegion="polite" style={[styles.status,{color:ink}]}>{status}</Text>
    {!connected&&<><Text style={{color:ink}}>PC 음성 비서에서 만든 임시 연결 정보를 붙여넣으세요.</Text><TextInput accessibilityLabel="임시 연결 정보" value={raw} onChangeText={setRaw} autoCapitalize="none" autoCorrect={false} secureTextEntry editable={!busy} placeholder="연결 정보" placeholderTextColor="#8792a2" style={[styles.input,{color:ink}]}/><Pressable accessibilityRole="button" disabled={!raw.trim()||busy} onPress={()=>void connect()} style={[styles.button,{opacity:!raw.trim()||busy ? 0.5 : 1}]}><Text style={styles.buttonText}>연결</Text></Pressable></>}
    {connected&&<Pressable accessibilityRole="button" disabled={busy} onPress={()=>void toggle()} style={[styles.button,{backgroundColor:mic?'#bd4141':'#285bca'}]}><Text style={styles.buttonText}>{mic?'마이크 끄기':'마이크 켜기'}</Text></Pressable>}
    {(connected||busy)&&<Pressable accessibilityRole="button" onPress={()=>void disconnect()} style={styles.secondary}><Text style={{color:ink}}>연결 종료</Text></Pressable>}
    {!!error&&<Text accessibilityRole="alert" style={{color:'#e46b60'}}>{error}</Text>}
    </View><Text style={[styles.sub,{color:ink}]}>연결 정보는 앱에 저장하지 않습니다. 앱을 백그라운드로 이동하면 음성 연결을 종료합니다.</Text>
    </View></SafeAreaView>
}
const styles=StyleSheet.create({page:{flex:1},body:{padding:24,gap:16},title:{fontSize:30,fontWeight:'700'},sub:{fontSize:14,lineHeight:22},card:{borderRadius:20,padding:20,gap:14},status:{fontSize:20,fontWeight:'600'},input:{borderWidth:1,borderColor:'#8994a6',borderRadius:12,padding:14},button:{padding:16,backgroundColor:'#285bca',borderRadius:12,alignItems:'center'},buttonText:{color:'#fff',fontWeight:'700',fontSize:16},secondary:{padding:14,alignItems:'center'}});
