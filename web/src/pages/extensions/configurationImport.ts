/** Generic, owner-reviewed configuration import. No save or execution occurs.
 * Only declared writable fields can be imported; generated IDs are always new. */
type Schema={type?:string;generated?:boolean;readOnly?:boolean;default?:unknown;format?:string;enum?:string[];properties?:Record<string,Schema>;items?:Schema;maxItems?:number}
const reserved=new Set(['__proto__','constructor','prototype'])
export function importConfigItem(schema:Schema,raw:string,newId:()=>string):Record<string,unknown>{
 if(raw.length>16384)throw Error('Configuration item too large')
 function check(s:Schema,v:unknown,depth:number):unknown{
  if(depth>5)throw Error('Configuration item too deeply nested')
  if(s.type==='object'){
   if(!v||typeof v!=='object'||Array.isArray(v)||!s.properties)throw Error('Expected declared configuration object')
   const row=v as Record<string,unknown>,props=s.properties
   const keys=Object.keys(row)
   if(keys.length>64)throw Error('Too many fields')
   for(const k of keys)if(reserved.has(k)||!Object.hasOwn(props,k)||props[k].generated||props[k].readOnly||props[k].format==='password')throw Error('Unsupported imported field: '+k)
   return Object.fromEntries(Object.entries(props).filter(([k,p])=>!reserved.has(k)&&!p.readOnly&&p.format!=='password'&&(p.generated||Object.hasOwn(row,k)||p.default!==undefined)).slice(0,64).map(([k,p])=>[k,p.generated?newId():check(p,Object.hasOwn(row,k)?row[k]:structuredClone(p.default),depth+1)]))
  }
  if(s.type==='array'){if(!Array.isArray(v)||v.length>Math.min(s.maxItems??64,64)||!s.items)throw Error('Invalid array');return v.map(x=>check(s.items!,x,depth+1))}
  if(s.type==='string'){if(typeof v!=='string'||v.length>4096||(s.enum&&!s.enum.includes(v)))throw Error('Invalid string');return v}
  if(s.type==='boolean'&&typeof v==='boolean')return v
  if((s.type==='number'||s.type==='integer')&&typeof v==='number'&&Number.isFinite(v)&&(s.type!=='integer'||Number.isInteger(v)))return v
  throw Error('Unsupported configuration value')
 }
 if(schema.type!=='object'||!schema.properties)throw Error('Expected declared item schema')
 return check(schema,JSON.parse(raw),0) as Record<string,unknown>
}
