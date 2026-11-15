// Node fs.realpathSync() on Windows may return \\?\ extended paths.
// Native filesystem APIs accept them; Git for Windows does not accept them
// as command-line patch/worktree arguments. Keep the canonical paths for
// filesystem access and normalize only at the Git child-process boundary.
const EXTENDED='\\\\?\\';

export function gitCompatibleArg(value){
 if(typeof value!=='string') return value;
 if(value.startsWith(EXTENDED)){
  const rest=value.slice(EXTENDED.length);
  return rest.startsWith('UNC\\') ? '\\\\'+rest.slice(4) : rest;
 }
 // A Git-for-Windows path may already have forward-slash separators.
 if(value.startsWith('//?/')){
  const rest=value.slice(4);
  return rest.startsWith('UNC/') ? '//'+rest.slice(4) : rest;
 }
 return value;
}
