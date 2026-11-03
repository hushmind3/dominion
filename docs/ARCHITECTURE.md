# DOMINION Architecture · 2026-11-03

## 설계 원칙

1. **독립적 엔진**: 다른 에이전트 제품의 프로그램을 통째로 기동하거나 iframe으로 붙여 기본 동작으로 사용하지 않는다. 외부 인터페이스는 프로토콜 어댑터.
2. **범용성**: 특정 업무(투자, 코딩, 9명 봇 등)를 플랫폼 코어의 전제로 하지 않는다. 에이전트, 도구, 채널, 모델은 설정·DB 데이터.
3. **로컬 우선**: 별도 클라우드 계정 없이 게이트웨이와 UI가 기동. 모델 호출 시에만 공급자 필요.
4. **검증 가능한 실행**: 대화 스트림, 도구 승인, 실행 결과, 오류, 작업 상태를 별도로 기록. 가짜 모델 출력이나 가짜 결과를 만들지 않는다.
5. **확장 가능한 경계**: provider / MCP / tool / chat / job / gateway / memory / skills / connector 간 책임 분리.
6. **사용자 권한**: 도구 호출은 기본 승인제, 루프백 서버, 암호화된 비밀저장소. 자동 승인은 사용자가 별도 선택.

## 참고 제품 / 독자 구현 취합표

| 조사한 저장소·공식 제품 | 차용한 개념 | DOMINION의 실제 자체 모듈 |
| --- | --- | --- |
| LM Studio + Hugging Face | 모델 탐색, 다운로드 및 로컬 API | `server/providers.mjs`, 모델 허브 |
| DeepSeek / OpenAI 호환 API | SSE content / reasoning / tools 스트리밍 | `server/providers.mjs`, `engine.mjs` |
| OpenCode | 자유로운 모델 제공자, 백그라운드 작업 UX | provider registry, `jobs.mjs` |
| Hermes Agent | 명시적 기억, 재사용 가능한 스킬, 툴 호출 | `store.mjs`, `tools.mjs`, `engine.mjs` |
| OpenClaw | 다중 에이전트 방과 메신저 | `engine.mjs`, `channels.mjs` |
| Claude Code / Codex | MCP 연결, 실제 도구 실행, 승인 | `mcp.mjs`, `tools.mjs` |
| Playwright MCP | 웹사이트를 UI로 제어하는 능력 | 설치 가능한 MCP 어댑터; 자체 브라우저 엔진 아님 |
| 사용자 `hushmind3/Orbit` | Node 실행기/관리 콘솔 분리, 외부 연결 | 독립 Node API + React UI |
| 사용자 `hushmind3/continual-trading-agent` | 모듈형 도구 생태계, DB 기록 | 범용 리소스별 Node 모듈 |
| 사용자 `hushmind3/hermes-desktop-macos` | 페르소나 프로필, 그룹 토론, 한국어 UI | 독립 에이전트/회의실/React 구성 |
| 사용자 `hushmind3/openclaw` | 채널·그룹 정책 | 새 텔레그램·디스코드 게이트웨이 |
| 사용자 `hushmind3/hermes-ui-collection` | 사용자가 테마·UI 완성도를 중요시 | 새 반응형 디자인 시스템 |
| 사용자 `hushmind3/self-agent` | 하나의 개발공간에서 모듈 구성 | 별도 독립 실행 엔진 |
| 사용자 `hushmind3/project-zero` | 원본 Hermes 기능 비교 | 코드 복사/이식 없이 설계만 참고 |
| 사용자 `hushmind3/llmlocalproject05` | 로컬 LLM·RAG 선행 설계 | 모델 추상화 참고; RAG 이식하지 않음 |
| 사용자 `hushmind3/kstock-autotrader` | 지속 실행·실행 원장 설계 개념 | 비금융 범용 작업/감사 원장 |
| 사용자 `hushmind3/FinRL-X-MoE` | 검증되지 않은 결과를 성공으로 보고하지 않음 | 실제 모델/연동 실패 표시 |

**참고 범위**: 해당 저장소의 README와 최상위 구조를 조사했다. 사용자의 로컬 컴퓨터 소스·상용 계정·전체 과거 커밋까지 직접 실행·검사한 것은 아니다. 특히 개인 구성 스냅샷의 API 키 및 세션 데이터는 읽거나 새 저장소로 복사하지 않는다.

## 컴포넌트

- `server/store.mjs`: SQLite WAL, `records`, `messages`, `audit` 테이블, 이벤트 버스.
- `server/vault.mjs`: AES-256-GCM token vault. 256-bit master key는 `runtime/master.key`에 별도 보관.
- `server/providers.mjs`: 모델 목록, Hugging Face REST, LM Studio REST, OpenAI 호환 SSE.
- `server/mcp.mjs`: MCP v2 세션, HTTP/stdio 클라이언트, 도구 발견·호출.
- `server/tools.mjs`: 내장 tool schema, 승인, sandbox-like workspace path 제한, 실제 툴 실행.
- `server/engine.mjs`: 채팅 로그 활용, 에이전트별 시스템 프롬프트, 최대 8회 tool loop, 회의실.
- `server/jobs.mjs`: persistent job queue, recovery, 일정 반복.
- `server/channels.mjs`: Telegram Bot API & Discord gateway 구동.
- `server/oauth.mjs`: Google/GitHub OAuth & 최소 읽기 연결.
- `server/index.mjs`: REST / SSE / 정적 파일.
- `src/pages/*`: API 연결형 React 컴포넌트, mock data 미사용.

## 알려진 초기 버전 한계

- SQLite 싱글 프로세스/서버 모델. 수천 명 동시 사용자용 다중 테넌시, Redis/PG, 분산 워커/요금측정 없음.
- 회의실은 순차 에이전트 발언 구조, 진정한 병렬성·비동기 자유발화/자발적 스케줄 변경은 차기 과제.
- 기억은 명시적 텍스트 매칭과 스킬 재사용. 의미 기반 장기기억, 자동 리플렉션, 자기 코드 개선·롤백·평가 루프는 미구현.
- Playwright MCP 연결로 UI 조작 기능을 가져올 수 있으나 플랫폼 고유 컴퓨터 비전·화면 탐지·보안 컨테이너는 아직 아님.
- GitHub OAuth 어댑터는 저장소 목록과 프로필 읽기. 커밋/이슈/PR 쓰기는 별도 도구 개발이 필요.
- YouTube OAuth는 읽기 스코프만 요청. 영상 업로드/댓글 수정 기능을 주장하지 않는다.
- HTTP MCP 연결은 토큰 Bearer; OAuth 연쇄 갱신·서버 목록 자동 인증은 후속 과제.
- LM Studio downloads에는 설치된 LM Studio 버전과 다운로드 API 활성 상태의 영향이 있다.

## 다음 독립 구현 우선순위

1. Node 실행 엔진 도구별 사전·사후 평가, 중단/재시작 가능한 task graph, 프로세스 격리 및 권한.
2. Chromium computer-use: 화면 캡처, UI 인식, 마우스/키보드, 네이티브 macOS/Windows 브리지.
3. agent reflection loop: 행동→검증→기억 후보→스킬 테스트→승인→롤백.
4. 공개 provider marketplace, OAuth scope 브로커, GitHub/Google/YouTube API 쓰기 액션.
5. Electron 설치파일 서명·배포 + 자동 업데이트 + 로컬/원격 노드.
6. SaaS 멀티테넌시, API 사용량 측정, 구독/크레딧/결제.
