"""Pipecat 1.17 / LiveKit standard STT → DOMINION → TTS pipeline.

The only reasoning service is the DOMINION bridge. All room and speech
credentials stay within this process. A private single-user room is required.
"""
import asyncio
import time

async def run_live(conversation, config, stop, report=lambda state: None):
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
    runner = WorkerRunner(handle_sigint=False)
    await runner.add_workers(worker)
    ready = asyncio.Event()
    participants = set()
    first = asyncio.Event()
    mode = config.get("mode", "manual")

    @transport.event_handler("on_connected")
    async def connected(transport):
        ready.set()

    @transport.event_handler("on_participant_connected")
    async def joined(transport, participant_id):
        participants.add(participant_id)
        if mode == "mobile" and participant_id != config["peer"]:
            stop.set()
            return
        if mode == "outbound" and participant_id != config["peer"]:
            stop.set()
            return
        if len(participants) > 1:
            stop.set()
            return
        first.set()
        if mode == "inbound":
            report("connected")

    @transport.event_handler("on_participant_disconnected")
    async def left(transport, participant_id):
        participants.discard(participant_id)
        if mode != "manual":
            stop.set()

    @transport.event_handler("on_disconnected")
    async def disconnected(transport):
        stop.set()

    task = asyncio.create_task(runner.run())
    deadline = time.monotonic() + (600 if mode in ("outbound", "inbound") else 1800)
    dial = None
    try:
        while not ready.is_set() and not task.done() and not stop.is_set():
            if time.monotonic() > deadline - (570 if mode in ("outbound", "inbound") else 1770):
                raise RuntimeError("LiveKit connection timeout")
            await asyncio.sleep(0.1)
        if task.done():
            await task
            raise RuntimeError("Voice worker exited before connection")
        if stop.is_set():
            return
        if mode == "outbound":
            from session import dial_once
            report("dialing")
            dial = asyncio.create_task(dial_once(config))
        else:
            report("waiting" if mode in ("mobile","inbound") else "running")
        while not task.done() and not stop.is_set() and time.monotonic() < deadline:
            if dial is not None and dial.done():
                await dial
                report("connected")
                dial = None
            if mode == "mobile" and first.is_set():
                report("connected")
            await asyncio.sleep(0.25)
        if task.done():
            await task
    finally:
        # Never issue a second dial on an uncertain result. Terminate the
        # uniquely allocated outbound room, even after timeout or cancellation.
        if dial is not None:
            if not dial.done():
                dial.cancel()
            await asyncio.gather(dial, return_exceptions=True)
        try:
            if not task.done():
                await runner.cancel()
                await asyncio.wait_for(task, timeout=15)
        finally:
            if mode in ("outbound","inbound"):
                from session import close_phone
                await close_phone(config, participants)
