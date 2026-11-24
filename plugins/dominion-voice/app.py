"""Independent DOMINION Voice service. No owner API token or filesystem access."""
import hashlib
import hmac
import json
import os
import secrets
import threading
import uuid
from collections import OrderedDict, deque
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path

MAX_BODY = 512 * 1024
ROOT = Path(__file__).resolve().parent
TOKEN = os.environ.get("DOMINION_BRIDGE_TOKEN", "")
UI_TOKEN = secrets.token_hex(32)


class Conversation:
    def __init__(self):
        self.lock = threading.RLock()
        self.pending = None
        self.answers = OrderedDict()
        self.history = deque(maxlen=3)
        self.last = None

    def submit(self, text, source):
        if not isinstance(text, str) or not text.strip() or len(text.encode("utf-8")) > 6000:
            raise ValueError("발화를 1~6000 UTF-8 바이트로 입력하세요.")
        with self.lock:
            if self.pending:
                raise ValueError("이전 요청의 답변을 기다리고 있습니다.")
            # History is conversation data, not a new tool or authority grant.
            history = json.dumps(list(self.history), ensure_ascii=False)
            if len(history.encode("utf-8")) > 6500:
                history = "[]"
            instruction = (
                "음성 대화입니다. 이전 대화 자료는 참고만 하세요. "
                "현재 발화에 자연스럽고 이해하기 쉽게 답하세요.\n"
                + "이전 대화: " + history + "\n현재 발화: " + text.strip()
            )
            rid = str(uuid.uuid4())
            self.pending = {"requestId": rid, "instruction": instruction,
                            "text": text.strip(), "source": source}
            return rid

    def request(self):
        with self.lock:
            if not self.pending:
                return None
            return {k: self.pending[k] for k in ("requestId", "instruction")}

    def accept(self, data):
        rid, answer, digest = (data.get(k) for k in ("requestId", "answer", "answerDigest"))
        if not isinstance(rid, str) or not isinstance(answer, str) or not isinstance(digest, str):
            raise ValueError("Invalid answer")
        actual = hashlib.sha256(answer.encode("utf-8")).hexdigest()
        if not hmac.compare_digest(actual, digest):
            raise ValueError("Answer digest mismatch")
        with self.lock:
            previous = self.answers.get(rid)
            if previous:
                if not hmac.compare_digest(previous["answerDigest"], digest):
                    raise ValueError("Conflicting duplicate answer")
                return {"requestId": rid, "answerDigest": digest}
            if not self.pending or self.pending["requestId"] != rid:
                raise ValueError("No matching pending request")
            result = {"requestId": rid, "answer": answer, "answerDigest": digest,
                      "source": self.pending["source"]}
            self.history.append({"user": self.pending["text"][:1500], "assistant": answer[:1500]})
            self.answers[rid] = result
            while len(self.answers) > 32:
                self.answers.popitem(last=False)
            self.last = result
            self.pending = None
            return {"requestId": rid, "answerDigest": digest}

    def answer(self, rid):
        with self.lock:
            result = self.answers.get(rid)
            return result["answer"] if result else None

    def status(self):
        with self.lock:
            return {"pending": self.pending.copy() if self.pending else None,
                    "last": self.last.copy() if self.last else None}

    def discard(self, rid):
        with self.lock:
            if not self.pending or self.pending["requestId"] != rid:
                raise ValueError("Request changed")
            self.pending = None


conversation = Conversation()
live_lock = threading.Lock()
live = {"state": "stopped"}
live_stop = threading.Event()


def live_start(config):
    names = ("url", "room", "token", "sttKey", "ttsKey", "voice", "language")
    if not all(isinstance(config.get(k), str) and 0 < len(config[k]) <= 4096 for k in names):
        raise ValueError("LiveKit와 음성 모델 설정을 입력하세요.")
    if not config["url"].startswith("wss://") or config["language"] not in ("ko", "en"):
        raise ValueError("LiveKit은 wss:// 주소와 ko/en 언어를 사용하세요.")
    with live_lock:
        if live["state"] in ("starting", "running"):
            raise ValueError("먼저 기존 음성 세션을 중지하세요.")
        live["state"] = "starting"
        live_stop.clear()
    def run():
        try:
            import asyncio
            from voice import run_live
            with live_lock:
                live["state"] = "running"
            asyncio.run(run_live(conversation, {k: config[k] for k in names}, live_stop))
            with live_lock:
                live["state"] = "stopped"
        except Exception:
            # Do not disclose provider credentials or signed room URLs.
            with live_lock:
                live["state"] = "failed"
                live["error"] = "LiveKit 또는 음성 공급자 연결 실패. 설정과 로그를 확인하세요."
    threading.Thread(target=run, daemon=True).start()


class Handler(BaseHTTPRequestHandler):
    server_version = "DOMINION-Voice"
    def log_message(self, *args):
        pass

    def local_host(self):
        host = self.headers.get("Host", "").split(":")[0].lower()
        return host in ("127.0.0.1", "localhost")

    def bridge_allowed(self):
        supplied = self.headers.get("Authorization", "")
        return bool(TOKEN) and hmac.compare_digest(supplied, "Bearer " + TOKEN)

    def ui_allowed(self):
        origin = self.headers.get("Origin")
        expected = "http://" + self.headers.get("Host", "")
        return self.local_host() and (origin is None or origin == expected) and hmac.compare_digest(
            self.headers.get("X-Voice-Session", ""), UI_TOKEN)

    def reply(self, code, data=None, content_type="application/json"):
        if data is None:
            raw = b""
        elif isinstance(data, bytes):
            raw = data
        else:
            raw = json.dumps(data, ensure_ascii=False).encode("utf-8")
        self.send_response(code)
        self.send_header("Content-Type", content_type)
        self.send_header("Content-Length", str(len(raw)))
        self.send_header("Cache-Control", "no-store")
        self.send_header("X-Content-Type-Options", "nosniff")
        self.send_header("Content-Security-Policy", "default-src 'self'; script-src 'self'; style-src 'self' 'unsafe-inline'; frame-ancestors 'none'; base-uri 'none'; form-action 'self'")
        self.end_headers()
        if raw:
            self.wfile.write(raw)

    def body(self):
        if self.headers.get("Content-Type", "").split(";")[0] != "application/json":
            raise ValueError("JSON required")
        n = int(self.headers.get("Content-Length", "0"))
        if not 0 < n <= MAX_BODY:
            raise ValueError("Invalid body length")
        data = json.loads(self.rfile.read(n))
        if not isinstance(data, dict):
            raise ValueError("JSON object required")
        return data

    def do_GET(self):
        if not self.local_host():
            self.reply(403, {"error": "Local host required"})
            return
        if self.path == "/dominion/v1/request":
            if not self.bridge_allowed():
                self.reply(401, {"error": "Bridge authentication required"})
                return
            request = conversation.request()
            self.reply(200 if request else 204, request)
        elif self.path == "/":
            page = (ROOT / "index.html").read_text().replace("__VOICE_SESSION__", UI_TOKEN)
            self.reply(200, page.encode(), "text/html; charset=utf-8")
        elif self.path == "/app.js":
            self.reply(200, (ROOT / "app.js").read_bytes(), "application/javascript")
        elif self.path == "/status" and self.ui_allowed():
            with live_lock:
                state = dict(live)
            self.reply(200, {**conversation.status(), "live": state})
        else:
            self.reply(404, {"error": "Not found"})

    def do_POST(self):
        if not self.local_host():
            self.reply(403, {"error": "Local host required"})
            return
        try:
            if self.path == "/dominion/v1/result":
                if not self.bridge_allowed():
                    self.reply(401, {"error": "Bridge authentication required"})
                    return
                self.reply(200, conversation.accept(self.body()))
                return
            if not self.ui_allowed():
                self.reply(403, {"error": "Local session required"})
                return
            data = self.body()
            if self.path == "/submit":
                self.reply(201, {"requestId": conversation.submit(data.get("text"), "browser")})
            elif self.path == "/discard":
                conversation.discard(data.get("requestId"))
                self.reply(200, {"ok": True})
            elif self.path == "/live/start":
                live_start(data)
                self.reply(202, {"state": "starting"})
            elif self.path == "/live/stop":
                live_stop.set()
                self.reply(200, {"state": "stopping"})
            else:
                self.reply(404, {"error": "Not found"})
        except (ValueError, UnicodeError, json.JSONDecodeError) as exc:
            self.reply(400, {"error": str(exc)[:300]})


if __name__ == "__main__":
    if not TOKEN:
        raise SystemExit("DOMINION bridge credential required")
    ThreadingHTTPServer(("0.0.0.0", 8080), Handler).serve_forever()
