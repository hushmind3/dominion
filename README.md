# DOMINION

> **당신의 모델, 당신의 도구, 당신의 실행 환경.**
>
> 독립적으로 구현한 로컬 우선 범용 AI 워크스페이스 — React 19 / TypeScript / Tailwind CSS 4 / Node.js 24 / SQLite / MCP v2.

DOMINION은 LM Studio, OpenClaw, Hermes, OpenCode의 소스를 결합한 래퍼가 아닙니다. 모델 선택·대화·도구 실행·기억·에이전트 회의·메신저 연동·작업 큐·자동화의 **실행 로직은 이 저장소의 독립적인 코드**로 구현합니다. 외부 프로그램은 선택적으로 연결하는 도구 또는 모델 공급자입니다.

## 즉시 실행

요구 사항: **Node.js 24.12+**, npm, Git. macOS / Windows / Linux에서 Node 서버와 React 화면을 실행합니다.

```bash
git clone https://github.com/hushmind3/dominion.git
cd dominion
npm install
npm run dev
```

브라우저에서 **http://127.0.0.1:5173**을 여세요. 프로덕션 모드는 `npm run build && npm start`, 주소는 **http://127.0.0.1:4317**입니다.

기본 실행은 로컬입니다. `npm run dev`는 Node API 서버와 Vite 개발 서버를 동시에 실행합니다. 선택적 설정은 `.env.example`을 참고해 루트 `.env`를 만드세요. 데이터는 `runtime/`, 에이전트 작업 파일은 `workspace/`에 저장됩니다. 이 두 경로와 API 키는 Git에 업로드되지 않습니다.

**실제 모델 연결**

1. LM Studio 앱에서 모델을 설치·로드하고 **Local Server**를 시작합니다. 일반적인 OpenAI 호환 기본 주소는 `http://127.0.0.1:1234/v1`입니다.
2. 또는 Ollama를 켜고 `http://127.0.0.1:11434/v1`로 연결합니다.
3. DOMINION → **모델 허브**에서 연결 확인. **대화**에서 제공자/모델을 고른 후 메시지를 보내세요.
4. 클라우드 모델은 **모델 허브 → 제공자 추가**에서 OpenAI 호환 Base URL과 API Key를 설정합니다. 키는 로컬 서버의 AES-256-GCM 저장소에 보관됩니다.

## 현재 구현된 엔진

| 영역 | 동작하는 구현 |
| --- | --- |
| 대화 | 실제 OpenAI 호환 `/chat/completions` SSE 스트리밍, 토큰·추론 텍스트 표시, 세션 영속화, 모델 선택 |
| 모델 허브 | Hugging Face 공개 모델 API 검색, GGUF/Safetensors 파일 조회, LM Studio 다운로드 API 호출·상태 확인 |
| 로컬/클라우드 | LM Studio, Ollama, OpenAI 호환 API 공급자 등록, 키 암호화, 모델 목록 조회 |
| 에이전트 | 인원 제한 없는 개별 에이전트 등록, 역할·시스템 프롬프트·모델 설정 |
| 회의 | 가변 참가자/라운드, 한 회의 세션에 발언 누적, 모델 기반 순차 토론 |
| 도구 | 실제 도구 호출 루프, 로컬 작업공간 파일 읽기·쓰기, 터미널 실행, SearXNG 검색, 기억/스킬 제안 |
| MCP | 공식 MCP v2 클라이언트, stdio 및 Streamable HTTP 연결, 도구 발견·호출, Registry 검색, 설치·제거 UI |
| 기억 | SQLite에 지속 저장되는 명시적 기억 검색과 스킬 제안/승인·적용 |
| 메신저 | Telegram Bot API long polling, Discord Gateway WebSocket + REST 응답 |
| 인증 | GitHub/Google OAuth 앱 연결과 토큰 암호화, GitHub 읽기·YouTube 채널 API 기초 연결 |
| 자동화 | DB 기반 영속 작업 큐, 프로세스 재시작 시 복구, 반복 예약, 로그/감사 원장 |
| UI | React/TypeScript/Tailwind 4, 반응형 사이드바, 명령 팔레트, 모델·에이전트·회의·MCP·계정·작업 관리 |
| 보안 | 루프백 기본 바인딩, 원격 접근 토큰 필요, 도구 승인 게이트, 작업공간 경로 제한, 서버 전용 비밀저장소 |

### 중요: 무엇까지 연결되었나

- **실제 동작은 환경에 의존합니다.** 로컬 모델을 로드하지 않았거나 API 키가 없으면 답변을 만들어 내지 않습니다. 모델이 출력한 추론만 보여줍니다. '한국어 추론' 옵션은 한국어로 제공해 달라는 요청이지 자동 번역 기능이 아닙니다.
- **컴퓨터 제어**: 파일·터미널 도구는 자체 구현. 브라우저 클릭·스크린샷은 **Playwright MCP를 별도 등록**해 수행할 수 있습니다. 독립적인 화면 인식/마우스/키보드 실행 엔진 및 원격 데스크톱은 아직 구현하지 않았습니다.
- **MCP 등록**은 실제 실행 연결을 시도합니다. 설치된 패키지의 정상 작동 여부는 사용자 컴퓨터에서 확인해야 합니다. 외부 MCP 패키지의 권한과 출처를 검토하세요.
- **Google/YouTube**는 현재 OAuth 연결과 읽기 API 예제만 동작하도록 작성했습니다. 'Google 계정 하나로 모든 구글 서비스 쓰기'는 서비스별 OAuth 범위와 API 어댑터가 더 필요합니다.
- **무료 모델/API**는 각 공급자가 제공하는 무료 엔드포인트를 직접 연결할 수 있도록 만든 것이지 별도의 무료 API 크레딧을 발급하지 않습니다.
- **스케줄러**는 DOMINION 서버가 켜져 있을 때 실행됩니다. 컴퓨터 절전 중 작업을 이어서 처리하려면 상시 실행되는 별도 서버가 필요합니다.
- **결제·구독·크레딧 측정, Electron 패키징, 원격 멀티테넌트 서비스, 네이티브 컴퓨터-비전 조작**은 이 초기 실행 버전에 포함되지 않습니다. README에서 완성된 기능으로 표시하지 않습니다.

## 명령

```bash
npm run dev          # Node API + Vite
npm run dev:server   # API만
npm run build        # React 생산 빌드
npm start            # dist/ 제공
npm test             # Node 테스트
npm run check        # TypeScript + Node 테스트 + 빌드
```

## 아키텍처

```text
React / Tailwind UI ── HTTP/SSE ──> Node API Gateway
                                        │
                    ┌───────────────────┼───────────────────┐
                    │                   │                   │
               Agent Runtime        Provider Hub         Tool Bus
                    │                   │                   │
              Meeting / Jobs      Local / Cloud       MCP / Workspace
                    │                   │                   │
               SQLite Ledger     OpenAI compatible    Approval / Audit
                    │
              Memory / Skills / Encrypted Vault
                    │
            Telegram / Discord / OAuth
```

자세한 구현 경계는 [아키텍처](docs/ARCHITECTURE.md), 실제 원문 링크가 있는 [조사 근거 장부](docs/RESEARCH_LEDGER.md), [26종 채널별 출처와 구현 상태](docs/CHANNEL_RESEARCH.md)를 참고하세요. 검증 상황은 [docs/STATUS.md](docs/STATUS.md)에 기록합니다.

## 라이선스와 저장소 취급

새로 작성한 DOMINION 코드입니다. 과거 개인 저장소 또는 제3자 제품의 코드/페르소나/토큰을 복사하지 않았습니다. 라이선스는 별도 결정 전까지 명시적인 재배포 허가를 하지 않습니다. 본 저장소는 공개 상태이므로 비밀번호, 토큰, 개인 메시지나 모델 파일을 커밋하지 마세요.

## Electron 데스크톱 실행

웹 콘솔과 독립적인 Electron 실행 진입점이 `desktop/`에 있습니다. macOS / Windows / Linux에서 별도 브라우저를 열지 않고 창으로 실행할 수 있습니다.

```bash
npm install
cd desktop
npm install
npm start
```

Electron 45에서 실행하면 내장 Node 프로세스가 DOMINION 게이트웨이를 직접 시작하고 앱 종료 시 함께 중지합니다. 앱 데이터는 OS의 사용자 데이터 디렉터리에 저장하며 작업 파일은 사용자 문서 폴더의 `DOMINION Workspace`에 보존합니다.

**현재 범위:** 소스 기반 Electron 실행 기능입니다. 서명된 설치 파일(`.dmg`, `.exe`) 배포, 자동 업데이트, 트레이 백그라운드 실행은 아직 추가하지 않았습니다.
