import {useMemo,type ReactNode} from 'react'

// Render an intentionally bounded subset of Markdown without HTML injection,
// external dependencies or a separate renderer for streamed content.
function inlineParts(text:string):ReactNode[]{
 const pattern=/(\*\*[^*\n]+\*\*|`[^`\n]+`|\[[^\]\n]+\]\(https?:\/\/[^\s)]+\))/g
 const parts:ReactNode[]=[]
 let last=0
 for(const match of text.matchAll(pattern)){
  const index=match.index??last
  if(index>last)parts.push(text.slice(last,index))
  const token=match[0]
  if(token.startsWith('**')){
   parts.push(<strong key={index} className="font-semibold text-[#292622]">{token.slice(2,-2)}</strong>)
  }else if(token.startsWith('`')){
   parts.push(<code key={index} className="rounded-md bg-[#F1EBE2] px-1.5 py-0.5 font-mono text-[.9em]">{token.slice(1,-1)}</code>)
  }else{
   const separator=token.indexOf('](')
   const label=token.slice(1,separator)
   const href=token.slice(separator+2,-1)
   parts.push(<a key={index} href={href} target="_blank" rel="noopener noreferrer" className="underline decoration-[#BA8D7D] underline-offset-2 hover:text-[#A94E30]">{label}</a>)
  }
  last=index+token.length
 }
 if(last<text.length)parts.push(text.slice(last))
 return parts
}

function pipeCells(line:string):string[]{
 return line.trim().replace(/^\||\|$/g,'').split('|').map(cell=>cell.trim())
}

function isTableDivider(line:string):boolean {
 const cells=pipeCells(line)
 return cells.length>1&&cells.every(cell=>/^:?-{3,}:?$/.test(cell))
}

function renderBlocks(text:string):ReactNode[]{
 const lines=text.replace(/\r\n?/g,'\n').split('\n')
 const blocks:ReactNode[]=[]
 const isBlock=(line:string)=>/^\s*```|^#{1,4}\s+|^>\s?|^\s*(?:[-*]|\d+[.)])\s+/.test(line)
 let i=0
 while(i<lines.length){
  const line=lines[i]
  if(!line.trim()){i++;continue}
  const code=line.match(/^\s*```([^\s`]*)\s*$/)
  if(code){
   i++
   const body:string[]=[]
   while(i<lines.length&&!/^\s*```\s*$/.test(lines[i]))body.push(lines[i++])
   if(i<lines.length)i++
   blocks.push(<div key={blocks.length} className="my-3 overflow-hidden rounded-xl border border-[#363A44] bg-[#23272F]">
    {code[1]&&<div className="border-b border-white/10 px-4 py-1.5 text-[11px] font-medium text-[#D1D5DB]">{code[1]}</div>}
    <pre className="max-w-full overflow-x-auto p-4 text-[13px] leading-6 text-[#F5F5F5]"><code className="font-mono whitespace-pre">{body.join('\n')}</code></pre>
   </div>)
   continue
  }
  if(i+1<lines.length&&line.includes('|')&&isTableDivider(lines[i+1])){
   const headers=pipeCells(line)
   i+=2
   const rows:string[][]=[]
   while(i<lines.length&&lines[i].trim()&&lines[i].includes('|')){
    rows.push(pipeCells(lines[i++]))
   }
   blocks.push(<div key={blocks.length} className="my-4 max-w-full overflow-x-auto rounded-xl border border-[#E3DBD0]">
    <table className="w-full min-w-[360px] border-collapse text-left text-[14px] leading-6">
     <thead className="bg-[#F5F0E8]"><tr>{headers.map((cell,j)=><th key={j} scope="col" className="border-b border-[#E3DBD0] px-3 py-2 font-semibold text-[#3A342D]">{inlineParts(cell)}</th>)}</tr></thead>
     <tbody>{rows.map((row,j)=><tr key={j} className="even:bg-[#FAF8F3]">{headers.map((_,k)=><td key={k} className="border-b border-[#EEE8DF] px-3 py-2 align-top">{inlineParts(row[k]||'')}</td>)}</tr>)}</tbody>
    </table>
   </div>)
   continue
  }
  if(/^\s*(?:-{3,}|\*{3,}|_{3,})\s*$/.test(line)){
   blocks.push(<hr key={blocks.length} className="my-5 border-0 border-t border-[#E3DBD0]"/>)
   i++;continue
  }
  const heading=line.match(/^(#{1,4})\s+(.+)$/)
  if(heading){
   const level=heading[1].length
   const cls=level===1?'text-[22px] font-bold':level===2?'text-[19px] font-bold':level===3?'text-[17px] font-semibold':'text-[16px] font-semibold'
   blocks.push(<div key={blocks.length} role="heading" aria-level={level} className={'mb-2 mt-4 leading-snug text-[#292622] '+cls}>{inlineParts(heading[2])}</div>)
   i++;continue
  }
  if(/^>\s?/.test(line)){
   const quoted:string[]=[]
   while(i<lines.length&&/^>\s?/.test(lines[i]))quoted.push(lines[i++].replace(/^>\s?/,''))
   blocks.push(<blockquote key={blocks.length} className="my-3 border-l-[3px] border-[#C9B5A6] bg-[#FAF7F1] py-2 pl-3 pr-2 text-[#61584E] whitespace-pre-wrap">{inlineParts(quoted.join('\n'))}</blockquote>)
   continue
  }
  const bullet=line.match(/^\s*([-*]|\d+[.)])\s+(.+)$/)
  if(bullet){
   const ordered=/^\d/.test(bullet[1])
   const items:ReactNode[]=[]
   const listPattern=ordered?/^\s*\d+[.)]\s+(.+)$/:/^\s*[-*]\s+(.+)$/
   while(i<lines.length){
    const item=lines[i].match(listPattern)
    if(!item)break
    items.push(<li key={items.length} className="pl-1">{inlineParts(item[1])}</li>)
    i++
   }
   blocks.push(ordered
    ?<ol key={blocks.length} className="my-3 list-decimal space-y-1 pl-6">{items}</ol>
    :<ul key={blocks.length} className="my-3 list-disc space-y-1 pl-6">{items}</ul>)
   continue
  }
  const paragraph:string[]=[line]
  i++
  while(i<lines.length&&lines[i].trim()&&!isBlock(lines[i]))paragraph.push(lines[i++])
  blocks.push(<p key={blocks.length} className="my-2 whitespace-pre-wrap">{inlineParts(paragraph.join('\n'))}</p>)
 }
 return blocks
}

export function ChatRichText({text}:{text:string}){
 const blocks=useMemo(()=>renderBlocks(text),[text])
 return <div className="chat-rich-text">{blocks}</div>
}
