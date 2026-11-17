import test from 'node:test';
import assert from 'node:assert/strict';
import {readFileSync} from 'node:fs';

const load=path=>readFileSync(new URL('../'+path,import.meta.url),'utf8');
const ui=load('web/src/pages/Connections.tsx');
const broker=load('src/api/cli_oauth.rs');
const oauth=load('src/engine/oauth.rs');
const tools=load('src/engine/tools.rs');

test('connected-account UI offers one-click browser login and never developer credentials',()=>{
  assert.match(ui,/async\(item:OAuthProvider\)/);
  assert.match(ui,/github'\?'github':item\.id==='google'\?'google_workspace'/);
  assert.match(ui,/\/api\/providers\/cli\//);
  assert.match(ui,/\/login\/status/);
  assert.match(ui,/공식 로그인 페이지 열기/);
  assert.doesNotMatch(ui,/OAuth 앱 설정/);
  assert.doesNotMatch(ui,/<FormField label="Client ID">/);
  assert.doesNotMatch(ui,/Client Secret/);
  assert.doesNotMatch(ui,/setOauthEditor|clientSecret/);
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
  assert.match(tools,/"github_work_read"/);
  assert.match(tools,/"google_workspace_read"/);
});
