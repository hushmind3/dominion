# DOMINION

<p align="center"><img src="src-tauri/icons/icon.png" alt="DOMINION" width="90" /></p>
<p align="center"><strong>내 모델, 내 작업 공간, 독립 Rust 에이전트 실행기.</strong></p>
<p align="center"><strong>한국어</strong> · <a href="README.md">English</a> · <a href="LICENSE">MIT</a> · <a href="https://github.com/hushmind3/dominion-rust/releases">설치 파일</a></p>

DOMINION은 **Rust 실행 엔진 + React/Vite 인터페이스 + Tauri 2 데스크톱**
구조의 로컬 우선 오픈소스 AI 에이전트 워크스페이스입니다.
**Windows와 macOS**에서 같은 코드의 네이티브 실행기와 UI를 사용합니다.

<p align="center">
  <img src="docs/01-multi-agent-room.webp" alt="다중 에이전트 방" width="48%" />
  <img src="docs/02-chat-workspace.webp" alt="채팅 작업 공간" width="48%" />
</p>

## 주요 기능

- **실제 작업 수행:** Rust 단일 실행 루프, 도구 실행 승인, 예산 제한,
  취소·재개, 효과 영수증, 결과 검증과 복구
- **모델 선택:** OpenAI·Anthropic 등 API, Ollama·llama.cpp·LM Studio 같은
  로컬 추론 엔드포인트 연결
- **다중 에이전트 방:** 에이전트별 페르소나와 독립 문맥, 공통 방 상태,
  SQLite 대화 기록, 호출별 토큰 계측
- **도구:** 파일·셸·브라우저와 외부 MCP 확장, 동일한 승인·권한·감사 체계
- **지속 상태:** 대화·기억·작업·검증·승인 기록의 SQLite 저장
- **SDK:** Rust 실행 커널과 작업 생성·승인·취소·검증용 TypeScript 클라이언트
- **데스크톱:** 실행 시 별도 Node 서버가 필요 없는 Rust 사이드카 기반 구성

## 설치 파일

[GitHub Releases](https://github.com/hushmind3/dominion-rust/releases)에
**Apple Silicon용 DMG, Intel Mac용 DMG, Windows용 EXE·MSI**를 제공합니다.
코드 서명 없는 빌드이므로 설치 시 운영체제의 보안 확인이 필요할 수 있습니다.

버전별 배포는 **동일한 소스 커밋에서 Windows·macOS 네이티브 검사와
빌드가 모두 성공한 후** 생성하도록 구성했습니다. 프런트 빌드만
성공했다고 데스크톱 릴리즈 완료로 취급하지 않습니다.

## 개발 환경

**Rust stable, Node.js 24.12 이상, npm**과
[Tauri 2 운영체제별 사전 준비](https://v2.tauri.app/start/prerequisites/)가 필요합니다.

```bash
git clone https://github.com/hushmind3/dominion-rust.git
cd dominion-rust
npm ci --prefix web
npm run typecheck --prefix web
npm run build --prefix web
cargo test --locked --workspace
```

개발 서버는 터미널 2개에서 다음을 실행합니다.

```bash
npm run dev:server --prefix web
npm run dev --prefix web
```

운영체제별 데스크톱 빌드:

```bash
node scripts/check-release-contract.mjs
node scripts/check-retired-domains.mjs
node scripts/build-desktop.mjs
```

## 코드 구조

| 계층 | 실제 코드 |
| --- | --- |
| 실행 커널·효과 영수증 | [dominion-core](crates/dominion-core/src/engine.rs), [저널](crates/dominion-core/src/journal.rs) |
| 실행·도구 승인 | [채팅 실행기](src/engine/chat/runtime.rs), [도구 게이트웨이](src/engine/chat/runtime/tool_dispatch.rs) |
| 방·기억·연속성 | [그룹 방](src/engine/rooms.rs), [문맥 구성](src/engine/chat/room_context.rs), [연속성](src/engine/chat/continuity.rs) |
| HTTP·영속 저장 | [API 라우터](src/api/mod.rs), [SQLite](src/db.rs) |
| 화면 | [React 앱](web/src/App.tsx), [메뉴](web/src/components/WorkspaceShell.tsx) |
| 설치 프로그램 | [Tauri 설정](src-tauri/tauri.conf.json), [빌드 스크립트](scripts/build-desktop.mjs) |
| TypeScript SDK | [클라이언트](packages/sdk-typescript/src/index.ts) |

### 대화와 작업 완료는 다릅니다

사용자 명령은 Rust에서 권한·도구·모델·예산을 적용한 실행으로
변환됩니다. 도구 결과와 승인, 취소, 검증 결과는 SQLite에 남습니다.
**모델이 성공했다고 답한 것만으로 작업 완료를 판정하지 않습니다.**
MCP 검색 후 도구를 호출하는 경우에도 현재 스키마와 실행·세션
권한을 재검증합니다.

### 대화 기억과 토큰 비용

9명 방의 대화 기록은 저장하되, 모델에 전송할 문맥은 각 참여자의
역할과 현재 질문·확정 상태를 기준으로 선택합니다. 제공자 캐시가
지원되면 고정 접두부의 재사용을 요청합니다. **실제 캐시 적중
토큰**과 하네스의 **재사용 추정치**는 분리해 기록하며,
독립 모델 호출이 동일한 내부 사고 상태를 유지한다고 가정하지 않습니다.

### 폐기 기능 정리

이전 **돈벌이 발굴 도구**, 내장 영상·음성 생성 파이프라인,
독립 RSI 자동 캠페인은 현재 제품의 실행 기능에서 제거했습니다.
외부 제작·자동화 서비스는 필요할 때 일반 MCP 도구로 연결합니다.
기존 DB 사용자 자료는 자동 삭제하지 않고, 과거 작업이 다시
실행되지 않도록 차단합니다.

## 검증·배포

- [일반 CI](.github/workflows/ci.yml): Rust 회귀, 화면 계약,
  프런트 빌드 및 Windows 검사
- [네이티브 릴리즈](.github/workflows/release.yml):
  macOS ARM64·x64 및 Windows x64 테스트·빌드·배포
- [구조 검사](scripts/check-runtime-architecture.mjs),
  [폐기 기능 검사](scripts/check-retired-domains.mjs),
  [버전·사이드카 검사](scripts/check-release-contract.mjs)

추가 문서: [시작 안내](docs/GETTING-STARTED.md),
[데스크톱](docs/DESKTOP.md), [엔진 구조](ENGINE.md),
[하네스](docs/HARNESS.md), [TypeScript SDK](packages/sdk-typescript/README.md).

**메인 브랜치**가 정식 개발 기준입니다. 라이선스는 MIT입니다.
