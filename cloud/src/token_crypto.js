// Shared token encoding and hashing for Google, OAuth and email identities.
// Keep the same canonical base64url representation for D1 hashes and bearer tokens.
export const b64url=data=>btoa(String.fromCharCode(...new Uint8Array(data)))
 .replace(/=/g,'').replace(/\+/g,'-').replace(/\//g,'_');
export const hash=async value=>b64url(await crypto.subtle.digest('SHA-256',new TextEncoder().encode(value)));
export const randomToken=()=>b64url(crypto.getRandomValues(new Uint8Array(32)));
