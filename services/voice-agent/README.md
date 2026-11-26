# Dominion voice agent

음성 통화용 로컬 파이프라인입니다. 미디어는 LiveKit가 나르고, 모델은 전부 맥에서 돕니다.

```
Android (A-4)  --WebRTC-->  livekit-server :7880
                                  |
                          livekit-agents (agent.py)
                    /             |              \
        Silero VAD      whisper.cpp :8081     llama.cpp :8080
                                  |                    |
                          Piper TTS :8082        Ornith 9B Q4
```

## 준비물

| 구성 | 위치 | 비고 |
| --- | --- | --- |
| LiveKit 서버 | `brew install livekit` | `~/.config/livekit/livekit.yaml` |
| LiveKit CLI | `brew install livekit-cli` | 통화 테스트용 |
| STT | `brew install whisper-cpp` | `~/Models/whisper/ggml-large-v3-turbo.bin` |
| LLM | `llama-server` | `~/Models/ornith-1.5-9b-mtp/*.gguf` |
| TTS | `uv` + `piper-tts` | `~/Models/piper/ko_KR-kss-medium.onnx` |

## 실행

```bash
# 1) 모델 서버
llama-server -m ~/Models/ornith-1.5-9b-mtp/Ornith-1.5-9B-MTP-Q4_K_M.gguf \
  --host 127.0.0.1 --port 8080 -c 8192 -ngl 99 --jinja --no-webui
livekit-server --config ~/.config/livekit/livekit.yaml

# 2) STT/TTS + 에이전트
cd services/voice-agent
./run.sh
```

`run.sh`는 whisper-server(:8081, `-l ko`)와 Piper TTS(:8082)를 띄운 뒤
`agent.py`를 dev 모드로 실행합니다.

## 환경 변수

| 변수 | 기본값 |
| --- | --- |
| `LIVEKIT_URL` | `ws://127.0.0.1:7880` |
| `LIVEKIT_API_KEY` / `LIVEKIT_API_SECRET` | `dominion` / 개발용 시크릿 |
| `DOMINION_LLM_BASE` | `http://127.0.0.1:8080/v1` |
| `DOMINION_STT_BASE` | `http://127.0.0.1:8081/v1` |
| `DOMINION_TTS_BASE` | `http://127.0.0.1:8082/v1` |
| `DOMINION_MODEL` | `ornith-1.5-9b` |
| `PIPER_VOICE` | `ko_KR-kss-medium` |

## 알려진 제약

- whisper.cpp 서버는 요청별 `language`를 무시합니다. 반드시 `-l ko`로 띄워야 합니다.
- 현재 llama.cpp 빌드는 Ornith의 MTP 드래프트 헤드를 무시합니다(`unused tensor blk.32.nextn.*`).
  동작에는 문제가 없지만 MTP 가속은 적용되지 않습니다.
- 9B Q4 기준 M5에서 초당 약 19토큰입니다. 턴 단위 대화에는 충분하지만 실시간 양방향에는 부족합니다.
