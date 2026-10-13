import base64
import hashlib
import hmac
import os
import unittest
from unittest.mock import patch
from fastapi import HTTPException
from app.api.calls import ice_configuration


class CallIceTests(unittest.TestCase):
    def test_unconfigured_relay_keeps_stun(self):
        with patch.dict(os.environ, TURN_URLS=""):
            self.assertEqual(ice_configuration("user"), {"iceServers": [{"urls": "stun:stun.l.google.com:19302"}]})

    def test_credentials_are_time_limited_and_user_scoped(self):
        with patch.dict(os.environ, TURN_URLS="turn:relay:3478?transport=udp,turn:relay:3478?transport=tcp", TURN_SECRET_FILE="/test-secret"), patch("app.api.calls.Path.read_text", return_value="test-secret\n"), patch("app.api.calls.time.time", return_value=1000):
            config = ice_configuration("alice")
            other = ice_configuration("bob")
        relay = config["iceServers"][1]
        self.assertEqual(relay["username"], "4600:alice")
        expected = base64.b64encode(hmac.new(b"test-secret", b"4600:alice", hashlib.sha1).digest()).decode()
        self.assertEqual(relay["credential"], expected)
        self.assertNotEqual(relay["credential"], other["iceServers"][1]["credential"])
        self.assertNotIn("test-secret", str(config))
        self.assertEqual(len(relay["urls"]), 2)

    def test_configured_but_missing_secret_fails_explicitly(self):
        with patch.dict(os.environ, TURN_URLS="turn:relay:3478", TURN_SECRET_FILE="/missing"), patch("app.api.calls.Path.read_text", side_effect=FileNotFoundError):
            with self.assertRaises(HTTPException) as result:
                ice_configuration("user")
        self.assertEqual(result.exception.status_code, 503)
