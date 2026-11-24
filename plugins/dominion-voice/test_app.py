import hashlib
import unittest
from app import Conversation

class ConversationTests(unittest.TestCase):
    def test_one_pending_and_idempotent_verified_delivery(self):
        c=Conversation()
        rid=c.submit("안녕","browser")
        self.assertEqual(c.request()["requestId"],rid)
        with self.assertRaises(ValueError): c.submit("second","live")
        answer="반갑습니다"
        digest=hashlib.sha256(answer.encode()).hexdigest()
        with self.assertRaises(ValueError): c.accept({"requestId":rid,"answer":answer,"answerDigest":"bad"})
        data={"requestId":rid,"answer":answer,"answerDigest":digest}
        self.assertEqual(c.accept(data),c.accept(data))
        self.assertIsNone(c.request())
        self.assertEqual(c.answer(rid),answer)
        new=c.submit("아까 뭐라고 했지?","live")
        self.assertNotEqual(rid,new)
        self.assertIn(answer,c.request()["instruction"])
    def test_discard_does_not_accept_stale_response(self):
        c=Conversation()
        rid=c.submit("hello","browser")
        c.discard(rid)
        with self.assertRaises(ValueError):
            c.accept({"requestId":rid,"answer":"ok","answerDigest":hashlib.sha256(b"ok").hexdigest()})

if __name__=="__main__":
    unittest.main()
