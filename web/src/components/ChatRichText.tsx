import {createElement, type ReactNode} from 'react'

// Safe, dependency-free display of common AI Markdown. Never render model HTML.
// Streamed unfinished code fences remain readable until the closing chunk.
const safeHref=(value:string)=>/^(https?:\/\/|mailto:)/i.test(value)?value:undefined
const INLINE=/\[([^\]\n]+)\]\(([^)\s]+)\)|`([^`\n]+)`|\*\*([^*\n]+)\*\*|~~([^~\n]+)~~|\*([^*\n]+)\*/g

function inline(text:string,key:string):ReactNode[]{
 const parts:ReactNode[]=[]
 let cursor=0
 for(const match of text.matchAll(INLINE)){
  const at=match.index??0
  if(at>cursor)parts.push(text.slice(cursor,at))
  const id=`${key}-${at}`
  if(match[1]!==undefined){
   const href=safeHref(match[2])
   parts.push(href?<a key={id} href={href} target="_blank" rel="noopener noreferrer">{match[1]}</a>:match[0])
  }else if(match[3]!==undefined)parts.push(<code key={id}>{match[3]}</code>)
  else if(match[4]!==undefined)parts.push(<strong key={id}>{inline(match[4],id)}</strong>)
  else if(match[5]!==undefined)parts.push(<del key={id}>{inline(match[5],id)}</del>)
  else if(match[6]!==undefined)parts.push(<em key={id}>{inline(match[6],id)}</em>)
  cursor=at+match[0].length
 }
 if(cursor<text.length)parts.push(text.slice(cursor))
 return parts
}
const heading=(line:string)=>/^(#{1,6})\s+(.+)$/.exec(line)
const listItem=(line:string)=>/^\s{0,3}([-*+]|\d+[.)])\s+(.+)$/.exec(line)
const isFence=(line:string)=>/^\s{0,3}```/.test(line)
const isRule=(line:string)=>/^\s{0,3}(?:-{3,}|\*{3,}|_{3,})\s*$/.test(line)
const isQuote=(line:string)=>/^\s{0,3}>\s?/.test(line)
const isTableSeparator=(line:string)=>/^\s*\|?\s*:?-{3,}:?(?:\s*\|\s*:?-{3,}:?)*\s*\|?\s*$/.test(line)
const cells=(line:string)=>line.trim().replace(/^\||\|$/g,'').split('|').map(part=>part.trim())
const isTable=(lines:string[],i:number)=>i+1<lines.length&&lines[i].includes('|')&&isTableSeparator(lines[i+1])
const blockStart=(lines:string[],i:number)=>!!(
 heading(lines[i])||listItem(lines[i])||isFence(lines[i])||isRule(lines[i])||
 isQuote(lines[i])||isTable(lines,i)
)

export function ChatRichText({content}:{content:string}){
 const lines=content.replace(/\r\n?/g,'\n').split('\n')
 const blocks:ReactNode[]=[]
 let i=0
 while(i<lines.length){
  const start=i,line=lines[i]
  if(!line.trim()){i++;continue}
  if(isFence(line)){
   const language=line.trim().slice(3).trim().split(/\s/)[0]
   i++
   const code:string[]=[]
   while(i<lines.length&&!isFence(lines[i]))code.push(lines[i++])
   if(i<lines.length)i++
   blocks.push(<div className="chat-code" key={start}>
    {language&&<div className="chat-code-language">{language}</div>}
    <pre><code>{code.join('\n')}</code></pre>
   </div>)
   continue
  }
  const h=heading(line)
  if(h){
   const level=h[1].length
   blocks.push(createElement(`h${level}`,{key:start},inline(h[2],`h${start}`)))
   i++;continue
  }
  if(isRule(line)){blocks.push(<hr key={start}/>);i++;continue}
  if(isTable(lines,i)){
   const columns=cells(line)
   i+=2
   const body:string[][]=[]
   while(i<lines.length&&lines[i].includes('|')&&lines[i].trim())body.push(cells(lines[i++]))
   blocks.push(<div className="chat-table-scroll" key={start}><table><thead><tr>{columns.map((column,j)=><th key={j}>{inline(column,`t${start}-${j}`)}</th>)}</tr></thead>
    <tbody>{body.map((row,r)=><tr key={r}>{columns.map((_,c)=><td key={c}>{inline(row[c]||'',`t${start}-${r}-${c}`)}</td>)}</tr>)}</tbody>
   </table></div>)
   continue
  }
  if(isQuote(line)){
   const quoted:string[]=[]
   while(i<lines.length&&isQuote(lines[i]))quoted.push(lines[i++].replace(/^\s{0,3}>\s?/,''))
   blocks.push(<blockquote key={start}>{quoted.map((value,j)=><p key={j}>{inline(value,`q${start}-${j}`)}</p>)}</blockquote>)
   continue
  }
  const item=listItem(line)
  if(item){
   const ordered=/^\d/.test(item[1])
   const items:string[]=[]
   while(i<lines.length){
    const row=listItem(lines[i]);if(!row||/^\d/.test(row[1])!==ordered)break
    items.push(row[2]);i++
   }
   const children=items.map((value,j)=><li key={j}>{inline(value,`l${start}-${j}`)}</li>)
   blocks.push(ordered?<ol key={start}>{children}</ol>:<ul key={start}>{children}</ul>)
   continue
  }
  const paragraph=[line];i++
  while(i<lines.length&&lines[i].trim()&&!blockStart(lines,i))paragraph.push(lines[i++])
  blocks.push(<p key={start}>{inline(paragraph.join('\n'),`p${start}`)}</p>)
 }
 return <div className="chat-richtext">{blocks}</div>
}
