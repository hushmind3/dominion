"""Pipecat 1.17 / LiveKit standard STT → DOMINION → TTS pipeline.

The only reasoning service is the DOMINION bridge. All room and speech
credentials stay within this process. A private single-user room is required.
"""
import asyncio
import time

async def run_live(conversation, config, stop):
    from pipecat.audio.vad.silero import SileroVADAnalyzer
    from pipecat.frames.frames import LLMContextFrame, TTSSpeakFrame
    from pipecat.pipeline.pipeline import Pipeline
    from pipecat.pipeline.worker import PipelineParams, PipelineWorker
    from pipecat.processors.aggregators.llm_context import LLMContext
    from pipecat.processors.aggregators.llm_response_universal import LLMContextAggregatorPair, LLMUserAggregatorParams
    from pipecat.processors.frame_processor import FrameProcessor, FrameDirection
    from pipecat.services.cartesia.tts import CartesiaTTSService
    from pipecat.services.deepgram.stt import DeepgramSTTService
    from pipecat.transports.livekit.transport import LiveKitParams, LiveKitTransport
    from pipecat.workers.runner import WorkerRunner
    from loguru import logger

    # SDK debug logs may include speech transcripts or provider metadata.
    logger.remove()
    transport = LiveKitTransport(url=config["url"], token=config["token"],
        room_name=config["room"], params=LiveKitParams(audio_in_enabled=True, audio_out_enabled=True))
    stt = DeepgramSTTService(api_key=config["sttKey"],
        settings=DeepgramSTTService.Settings(language=config["language"], model="nova-3"))
    tts = CartesiaTTSService(api_key=config["ttsKey"],
        settings=CartesiaTTSService.Settings(voice=config["voice"], language=config["language"]))
    context = LLMContext()
    user, _ = LLMContextAggregatorPair(context, user_params=LLMUserAggregatorParams(
        vad_analyzer=SileroVADAnalyzer()))

    class Dominion(FrameProcessor):
        def __init__(self):
            super().__init__()
            self.last_count = 0

        async def process_frame(self, frame, direction):
            await super().process_frame(frame, direction)
            if not isinstance(frame, LLMContextFrame) or direction != FrameDirection.DOWNSTREAM:
                await self.push_frame(frame, direction)
                return
            if getattr(frame, "speculation", False):
                return
            messages = frame.context.get_messages()
            if len(messages) <= self.last_count:
                return
            self.last_count = len(messages)
            latest = next((m for m in reversed(messages) if isinstance(m, dict) and m.get("role") == "user"), None)
            if not latest:
                return
            text = latest.get("content")
            if isinstance(text, list):
                text = " ".join(x.get("text", "") for x in text if isinstance(x, dict) and x.get("type") == "text")
            if not isinstance(text, str) or not text.strip():
                return
            try:
                rid = conversation.submit(text, "live")
            except ValueError:
                await self.push_frame(TTSSpeakFrame("이전 답변을 기다리고 있습니다." if config["language"] == "ko" else "Waiting for the previous answer."))
                return
            self.create_task(self.wait_answer(rid))

        async def wait_answer(self, rid):
            deadline = time.monotonic() + 180
            while time.monotonic() < deadline and not stop.is_set():
                result = conversation.answer(rid)
                if result is not None:
                    if result:
                        await self.push_frame(TTSSpeakFrame(result[:8000]))
                    return
                if not conversation.status()["pending"]:
                    return
                await asyncio.sleep(0.35)
            if not stop.is_set():
                await self.push_frame(TTSSpeakFrame("답변이 아직 준비되지 않았습니다. 도미니온 작업 상태를 확인하세요." if config["language"] == "ko" else "The answer is not ready. Check the DOMINION task."))

    worker = PipelineWorker(Pipeline([transport.input(), stt, user, Dominion(), tts, transport.output()]),
        params=PipelineParams(enable_metrics=True, enable_usage_metrics=True))
    runner = WorkerRunner()
    await runner.add_workers(worker)
    task = asyncio.create_task(runner.run())
    deadline = time.monotonic() + 1800
    try:
        while not task.done() and not stop.is_set() and time.monotonic() < deadline:
            await asyncio.sleep(0.5)
        if task.done():
            await task
    finally:
        if not task.done():
            task.cancel()
            await asyncio.gather(task, return_exceptions=True)
