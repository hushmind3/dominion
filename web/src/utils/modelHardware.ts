// Shared pure estimates for the Hugging Face model browser.
// Do not add GPU VRAM to system RAM: MLX uses unified memory.
export type ModelCandidate={id:string;format:'gguf'|'mlx';bytes:number}
export type MemoryFit='comfortable'|'tight'|'insufficient'|'unknown'
export type RuntimeFailure='memory'|'file'|'engine'|'other'
export const MEMORY_HEADROOM=1.3

export function requiredMemory(bytes:number):number{
 return Number.isFinite(bytes)&&bytes>0?Math.ceil(bytes*MEMORY_HEADROOM):0
}

export function memoryFit(bytes:number,available:number):MemoryFit{
 if(!Number.isFinite(available)||available<=0||requiredMemory(bytes)===0)return 'unknown'
 return available>=requiredMemory(bytes)?'comfortable':available>=bytes?'tight':'insufficient'
}

export function selectRecommended<T extends ModelCandidate>(options:readonly T[],availableRam:number,availableGpu:number):{option:T|null;basis:'gpu'|'ram'|null}{
 const eligible=options.filter(o=>o.bytes>0&&Number.isFinite(o.bytes))
 const gpu=eligible.filter(o=>o.format==='gguf'&&memoryFit(o.bytes,availableGpu)==='comfortable').sort((a,b)=>b.bytes-a.bytes)[0]
 if(gpu)return {option:gpu,basis:'gpu'}
 const ram=eligible.filter(o=>memoryFit(o.bytes,availableRam)==='comfortable').sort((a,b)=>b.bytes-a.bytes)[0]
 return {option:ram||null,basis:ram?'ram':null}
}

export function selectSmaller<T extends ModelCandidate>(options:readonly T[],currentBytes:number,availableRam:number,availableGpu:number):{option:T|null;fits:boolean}{
 const smaller=options.filter(o=>o.bytes>0&&o.bytes<currentBytes)
 const best=selectRecommended(smaller,availableRam,availableGpu).option
 if(best)return {option:best,fits:true}
 return {option:smaller.sort((a,b)=>a.bytes-b.bytes)[0]||null,fits:false}
}

export function classifyRuntimeFailure(error:string):RuntimeFailure{
 const message=error.toLowerCase()
 if(/out.of.memory|\boom\b|memory|메모리|allocat|cuda.*alloc|not enough resources/i.test(message))return 'memory'
 if(/not.found|no.such.file|missing.file|corrupt|invalid.gguf|checksum|파일.*없|파일.*찾을 수 없|손상|불완전|safetensors|tokenizer|찾을 수 없습니다.*모델|모델.*찾을 수 없습니다/i.test(message))return 'file'
 if(/backend|engine|executable|llama[.-]?(?:cpp|server)|mlx.*실행기|not.installed|설치.*안|spawn|실행기.*필요|실행기.*없/i.test(message))return 'engine'
 return 'other'
}
