#!/usr/bin/env bash
# Start the local voice stack: whisper.cpp STT, Piper TTS, LiveKit agent.
# LiveKit server and llama-server are managed separately (launchd).
set -euo pipefail
cd "$(dirname "$0")"

MODELS="${DOMINION_MODELS:-$HOME/Models}"
WHISPER_MODEL="$MODELS/whisper/ggml-base.bin"
AUDIO_PORT=8082

export LIVEKIT_URL="${LIVEKIT_URL:-ws://127.0.0.1:7880}"
export LIVEKIT_API_KEY="${LIVEKIT_API_KEY:-dominion}"
export LIVEKIT_API_SECRET="${LIVEKIT_API_SECRET:-dominion-local-dev-secret-0123456789}"
export DOMINION_LLM_BASE="${DOMINION_LLM_BASE:-http://127.0.0.1:8080/v1}"
# STT and TTS both go through the local audio gateway.
export DOMINION_STT_BASE="${DOMINION_STT_BASE:-http://127.0.0.1:$AUDIO_PORT/v1}"
export DOMINION_TTS_BASE="${DOMINION_TTS_BASE:-http://127.0.0.1:$AUDIO_PORT/v1}"

if ! curl -sf -m 2 http://127.0.0.1:8080/health >/dev/null; then
  echo "llama-server is not running on :8080" >&2
  exit 1
fi
if ! curl -sf -m 2 http://127.0.0.1:7880/ >/dev/null; then
  echo "livekit-server is not running on :7880" >&2
  exit 1
fi

# whisper.cpp's server ignores the per-request language, so pin Korean here.
if ! curl -sf -m 2 http://127.0.0.1:8081/ >/dev/null; then
  whisper-server -m "$WHISPER_MODEL" --host 127.0.0.1 --port 8081 -l ko \
    > /private/tmp/whisper-server.log 2>&1 &
fi
if ! curl -sf -m 2 "http://127.0.0.1:$AUDIO_PORT/v1/models" >/dev/null; then
  uv run uvicorn audio_server:app --host 127.0.0.1 --port "$AUDIO_PORT" \
    > /private/tmp/dominion-audio.log 2>&1 &
fi

for _ in $(seq 1 30); do
  if curl -sf -m 1 http://127.0.0.1:8081/ >/dev/null \
     && curl -sf -m 1 "http://127.0.0.1:$AUDIO_PORT/v1/models" >/dev/null; then
    break
  fi
  sleep 1
done

exec uv run agent.py dev
