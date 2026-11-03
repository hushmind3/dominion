// DOMINION's channel registry is independent of engine and UI.
// A catalog entry describes capabilities and installation requirements;
// it never claims a connection is operational unless a driver exists.
const catalog = [
 {id:'telegram',name:'Telegram',category:'messaging',mode:'polling',driver:'telegram',docs:'https://core.telegram.org/bots/api',description:'Bot API · DM과 그룹 · Long Polling',features:['dm','groups','replies'],fields:[{key:'token',label:'Bot Token',type:'secret',required:true}],instructions:['BotFather에서 봇 생성 및 토큰 발급','봇을 그룹에 추가하고 메시지 수신 권한 설정','저장 후 DOMINION에서 상태 진단']},
 {id:'discord',name:'Discord',category:'community',mode:'gateway',driver:'discord',docs:'https://discord.com/developers/docs/topics/gateway',description:'Gateway WebSocket · 서버와 DM',features:['dm','groups','replies'],fields:[{key:'token',label:'Bot Token',type:'secret',required:true}],instructions:['Discord Developer Portal에서 Bot 생성','Message Content Intent를 활성화','OAuth2 초대 URL로 서버에 봇 추가']},
 {id:'slack',name:'Slack',category:'workplace',mode:'socket',driver:'slack',docs:'https://docs.slack.dev/apis/events-api/using-socket-mode/',description:'Socket Mode · 공개 웹훅 불필요',features:['dm','groups','replies'],fields:[{key:'token',label:'Bot Token (xoxb-)',type:'secret',required:true},{key:'appToken',label:'App-level Token (xapp-)',type:'secret',required:true}],instructions:['Slack App 생성 후 Socket Mode 켜기','App Token에 connections:write 스코프 부여','Bot Token에 chat:write 및 필요한 이벤트 구독 스코프 설정']},
 {id:'matrix',name:'Matrix',category:'decentralized',mode:'polling',driver:'matrix',docs:'https://spec.matrix.org/latest/client-server-api/',description:'Client-Server API · Matrix 홈서버와 룸',features:['dm','groups','replies'],fields:[{key:'homeserver',label:'Homeserver URL',type:'url',required:true,placeholder:'https://matrix.example.org'},{key:'userId',label:'Bot User ID',type:'text',required:true,placeholder:'@bot:example.org'},{key:'token',label:'Access Token',type:'secret',required:true}],instructions:['Matrix 계정 또는 봇 계정을 생성','홈서버 주소·User ID·Access Token 등록','E2EE 룸은 별도 암호화 브리지가 필요합니다']},
 {id:'whatsapp',name:'WhatsApp Business',category:'messaging',mode:'webhook',driver:'whatsapp',docs:'https://developers.facebook.com/docs/whatsapp/cloud-api/',description:'Meta Cloud API · 공식 Business 웹훅',features:['dm','replies'],fields:[{key:'token',label:'System User Access Token',type:'secret',required:true},{key:'appSecret',label:'Meta App Secret',type:'secret',required:true},{key:'verifyToken',label:'Webhook Verify Token',type:'secret',required:true},{key:'phoneNumberId',label:'Phone Number ID',type:'text',required:true},{key:'graphVersion',label:'Graph API 버전',type:'text',required:true,placeholder:'v23.0'}],instructions:['Meta Developer App에 WhatsApp 제품 설정','WhatsApp 전화번호 ID 및 Access Token 등록','공개 HTTPS Webhook URL을 설정하고 messages 이벤트 구독'],requiresPublicUrl:true},
 {id:'line',name:'LINE',category:'messaging',mode:'webhook',driver:'line',docs:'https://developers.line.biz/en/docs/messaging-api/overview/',description:'Messaging API · 공식 웹훅',features:['dm','groups','replies'],fields:[{key:'token',label:'Channel Access Token',type:'secret',required:true},{key:'channelSecret',label:'Channel Secret',type:'secret',required:true}],instructions:['LINE Developers에서 Messaging API 채널 생성','채널 접근 토큰과 Secret 등록','공개 HTTPS Webhook URL을 채널 설정에 입력'],requiresPublicUrl:true},
 {id:'signal',name:'Signal',category:'messaging',mode:'gateway',driver:'signal',docs:'https://github.com/bbernhard/signal-cli-rest-api',description:'signal-cli REST bridge · 별도 실행기 필요',features:['dm','groups','replies'],fields:[{key:'bridgeUrl',label:'signal-cli REST URL',type:'url',required:true,placeholder:'http://127.0.0.1:8080'},{key:'phoneNumber',label:'Signal 계정 전화번호',type:'text',required:true,placeholder:'+821012345678'},{key:'token',label:'Bridge Bearer Token (선택)',type:'secret'}],instructions:['signal-cli-rest-api 설치 및 Signal 계정 연결','REST 또는 JSON-RPC 모드를 시작','/v1/receive WebSocket 사용 가능 여부 확인']},
 {id:'googlechat',name:'Google Chat',category:'workplace',mode:'webhook',driver:null,docs:'https://developers.google.com/workspace/chat/overview',description:'Google Workspace Chat App · 추가 구현 예정',features:['dm','groups'],fields:[],instructions:['Google Cloud에서 Chat App 구성','검증된 요청 인증과 API 권한 위임 어댑터 필요'],status:'planned'},
 {id:'teams',name:'Microsoft Teams',category:'workplace',mode:'webhook',driver:null,docs:'https://learn.microsoft.com/en-us/microsoftteams/platform/bots/what-are-bots',description:'Teams Bot Framework · 추가 구현 예정',features:['dm','groups'],fields:[],instructions:['Azure Bot 등록과 Microsoft Entra 앱 권한 필요','Bot Framework JWT 검증·토큰 갱신 후 연결 가능'],status:'planned'},
 {id:'mattermost',name:'Mattermost',category:'workplace',mode:'gateway',driver:null,docs:'https://developers.mattermost.com/integrate/reference/websocket-api/',description:'Self-hosted Chat · 추가 구현 예정',features:['dm','groups'],fields:[],status:'planned'},
 {id:'zulip',name:'Zulip',category:'workplace',mode:'polling',driver:null,docs:'https://zulip.com/api/',description:'Streams / Topics · 추가 구현 예정',features:['dm','groups'],fields:[],status:'planned'},
 {id:'rocketchat',name:'Rocket.Chat',category:'workplace',mode:'gateway',driver:null,docs:'https://developer.rocket.chat/apidocs',description:'REST / Realtime · 추가 구현 예정',features:['dm','groups'],fields:[],status:'planned'},
 {id:'irc',name:'IRC',category:'community',mode:'socket',driver:null,docs:'https://ircv3.net/',description:'IRCv3 · 추가 구현 예정',features:['groups'],fields:[],status:'planned'},
 {id:'twitch',name:'Twitch Chat',category:'community',mode:'gateway',driver:null,docs:'https://dev.twitch.tv/docs/chat/',description:'Live chat · 추가 구현 예정',features:['groups'],fields:[],status:'planned'},
 {id:'imessage',name:'iMessage',category:'messaging',mode:'gateway',driver:null,docs:'https://docs.openclaw.ai/channels/imessage',description:'macOS 전용 브리지 검토',features:['dm','groups'],fields:[],status:'planned'},
 {id:'kakaotalk',name:'KakaoTalk Channel',category:'messaging',mode:'webhook',driver:null,docs:'https://i.kakao.com/docs',description:'카카오 i 오픈빌더 스킬 서버 검토',features:['dm'],fields:[],status:'planned'}
];
export function channelCatalog(){return catalog.map(x=>({...x,fields:x.fields.map(f=>({...f})),available:Boolean(x.driver),status:x.driver?'driver':'planned'}));}
export function channelDefinition(id){return catalog.find(x=>x.id===id)||null;}
export function validateChannelConfiguration(kind,record,credentials){
 const entry=channelDefinition(kind);
 if(!entry)throw Error('Unknown channel type: '+kind);
 if(!entry.driver)throw Error(kind+' is a catalog-only channel, not an installed driver');
 const missing=entry.fields.filter(f=>f.required&&!(f.type==='secret'?credentials[f.key]:record[f.key])).map(f=>f.label);
 if(missing.length)throw Error('Missing configuration: '+missing.join(', '));
 for(const f of entry.fields.filter(f=>f.type==='url'&&record[f.key])){
  const u=new URL(record[f.key]);
  if(!['https:','http:'].includes(u.protocol))throw Error(f.label+' must use HTTP(S)');
 }
 if(kind==='whatsapp'&&!/^v\d+\.\d+$/.test(record.graphVersion||''))throw Error('Graph API version must match vXX.X');
 return true;
}
