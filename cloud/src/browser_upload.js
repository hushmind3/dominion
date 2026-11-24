// User-selected file only. No model-supplied bytes or arbitrary filesystem paths.
export const MAX_UPLOAD=2*1024*1024;
export function validUpload(file){
 if(!file||typeof file.name!=='string'||file.name.length<1||file.name.length>120||/[\\/\x00-\x1f\x7f]/.test(file.name)||!['application/pdf','image/jpeg','image/png'].includes(file.mime)||typeof file.base64!=='string'||file.base64.length>Math.ceil(MAX_UPLOAD/3)*4||!/^[A-Za-z0-9+/]+={0,2}$/.test(file.base64))throw Error('PDF, JPG, PNG 파일만 2MB 이하로 첨부할 수 있습니다.');
 let raw;try{raw=atob(file.base64)}catch{throw Error('파일 인코딩 오류')}
 if(raw.length<8||raw.length>MAX_UPLOAD)throw Error('파일 크기 제한');
 const valid=file.mime==='application/pdf'?raw.startsWith('%PDF-')&&/\.pdf$/i.test(file.name):file.mime==='image/jpeg'?raw.charCodeAt(0)===255&&raw.charCodeAt(1)===216&&raw.charCodeAt(2)===255&&/\.jpe?g$/i.test(file.name):raw.startsWith('\x89PNG\r\n\x1a\n')&&/\.png$/i.test(file.name);
 if(!valid)throw Error('파일 내용과 확장자가 일치하지 않습니다.');
 return {name:file.name,mime:file.mime,base64:file.base64,size:raw.length};
}
export function acceptsFile(accept,file){
 const values=String(accept||'').toLowerCase().split(',').map(x=>x.trim()).filter(Boolean);
 return !values.length||values.some(x=>x==='*/*'||x===file.mime||x.endsWith('/*')&&file.mime.startsWith(x.slice(0,-1))||x.startsWith('.')&&file.name.toLowerCase().endsWith(x));
}
export function attachFile(index,expected,file,expectedUrl){
 if(location.href!==expectedUrl)return {ok:false};
 const clean=(s,n=160)=>String(s??'').replace(/\s+/g,' ').trim().slice(0,n);
 const nodes=[...document.querySelectorAll('a[href],button,input:not([type=hidden]),textarea,select,[role=button]')].filter(e=>{const r=e.getBoundingClientRect(),s=getComputedStyle(e);return r.width>0&&r.height>0&&s.visibility!=='hidden'&&s.display!=='none'&&s.opacity!=='0'&&!e.closest('[hidden],[aria-hidden="true"]')&&!e.disabled&&!e.closest('[aria-disabled=true]')}).slice(0,100);
 const e=nodes[index];if(!e||e.tagName!=='INPUT'||e.type!=='file'||e.files?.length)return {ok:false};
 const label=clean(e.getAttribute('aria-label')||Array.from(e.labels||[]).map(l=>l.innerText).join(' ')||e.innerText||e.getAttribute('placeholder')||e.getAttribute('title')||e.name);
 if(label!==expected.label||clean(e.accept,200)!==expected.accept)return {ok:false};
 const raw=atob(file.base64),bytes=new Uint8Array(raw.length);for(let i=0;i<raw.length;i++)bytes[i]=raw.charCodeAt(i);
 const transfer=new DataTransfer();transfer.items.add(new File([bytes],file.name,{type:file.mime}));
 e.files=transfer.files;
 if(e.files.length!==1||e.files[0].name!==file.name||e.files[0].size!==file.size)return {ok:false};
 e.dispatchEvent(new Event('input',{bubbles:true}));e.dispatchEvent(new Event('change',{bubbles:true}));
 return {ok:true};
}
