import {useState,type ReactNode} from 'react'
import {Check,Copy} from 'lucide-react'

// Text-only Markdown; arbitrary HTML never enters the DOM.
// Only absolute http(s) / mailto links are interactive.
const inlineRe=/(\x60([^\x60\n]+)\x60)|(\[([^\]\n]+)\]\((https?:\/\/[^\s)]+|mailto:[^\s)]+)\))|(\*\*([^*\n]+)\*\*)|(~~([^~\n]+)~~)|(\*([^*\n]+)\*)/g
function inline(s:string):ReactNode[]{
 const nodes:ReactNode[]=[];let last=0
 for(const match of s.matchAll(inlineRe)){
  const at=match.index
  if(at>last)nodes.push(s.slice(last,at))
  if(match[2]!==undefined)nodes.push(<code key={at} className="md-inline-code">{match[2]}</code>)
  else if(match[4]!==undefined)nodes.push(<a key={at} href={match[5]} rel="noopener noreferrer" target="_blank">{match[4]}</a>)
  else if(match[7]!==undefined)nodes.push(<strong key={at}>{match[7]}</strong>)
  else if(match[9]!==undefined)nodes.push(<del key={at}>{match[9]}</del>)
  else nodes.push(<em key={at}>{match[11]}</em>)
  last=at+match[0].length
 }
 if(last<s.length)nodes.push(s.slice(last))
 return nodes
}
function CodePanel({code,lang}:{code:string;lang:string}){
 const [copied,setCopied]=useState(false)
 const copy=async()=>{try{await navigator.clipboard.writeText(code);setCopied(true)}catch{}}
 return <div className="md-code-panel">
  <div className="md-code-bar"><span>{lang||'텍스트'}</span><button type="button" onClick={()=>void copy()} aria-label="코드 복사">{copied?<Check size={13}/>:<Copy size={13}/>} {copied?'복사됨':'복사'}</button></div>
  <pre><code>{code}</code></pre>
 </div>
}
const fenceRe=/^ {0,3}(\x60{3,}|~{3,})(.*)$/
const headingRe=/^ {0,3}(#{1,6})\s+(.+)$/
const listRe=/^ {0,3}([-+*]|\d+\.)\s+(.+)$/
const hrRe=/^ {0,3}(?:-{3,}|\*{3,}|_{3,})\s*$/
const cells=(s:string)=>s.trim().replace(/^\|/,'').replace(/\|$/,'').split('|').map(x=>x.trim())
const isSep=(s:string)=>s.includes('|')&&cells(s).length>=2&&cells(s).every(x=>/^:?-{3,}:?$/.test(x))
const block=(s:string,next:string|undefined)=>Boolean(fenceRe.test(s)||headingRe.test(s)||listRe.test(s)||hrRe.test(s)||/^ {0,3}>\s?/.test(s)||(next!==undefined&&s.includes('|')&&isSep(next)))

export function MarkdownMessage({text}:{text:string}){
 // Huge tool outputs remain intact without triggering a heavy Markdown parse.
 if(text.length>180000)return <div className="md-message md-message-raw">{text}</div>
 const lines=text.replace(/\r\n?/g,'\n').split('\n')
 const out:ReactNode[]=[];let i=0
 while(i<lines.length){
  const line=lines[i]
  if(!line.trim()){i++;continue}
  const fenced=fenceRe.exec(line)
  if(fenced){
   const char=fenced[1][0],min=fenced[1].length,lang=fenced[2].trim().split(/\s+/)[0].replace(/[^\w+.-]/g,'').slice(0,30)
   const body:string[]=[];i++
   while(i<lines.length){
    const candidate=lines[i].trim()
    if(candidate.length>=min&&[...candidate].every(x=>x===char)){i++;break}
    body.push(lines[i++])
   }
   out.push(<CodePanel key={out.length} code={body.join('\n')} lang={lang}/>)
   continue
  }
  const heading=headingRe.exec(line)
  if(heading){
   out.push(heading[1].length<=2?<h2 key={out.length}>{inline(heading[2])}</h2>:<h3 key={out.length}>{inline(heading[2])}</h3>)
   i++;continue
  }
  if(hrRe.test(line)){out.push(<hr key={out.length}/>);i++;continue}
  if(i+1<lines.length&&line.includes('|')&&isSep(lines[i+1])){
   const head=cells(line),rows:string[][]=[];i+=2
   while(i<lines.length&&lines[i].trim()&&lines[i].includes('|')&&rows.length<500)rows.push(cells(lines[i++]))
   out.push(<div key={out.length} className="md-table-scroll"><table><thead><tr>{head.map((cell,j)=><th key={j}>{inline(cell)}</th>)}</tr></thead><tbody>{rows.map((row,j)=><tr key={j}>{head.map((_,k)=><td key={k}>{inline(row[k]||'')}</td>)}</tr>)}</tbody></table></div>)
   continue
  }
  const entry=listRe.exec(line)
  if(entry){
   const ordered=/^\d+\.$/.test(entry[1]),items:string[]=[],start=Number.parseInt(entry[1],10)||1
   while(i<lines.length){
    const current=listRe.exec(lines[i])
    if(!current||/^\d+\.$/.test(current[1])!==ordered)break
    items.push(current[2]);i++
    while(i<lines.length&&/^ {2,}\S/.test(lines[i])&&!listRe.test(lines[i]))items[items.length-1]+=' '+lines[i++].trim()
   }
   const children=items.map((value,j)=><li key={j}>{inline(value)}</li>)
   out.push(ordered?<ol key={out.length} start={start}>{children}</ol>:<ul key={out.length}>{children}</ul>)
   continue
  }
  if(/^ {0,3}>\s?/.test(line)){
   const quoted:string[]=[]
   while(i<lines.length&&/^ {0,3}>\s?/.test(lines[i]))quoted.push(lines[i++].replace(/^ {0,3}>\s?/,''))
   out.push(<blockquote key={out.length}>{quoted.map((value,j)=><p key={j}>{inline(value)}</p>)}</blockquote>)
   continue
  }
  const paragraph:string[]=[]
  while(i<lines.length&&lines[i].trim()){
   if(paragraph.length&&block(lines[i],lines[i+1]))break
   paragraph.push(lines[i++].trim())
  }
  out.push(<p key={out.length}>{paragraph.map((value,j)=><span key={j}>{j>0&&<br/>}{inline(value)}</span>)}</p>)
 }
 return <div className="md-message">{out}</div>
}
