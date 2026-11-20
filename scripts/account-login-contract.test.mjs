import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';

const load=path=>readFileSync(new URL('../'+path,import.meta.url),'utf8');
const ui=load('web/src/pages/Connections.tsx');
const broker=load('src/api/cli_oauth.rs');
const oauth=load('src/integrations/accounts/oauth.rs');
const accountTools=load('src/integrations/accounts/tools.rs');
const tools=load('src/engine/tools.rs');

test('only the quick-connect UI exposes GitHub and Google sign-in',()=>{
  const quick=load('web/src/components/ConnectedAccounts.tsx');
  const accounts=load('web/src/pages/Connections.tsx');
  const operations=load('web/src/pages/Operations.tsx');
  const authority=load('src/api/channels.rs');
  assert.match(quick,/supportedOAuthAccounts\(providers\.value\)/);
  assert.match(accounts,/supportedOAuthAccounts\(oauth\.value\)/);
  assert.match(quick,/await openOAuth\(id\)/);
  assert.doesNotMatch(accounts,/\bopenOAuth\b/);
  assert.doesNotMatch(operations,/\bopenOAuth\b/);
  assert.doesNotMatch(operations,/\/api\/oauth\/providers/);
  assert.match(authority,/is_supported_provider/);
  assert.doesNotMatch(oauth,/id:\s*"youtube"/);
  assert.doesNotMatch(oauth,/auth\/youtube\./);
  assert.match(oauth,/pub fn purge_retired_grants/);
});

test('provider-owned browser flow uses fixed official CLI binaries not user-supplied shells',()=>{
  assert.match(broker,/"github"=>Ok\(\("gh"/);
  assert.match(broker,/"google_workspace"=>Ok\(\("gws"/);
  assert.match(broker,/"--web"/);
  assert.match(broker,/"--scopes"/);
  assert.match(broker,/adopt_cli_grant/);
  assert.match(broker,/source=="github"/);
  assert.match(broker,/github_device_code/);
  assert.match(broker,/timeout=tokio::time::sleep\(Duration::from_secs\(600\)\)/);
  assert.match(broker,/child\.kill\(\)\.await/);
});

test('official CLI grant enters existing encrypted vault and standard agent tool catalog',()=>{
  assert.match(oauth,/pub async fn adopt_cli_grant/);
  assert.match(oauth,/state\.save_secret\("oauth:github:token"/);
  assert.match(oauth,/state\.save_secret\("oauth:google:token"/);
  assert.match(oauth,/state\.save_secret\("oauth:google:secret"/);
  assert.match(oauth,/oauth2\.googleapis\.com\/tokeninfo/);
  assert.match(oauth,/authSource"\]=json!\("github_cli"\)/);
  assert.match(oauth,/authSource"\]=json!\("gws_cli"\)/);
  assert.match(tools,/pub fn definitions_for/);
  assert.match(accountTools,/"github_work_read"/);
  assert.match(accountTools,/"google_workspace_read"/);
});
