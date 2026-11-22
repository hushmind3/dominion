export function explicitWebSearchRequested(input:string):boolean{
 const s=input.trim().toLocaleLowerCase('ko')
 if(/(?:검색하지|검색은\s*하지|검색\s*없이|웹\s*검색\s*금지|do\s+not\s+(?:search|browse)|don['’]t\s+(?:search|browse)|without\s+(?:searching|browsing))/.test(s))return false
 return /(?:검색해(?:줘|주세요|봐|보세요)?|검색해서|인터넷(?:에서)?\s*검색|웹\s*검색|인터넷에서\s*찾아|search\s+(?:the\s+)?web|browse\s+(?:the\s+)?web|look\s+(?:it\s+)?up\s+online)/.test(s)
}
