# 구현 및 검증 상태

작성일: **2026-11-03**. 상태는 실제로 수행한 검사에 따라 변경한다.

| 항목 | 소스 | 검증 |
| --- | --- | --- |
| React TypeScript/Tailwind 화면 | 작성 | CI 타입검사 및 Vite 빌드 예정 |
| SQLite 영속 저장/암호화 vault | 작성 | Node 단위 테스트 포함 |
| HTTP API와 서버 상태 | 작성 | Node HTTP 스모크 테스트 포함 |
| LLM API / tool calls / reasoning SSE | 작성 | 실제 LM Studio/DeepSeek 계정·모델 환경 아직 미검증 |
| Hugging Face 검색/LM Studio 다운로드 | 작성 | 실제 사용자 PC 다운로드 미검증 |
| MCP stdio/HTTP/Registry | 작성 | 외부 MCP 실제 프로세스 연결 미검증 |
| 에이전트/가변 회의실/작업 큐 | 작성 | 기본 생성 테스트 포함, 실제 모델 기반 회의 미검증 |
| Telegram / Discord | 작성 | 사용자 봇 토큰 및 실제 채널 수신·발신 미검증 |
| OAuth Google/GitHub | 작성 | 사용자의 OAuth 앱 자격증명 미설정/미검증 |
| 컴퓨터 직접 조작 | 로컬 파일·셸 도구 작성 | 사용자 OS GUI 직접 조작은 미구현 |
| 스킬 및 기억 | 명시적 기억/스킬 작성 | 자동 자기개선 학습 루프 미구현 |
| API 무료 모델 중개 | 범용 OpenAI 호환 제공자 | 외부 무료 API 크레딧 발급 불가 |
| Electron | 미구현 | 웹 애플리케이션 동작 |
| SaaS 결제·과금 | 미구현 | 구독/크레딧 시스템 미구현 |

**기능 명칭과 실제 검증 범위를 분리한다.** 모델이 없으면 모델 응답을 시뮬레이션하지 않는다. CI가 녹색이더라도 사용자 로컬 LM Studio·Discord·OAuth까지 검증했다는 뜻은 아니다.
