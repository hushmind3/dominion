# DOMINION 문서 지도 — dominion-sub3 (2026-11-20)

현재 배포·코드 구조를 설명하는 문서만 안내합니다. 폐기된 영상·머니·독립 RSI 실행기의 설치·세대 반복·자동 실험 문서는 정리했습니다.

## 첫 사용과 주요 구조

- [설치 및 시작](GETTING-STARTED.md) — macOS / Windows / Rust + React
- [프로젝트 개요](../README.ko.md) · [English](../README.md)
- [하네스 및 실행 경계](HARNESS.md)
- [네이티브 Rust·TypeScript SDK](../sdk/rust/README.md) · [TypeScript](../sdk/typescript/README.md)
- [데스크톱 앱](DESKTOP.md)

## 에이전트 실행·검증

- [경량화된 제품 경계와 기능 퇴역](SUB3-LEAN-COMPLEXITY-2026-11-16.md)
- [실행 근거와 파일 무결성](SUB3-EVIDENCE-GOVERNOR-2026-11-16.md)
- [네이티브 도구 함수 호출](SUB3-NATIVE-FUNCTION-CALLS-2026-11-15.md)
- [네이티브 스트리밍](SUB3-NATIVE-STREAM-2026-11-15.md)
- [실행 메트릭](SUB3-EXECUTION-METRICS-2026-11-17.md)
- [현재 소스 경계 감사](SUB3-SOURCE-AUDIT-2026-11-18.md)

## 컨텍스트·모델·그룹 대화

- [그룹 맥락과 캐시 재사용](SUB3-ROOM-CONTEXT-DELTA-2026-11-18.md)
- [대화 문맥 압축·비용](SUB3-CACHE-CONTEXT-2026-11-18.md)
- [지속 가능한 작업 문맥](SUB3-DURABLE-CONTEXT-2026-11-15.md)
- [도구 점진적 탐색](SUB3-PROGRESSIVE-TOOLS-2026-11-16.md)
- [토큰·캐시 조사 기록](research/README.md)

## 연결과 운영

- [GitHub·Google 계정 연결](CONNECTED-ACCOUNTS-2026-11-17.md)
- [선택형 Cloudflare 스테이징 게이트웨이](SUB3-CLOUD-GATEWAY-2026-11-19.md)
- [통합 실행 점검](SUB3-INTEGRATION-2026-11-19.md)

옛날 측정 파일 \`benchmark/\`는 역사적 근거로 유지합니다. 그 수치를 현재 브랜치에서 측정한 결과로 취급하지 않습니다. 실행 가능한 폐기 기능이나 과거 자동 실험 가이드는 더 이상 제품 문서에 포함하지 않습니다.
