"""Dominion voice agent.

Pipeline: Silero VAD -> whisper.cpp (STT) -> llama.cpp/Ornith (LLM) -> Piper (TTS).
Every model is local; only LiveKit carries the media.
"""
import os

from livekit.agents import Agent, AgentSession, JobContext, WorkerOptions, cli
from livekit.plugins import openai, silero

LLM_BASE = os.getenv("DOMINION_LLM_BASE", "http://127.0.0.1:8080/v1")
STT_BASE = os.getenv("DOMINION_STT_BASE", "http://127.0.0.1:8082/v1")
TTS_BASE = os.getenv("DOMINION_TTS_BASE", "http://127.0.0.1:8082/v1")
MODEL = os.getenv("DOMINION_MODEL", "ornith-1.5-9b")
VOICE = os.getenv("PIPER_VOICE", "ko_KR-kss-medium")

INSTRUCTIONS = """
당신은 '도미니언'입니다. 사용자의 맥북에서 돌아가는 개인 비서이고,
지금은 음성 통화로 대화하고 있습니다.

규칙:
- 한국어로 말합니다.
- 답은 두 문장 이내로 짧게 합니다. 음성이라 길면 듣기 어렵습니다.
- 모르는 것은 지어내지 말고 모른다고 말합니다.
- 실행을 요청받으면 무엇을 할지 한 문장으로 확인한 뒤 진행합니다.
"""


async def entrypoint(ctx: JobContext) -> None:
    await ctx.connect()

    session = AgentSession(
        vad=silero.VAD.load(),
        # The default adaptive detector calls LiveKit Cloud; keep everything local.
        turn_detection="vad",
        stt=openai.STT(
            model="whisper-1",
            base_url=STT_BASE,
            api_key="local",
            language="ko",
        ),
        llm=openai.LLM(
            model=MODEL,
            base_url=LLM_BASE,
            api_key="local",
        ),
        tts=openai.TTS(
            model="tts-1",
            voice=VOICE,
            base_url=TTS_BASE,
            api_key="local",
        ),
    )

    await session.start(
        room=ctx.room,
        agent=Agent(instructions=INSTRUCTIONS),
    )
    await session.generate_reply(
        user_input="전화를 걸었습니다. 짧게 인사해 주세요.",
    )


if __name__ == "__main__":
    cli.run_app(WorkerOptions(entrypoint_fnc=entrypoint))
