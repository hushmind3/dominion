"""Owner-initiated LiveKit mobile and single-call sessions.

No server credentials are included in mobile pairing. Inbound is deliberately
an existing, dedicated SIP room routed by the operator's configured trunk.
"""
import re
import time
import uuid
from datetime import timedelta
from urllib.parse import urlsplit


def phone_number(s):
    return isinstance(s, str) and re.fullmatch(r"\+[1-9][0-9]{7,14}", s) is not None


def prepare(data, api_module=None):
    mode = data.get("mode")
    if mode not in ("mobile", "outbound", "inbound"):
        raise ValueError("지원하는 연결 방식을 선택하세요.")
    fields = ("url", "apiKey", "apiSecret", "sttKey", "ttsKey", "voice", "language")
    if any(not isinstance(data.get(k), str) or not data[k].strip() or len(data[k]) > 4096 for k in fields):
        raise ValueError("LiveKit API 키와 음성 공급자 설정을 입력하세요.")
    url = urlsplit(data["url"])
    if url.scheme != "wss" or not url.hostname or url.username or url.password or url.query or url.fragment:
        raise ValueError("LiveKit의 wss:// 서버 주소를 입력하세요.")
    if data["language"] not in ("ko","en"):
        raise ValueError("지원하는 언어를 선택하세요.")
    cfg = {k: data[k] for k in fields}
    cfg["mode"] = mode
    nonce = uuid.uuid4().hex
    cfg["room"] = "dominion-" + nonce
    cfg["peer"] = "dominion-peer-" + nonce
    if mode == "outbound":
        if not phone_number(data.get("number")) or not re.fullmatch(r"ST_[A-Za-z0-9_-]{1,100}", str(data.get("trunk",""))):
            raise ValueError("국제 전화번호(+국가번호)와 발신 트렁크 ID를 확인하세요.")
        cfg["number"], cfg["trunk"] = data["number"], data["trunk"]
    elif mode == "inbound":
        room = data.get("room")
        if not isinstance(room,str) or not re.fullmatch(r"dominion-inbound-[A-Za-z0-9_-]{8,80}",room):
            raise ValueError("전용 수신 방 이름(dominion-inbound-...)을 입력하세요.")
        cfg["room"] = room
        if data.get("dedicatedRoomConfirmed") is not True:
            raise ValueError("수신 트렁크와 전용 방의 발신자 제한을 확인하세요.")
    if api_module is None:
        from livekit import api as api_module
    def token(identity, seconds):
        return (api_module.AccessToken(cfg["apiKey"],cfg["apiSecret"])
                .with_identity(identity).with_ttl(timedelta(seconds=seconds))
                .with_grants(api_module.VideoGrants(room_join=True,room=cfg["room"],
                    can_publish=True,can_subscribe=True,can_publish_data=False,
                    can_publish_sources=["microphone"]))
                .to_jwt())
    cfg["token"] = token("dominion-bot-"+nonce,1800)
    pairing = None
    if mode == "mobile":
        pairing = {"url":cfg["url"],"token":token(cfg["peer"],600),"room":cfg["room"],
                   "expiresAt":int(time.time())+600}
        # A mobile session does not require the server secret after token mint.
        cfg.pop("apiKey");cfg.pop("apiSecret")
    return cfg,pairing


async def dial_once(cfg):
    from livekit import api
    from google.protobuf.duration_pb2 import Duration
    # Unique identity and room per user-approved attempt. No app-level retry.
    async with api.LiveKitAPI(url=cfg["url"],api_key=cfg["apiKey"],api_secret=cfg["apiSecret"]) as lk:
        return await lk.sip.create_sip_participant(api.CreateSIPParticipantRequest(
            sip_trunk_id=cfg["trunk"],sip_call_to=cfg["number"],room_name=cfg["room"],
            participant_identity=cfg["peer"],wait_until_answered=True,
            ringing_timeout=Duration(seconds=30),max_call_duration=Duration(seconds=600)))


async def close_phone(cfg, participants):
    from livekit import api
    async with api.LiveKitAPI(url=cfg["url"],api_key=cfg["apiKey"],api_secret=cfg["apiSecret"]) as lk:
        if cfg["mode"] == "outbound":
            # We generated this room solely for the single outbound call.
            await lk.room.delete_room(api.DeleteRoomRequest(room=cfg["room"]))
        else:
            # Never delete the externally configured inbound dispatch room.
            # Remove only SIP participants that were observed in this session.
            listed = await lk.room.list_participants(api.ListParticipantsRequest(room=cfg["room"]))
            for peer in listed.participants:
                if peer.identity in participants and peer.kind == api.ParticipantInfo.SIP:
                    await lk.room.remove_participant(api.RoomParticipantIdentity(room=cfg["room"],identity=peer.identity))
