export const MAX_CHAT_FILE_BYTES=8*1024*1024
export const MAX_CHAT_TOTAL_BYTES=24*1024*1024
export const MAX_CHAT_FILES=8

export type ChatFileDraft={file:File;uploadedId?:string;uploadedSession?:string}
export function mergeChatFiles(existing:ChatFileDraft[],incoming:File[]):ChatFileDraft[]{
 const unique=new Map<string,ChatFileDraft>()
 for(const entry of existing){
  const {name,size,lastModified}=entry.file
  unique.set(name+'\0'+size+'\0'+lastModified,entry)
 }
 for(const file of incoming){
  const key=file.name+'\0'+file.size+'\0'+file.lastModified
  if(!unique.has(key))unique.set(key,{file})
 }
 const result=[...unique.values()]
 if(result.length>MAX_CHAT_FILES)throw Error('첨부 파일은 최대 8개입니다.')
 if(result.some(item=>item.file.size>MAX_CHAT_FILE_BYTES))throw Error('첨부 파일 1개는 최대 8MiB입니다.')
 if(result.reduce((sum,item)=>sum+item.file.size,0)>MAX_CHAT_TOTAL_BYTES)
  throw Error('첨부 파일 합계는 최대 24MiB입니다.')
 return result
}
export async function base64File(file:File):Promise<string>{
 return new Promise<string>((resolve,reject)=>{
  const reader=new FileReader()
  reader.onerror=()=>reject(Error('첨부 파일을 읽을 수 없습니다.'))
  reader.onload=()=>{
   const source=String(reader.result||'')
   const comma=source.indexOf(',')
   if(comma<0){reject(Error('첨부 파일 데이터를 읽을 수 없습니다.'));return}
   resolve(source.slice(comma+1))
  }
  reader.readAsDataURL(file)
 })
}
