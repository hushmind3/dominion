// Output normalization for the immutable Rust A/B evaluator.
// Windows libtest may emit CRLF even though Node splits stdout on LF.
// Treat test names and executable paths as opaque; do not normalize the
// names themselves, only the record delimiters.
export function parseCargoTestArtifacts(stdout) {
 const artifacts=new Set();
 for(const line of stdout.split(/\r?\n/)) {
  let record;
  try{record=JSON.parse(line)}catch{continue}
  if(record?.reason==='compiler-artifact'&&record.profile?.test&&
     typeof record.executable==='string'&&record.executable.length>0){
   artifacts.add(record.executable);
  }
 }
 return [...artifacts];
}

export function parseTestInventory(stdout) {
 const names=[];
 for(const raw of stdout.split(/\r?\n/)) {
  const line=raw.trimEnd();
  if(line.endsWith(': test'))names.push(line.slice(0,-6));
 }
 return names;
}
