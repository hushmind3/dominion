import type {Provider} from '../../api'
export type PresetCategory='free'|'cloud'|'local'
export type ProviderPreset={name:string;baseUrl:string;kind:string;local:boolean;integration?:'freellmapi'|'opencode-go';free?:boolean;requestsPerMinute?:number;category:PresetCategory;auth?:'key'|'none';note?:string}
export type CliCredential={id:string;name:string;available:boolean;supported:boolean;connected?:boolean;importAvailable?:boolean;accountId?:string;model?:string;hint?:string;loginSupported?:boolean;loginMethod?:'browser'|'browser-cli'|'api-key'}
export type CatalogProvider={id:string;name:string;api:string;models:number;env?:string[];doc?:string}
export type CatalogResponse={providers:CatalogProvider[];cached?:boolean;error?:string}
export type FreeCatalogModel={id:string;free?:boolean|null;freeStatus?:string;freeSource?:string}
export type FreeCatalogProvider={providerId:string;providerName:string;reachable:boolean;error?:string;models:FreeCatalogModel[]}
export type FreeCatalog={status?:string;updatedAt?:string;providers:FreeCatalogProvider[];counts?:{free:number;paid:number;unknown:number}}
export type Tab='providers'|'cloud'|'connect'|'free'|'catalog'|'local'|'huggingface'|'oauth'

export const opencodeGoPreset:ProviderPreset={name:'OpenCode Go',baseUrl:'https://opencode.ai/inference/go/openai/v1',kind:'openai',local:false,category:'cloud',integration:'opencode-go'}

export const providerPresets:ProviderPreset[]=[
 // Free tiers and local gateways. Keys are still required where the provider asks for one.
 {name:'FreeLLMAPI',baseUrl:'http://127.0.0.1:31415/v1',kind:'openai',local:false,integration:'freellmapi',free:true,category:'free',auth:'none'},
 {name:'OVHcloud (무료)',baseUrl:'https://oai.endpoints.kepler.ai.cloud.ovh.net/v1',kind:'openai',local:false,free:true,requestsPerMinute:2,category:'free'},
 {name:'Groq',baseUrl:'https://api.groq.com/openai/v1',kind:'openai',local:false,free:true,category:'free'},
 {name:'z.ai',baseUrl:'https://api.z.ai/api/paas/v4',kind:'openai',local:false,free:true,category:'free'},
 {name:'Cloudflare Workers AI',baseUrl:'https://api.cloudflare.com/client/v4/accounts/ACCOUNT_ID/ai/v1',kind:'openai',local:false,free:true,category:'free'},
 {name:'OpenRouter',baseUrl:'https://openrouter.ai/api/v1',kind:'openai',local:false,free:true,category:'free'},
 {name:'Vercel AI Gateway',baseUrl:'https://ai-gateway.vercel.sh/v1',kind:'openai',local:false,free:true,category:'free'},
 {name:'Chutes',baseUrl:'https://llm.chutes.ai/v1',kind:'openai',local:false,category:'cloud'},
 {name:'Cerebras',baseUrl:'https://api.cerebras.ai/v1',kind:'openai',local:false,category:'cloud'},
 {name:'SambaNova',baseUrl:'https://api.sambanova.ai/v1',kind:'openai',local:false,free:true,category:'free'},
 {name:'Hyperbolic',baseUrl:'https://api.hyperbolic.ai/inference/v1',kind:'openai',local:false,category:'cloud'},
 {name:'Nebius',baseUrl:'https://api.tokenfactory.nebius.com/v1',kind:'openai',local:false,category:'cloud'},
 {name:'Novita',baseUrl:'https://api.novita.ai/v3/openai',kind:'openai',local:false,category:'cloud'},
 // Updated against official provider billing/API docs in November 2026.
 // Chutes ended its free plan; Cerebras/Nebius have limited trial credits;
 // Hyperbolic Flux uses a new paid endpoint and requires new sk_live_ keys;
 // Novita's credits are conditional, not a recurring no-cost quota.
 // Metered cloud APIs.
 {name:'Google Gemini',baseUrl:'https://generativelanguage.googleapis.com/v1beta/openai',kind:'openai',local:false,category:'cloud'},
 {name:'Anthropic',baseUrl:'https://api.anthropic.com/v1',kind:'anthropic',local:false,category:'cloud'},
 {name:'OpenAI',baseUrl:'https://api.openai.com/v1',kind:'openai',local:false,category:'cloud'},
 {name:'DeepSeek',baseUrl:'https://api.deepseek.com/v1',kind:'openai',local:false,category:'cloud'},
 {name:'Mistral',baseUrl:'https://api.mistral.ai/v1',kind:'openai',local:false,category:'cloud'},
 {name:'xAI',baseUrl:'https://api.x.ai/v1',kind:'openai',local:false,category:'cloud'},
 {name:'Together',baseUrl:'https://api.together.xyz/v1',kind:'openai',local:false,category:'cloud'},
 {name:'Fireworks',baseUrl:'https://api.fireworks.ai/inference/v1',kind:'openai',local:false,category:'cloud'},
 {name:'Perplexity',baseUrl:'https://api.perplexity.ai',kind:'openai',local:false,category:'cloud'},
 {name:'GitHub Models',baseUrl:'https://models.github.ai/inference',kind:'openai',local:false,category:'cloud'},
 {name:'Requesty',baseUrl:'https://router.requesty.ai/v1',kind:'openai',local:false,category:'cloud'},
 {name:'Reflection',baseUrl:'https://api.reflection.ai/openai/v1',kind:'openai',local:false,category:'cloud'},
 // Local runtimes and localhost proxies.
 {name:'Ollama',baseUrl:'http://127.0.0.1:11434/v1',kind:'ollama',local:true,category:'local',auth:'none'},
 {name:'LM Studio',baseUrl:'http://127.0.0.1:1234/v1',kind:'openai',local:true,category:'local',auth:'none'},
 {name:'llama.cpp',baseUrl:'http://127.0.0.1:8080/v1',kind:'openai',local:true,category:'local',auth:'none'},
 {name:'vLLM',baseUrl:'http://127.0.0.1:8000/v1',kind:'openai',local:true,category:'local',auth:'none'},
 {name:'MLX',baseUrl:'http://127.0.0.1:8081/v1',kind:'openai',local:true,category:'local',auth:'none'},
 {name:'LiteLLM',baseUrl:'http://127.0.0.1:4000/v1',kind:'openai',local:true,category:'local',auth:'none'},
 opencodeGoPreset
]

export const sameProvider=(p:Provider,preset:ProviderPreset)=>p.name===preset.name||p.baseUrl.replace(/\/$/,'')===preset.baseUrl.replace(/\/$/,'')
export const updatedProviderEndpoint=(p:Provider):string|undefined=>{
 const url=p.baseUrl.replace(/\/+$/,'')
 if(p.name==='Hyperbolic'&&url==='https://api.hyperbolic.xyz/v1')
  return 'https://api.hyperbolic.ai/inference/v1'
 if(p.name==='Nebius'&&url==='https://api.studio.nebius.ai/v1')
  return 'https://api.tokenfactory.nebius.com/v1'
 return undefined
}

