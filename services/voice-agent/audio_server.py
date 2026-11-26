"""Local audio gateway: OpenAI-compatible STT and TTS over local models.

LiveKit's OpenAI plugins only need `/v1/audio/transcriptions` and
`/v1/audio/speech`. whisper.cpp and Piper speak different protocols, so this
process is the single translation point for both.
"""
import os
import subprocess
import tempfile

import httpx
from fastapi import FastAPI, File, Form, HTTPException, UploadFile
from fastapi.responses import Response
from pydantic import BaseModel

PIPER_BIN = os.getenv("PIPER_BIN", "piper")
VOICE_DIR = os.path.expanduser(os.getenv("PIPER_VOICE_DIR", "~/Models/piper"))
DEFAULT_VOICE = os.getenv("PIPER_VOICE", "ko_KR-kss-medium")
WHISPER_URL = os.getenv("WHISPER_URL", "http://127.0.0.1:8081/inference")
WHISPER_TIMEOUT = float(os.getenv("WHISPER_TIMEOUT", "120"))

app = FastAPI(title="Dominion local audio gateway")


class SpeechRequest(BaseModel):
    model: str | None = None
    input: str
    voice: str | None = None
    response_format: str | None = "wav"
    speed: float | None = None


def voice_path(voice: str) -> str:
    path = os.path.join(VOICE_DIR, f"{voice}.onnx")
    if not os.path.isfile(path):
        raise HTTPException(status_code=400, detail=f"unknown voice: {voice}")
    return path


@app.get("/v1/models")
def list_models() -> dict:
    voices = sorted(
        name[: -len(".onnx")]
        for name in os.listdir(VOICE_DIR)
        if name.endswith(".onnx")
    )
    return {"object": "list", "data": [{"id": v, "object": "model"} for v in voices]}


@app.post("/v1/audio/transcriptions")
async def transcribe(
    file: UploadFile = File(...),
    model: str = Form("whisper-1"),
    language: str | None = Form(None),
) -> dict:
    payload = await file.read()
    if not payload:
        raise HTTPException(status_code=400, detail="empty audio")

    files = {"file": (file.filename or "audio.wav", payload, "audio/wav")}
    async with httpx.AsyncClient(timeout=WHISPER_TIMEOUT) as client:
        response = await client.post(WHISPER_URL, files=files)
    if response.status_code != 200:
        raise HTTPException(status_code=502, detail=response.text[-400:])
    return {"text": response.json().get("text", "").strip()}


@app.post("/v1/audio/speech")
def synthesize(request: SpeechRequest) -> Response:
    text = request.input.strip()
    if not text:
        raise HTTPException(status_code=400, detail="input is empty")

    handle, wav_path = tempfile.mkstemp(suffix=".wav")
    os.close(handle)
    try:
        result = subprocess.run(
            [PIPER_BIN, "-m", voice_path(request.voice or DEFAULT_VOICE), "-f", wav_path],
            input=text.encode("utf-8"),
            capture_output=True,
        )
        if result.returncode != 0:
            raise HTTPException(
                status_code=500,
                detail=result.stderr.decode("utf-8", "replace")[-400:],
            )
        with open(wav_path, "rb") as audio:
            return Response(content=audio.read(), media_type="audio/wav")
    finally:
        os.unlink(wav_path)
