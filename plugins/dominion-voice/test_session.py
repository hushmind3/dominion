import unittest
from session import prepare,phone_number

class FakeAPI:
    class VideoGrants:
        def __init__(self, **kw): self.kw=kw
    class AccessToken:
        def __init__(self,*_): pass
        def with_identity(self,identity): self.identity=identity;return self
        def with_ttl(self,ttl): self.ttl=ttl;return self
        def with_grants(self,grant): self.grant=grant.kw;return self
        def to_jwt(self): return self.identity+"@"+self.grant["room"]

def config(mode):
    return dict(mode=mode,url="wss://example.org",apiKey="key",apiSecret="private",sttKey="stt",ttsKey="tts",voice="voice",language="ko")

class SessionTests(unittest.TestCase):
    def test_mobile_scope_and_no_secret(self):
        cfg,pair=prepare(config("mobile"),FakeAPI)
        self.assertEqual(set(pair),{"url","room","token","expiresAt"})
        self.assertNotIn("apiSecret",cfg)
        self.assertNotIn("private",str(pair))
        self.assertNotEqual(cfg["token"],pair["token"])
        self.assertEqual(cfg["room"],pair["room"])
        self.assertNotEqual(prepare(config("mobile"),FakeAPI)[0]["room"],cfg["room"])

    def test_sip_requires_explicit_valid_target(self):
        self.assertFalse(phone_number("112"))
        with self.assertRaises(ValueError): prepare(config("outbound"),FakeAPI)
        cfg,pair=prepare(dict(config("outbound"),number="+821012345678",trunk="ST_example"),FakeAPI)
        self.assertIsNone(pair)
        self.assertEqual(cfg["number"],"+821012345678")
        with self.assertRaises(ValueError): prepare(dict(config("inbound"),room="existing"),FakeAPI)
        with self.assertRaises(ValueError): prepare(dict(config("inbound"),room="dominion-inbound-private1"),FakeAPI)
        cfg,pair=prepare(dict(config("inbound"),room="dominion-inbound-private1",dedicatedRoomConfirmed=True),FakeAPI)
        self.assertEqual(cfg["room"],"dominion-inbound-private1")
        self.assertIsNone(pair)

if __name__=="__main__": unittest.main()
