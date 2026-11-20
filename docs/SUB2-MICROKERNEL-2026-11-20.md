# 서브2 최소 코어 · 외부 서비스 아키텍처 (2026-11-20)

담당: dominion-sub2. 사용자 화면이나 API 경로가 아니라 실제 Rust 소스의 책임 영역을 재편합니다.

## Core
- src/engine/core/mod.rs : 자연어 요청을 기존 Rust 실행기로 전달하고 실행 결과·도구 영수증 수집
- src/engine/core/chat/runtime.rs : 요청→모델→도구→결과의 단일 실행 루프
- src/engine/core/chat/protocol.rs : 모델 응답과 도구 호출 해석
- 승인·효과(effect)·취소·결과 무결성은 실행 경계에 남겨 부가기능이 독립 실행 권한을 갖지 못하게 유지

## Services
- providers : 모델 선택, LLM 제공자, 스트리밍·전송, CLI 인증
- conversation : 메모리, 맥락 구성, 원문 조회, 멀티에이전트·방
- decision : 계획, 검증 보조, 판단, 평가, 프로그램 수리
- automation : 지속 작업, 스케줄·재개·복구
- tools : 파일·터미널·코드 검색 등 도구 구현 및 도구 탐색
- operations : 백업, 유지관리, 토큰 진단

## Addons
- MCP, 외부 계정과 GitHub/Google, 브라우저, Discord/Telegram/채널 메시징
- API 등록은 src/api/addons.rs 에서 맡으며, 기존 27개 URL 그대로 유지

## 바뀐 것과 안 바뀐 것
- engine 최상위의 60개 영역 모듈과 chat 내부의 제공자·맥락·탐색 12개 모듈을 파일 단위로 이동했습니다.
- 이동한 Rust 구현 파일은 Git blob SHA가 동일하며, 기존 모듈 이름은 Rust #[path] 선언으로 유지합니다.
- 기존 UI·메뉴, SDK 경로, DB 기록과 공개 HTTP 경로는 변경하지 않았습니다.
- 현재는 소스·책임 분리이며 Cargo 개별 패키지 분리나 기능 언로드를 구현했다고 주장하지 않습니다.
- 코어가 일부 서비스 함수를 직접 호출하는 기존 방식도 남아 있습니다. 포트 인터페이스 전면화를 아직 완료한 상태는 아닙니다.

## 검증
- scripts/sub2-core-boundary.test.mjs 에서 코어·서비스 파일 위치와 API 보존 점검
- scripts/sub2-addon-boundary.test.mjs 에서 MCP/연결 채널 격리 점검
- Rust CI에서 기존 engine::chat::* 경로로 실행 로직 재검증

2026년 아키텍처 참고:
https://github.com/QwenLM/qwen-code/blob/main/docs/developers/architecture.md
https://github.com/QwenLM/qwen-code/issues/11695
