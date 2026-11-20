# 설치와 첫 사용 — DOMINION (2026-11-20)

DOMINION은 Rust 에이전트 실행 엔진, React UI, SQLite 작업 기록을 사용하는 제품입니다. 채팅, 그룹 대화, 사용자가 지시하는 코드 수정·테스트·검증과 기억/도구/모델 연결을 제공합니다. **내장 영상·음성 제작, YouTube 업로드, 머니 조사 엔진, 독립 RSI 자동 소스 승격/실험 캠페인 기능은 제거됐습니다.**

## 준비물

| 사용 방식 | 준비 |
| --- | --- |
| 배포된 macOS/Windows 앱 | 해당 OS 설치 파일, 로컬 모델 또는 API 제공자 |
| 저장소 소스 실행 | Git, 안정 Rust/Cargo, Node.js 24.12 이상, npm, OS별 C/C++ 개발 도구 |
| 로컬 모델 | 사용하려는 모델과 호환되는 Ollama, llama.cpp 등 로컬 제공자 |
| API 모델 | 제공자 설정 또는 지원 로그인 |
| 데스크톱 직접 패키징 | [Tauri v2 사전 요구사항](https://v2.tauri.app/start/prerequisites/) |

macOS에는 Xcode Command Line Tools(\`xcode-select --install\`), Windows에는 Visual Studio Build Tools의 C++ 데스크톱 구성요소와 Windows SDK가 필요합니다. 설치된 데스크톱 앱에서 일반 채팅을 하기 위해 별도 Rust나 Node.js 런타임을 설치할 필요는 없습니다.

## 소스 설치와 실행

\`\`\`sh
git clone https://github.com/hushmind3/dominion-rust.git
cd dominion-rust
git checkout dominion-sub3
npm ci --prefix web
npm run build --prefix web
cargo run --release -- --home .
\`\`\`

브라우저에서 \`http://127.0.0.1:4317\`을 엽니다. 자신의 모델 제공자를 연결한 다음 채팅·그룹 대화를 사용하거나 작업을 지시합니다.

1. **모델 관리**에서 로컬 모델 또는 API 제공자를 등록합니다. 모델 카탈로그의 공개 조회는 개별 API 키 없이 가능한 경로를 사용합니다.
2. **채팅**에서 필요한 파일 조사와 수정을 요청합니다. 도구 접근·쓰기·셸 실행은 실행 권한 및 승인 규칙을 따릅니다.
3. **작업/에이전트**에서 목표, 선택한 모델, 허용 도구와 검증 명령을 지정합니다. 결과와 실제 도구 영수증은 Rust 실행 저널에 남습니다.
4. **기억/연결**에서 기록을 검토하고 승인된 GitHub·Google 계정 및 MCP/Skills를 사용합니다. Google 로그인은 YouTube 업로드 권한을 요청하지 않습니다.

코드를 개선하려면 사용자가 구체적인 수정을 지시하고, 작업 도구가 수정한 파일을 검증·테스트하도록 실행합니다. **자동 RSI 세대 반복, 자율 평가 캠페인, 자동 코드 채택 및 영상 배포 메뉴는 존재하지 않습니다.** 필요 시 외부 미디어 기능을 승인된 MCP/플러그인 도구로 연결할 수 있습니다.

## 데이터와 변경 안전성

대화, 영수증, 과거 작업 기록, 사용자 파일은 SQLite와 작업 디렉터리에 유지됩니다. 폐기된 예전 영상/RSI 작업 종류는 재실행을 차단합니다. 과거 기록은 새 도구 호출로 승격하거나 임의로 재생하지 않습니다. YouTube 업로드 전용 구형 OAuth 자격증명은 시작 시 안전하게 정리되고 일반 Google/GitHub 연결은 유지됩니다.

개인 대화·키·OAuth 비밀·실패 원문을 공개 저장소에 올리지 마십시오. 소스 변경 후 Rust 및 프론트 CI 검사를 사용하고, OS별 패키징은 별도 릴리즈 검증으로 취급합니다.

## 개발자 검증

\`\`\`sh
node scripts/check-lean-product.mjs
cargo test -p dominion-agent-sdk --lib
cargo test -p dominion-sdk --lib
cargo test --bin dominion-rust engine::chat::provider::tests
npm run typecheck --prefix web
npm run build --prefix web
\`\`\`

보다 상세한 현재 실행 계약은 [하네스 문서](HARNESS.md), [작업 SDK](../sdk/typescript/README.md), [문서 지도](INDEX.md)를 참고하십시오. 실제 통과 여부는 해당 GitHub Actions 커밋을 기준으로 확인합니다.
