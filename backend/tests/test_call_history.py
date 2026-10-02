import asyncio
import unittest
import uuid
from types import SimpleNamespace
from unittest.mock import AsyncMock, patch

from app.api import ws
from app.realtime import call_history


class Socket:
    def __init__(self):
        self.events = []

    async def send_json(self, data):
        self.events.append(data)


class CallHistoryTests(unittest.IsolatedAsyncioTestCase):
    async def asyncSetUp(self):
        self.a, self.b = uuid.uuid4(), uuid.uuid4()
        self.sa, self.sb = Socket(), Socket()
        self.cid = str(uuid.uuid4())
        ws.active_calls[self.cid] = (self.a, self.b, self.sa, self.sb)

    async def asyncTearDown(self):
        ws.active_calls.clear()
        ws.call_started.clear()

    async def test_connected_duration_and_duplicate_hangup(self):
        with patch.object(ws, 'save_call_message', new_callable=AsyncMock) as save, patch.object(ws, 'monotonic', return_value=100):
            await ws.handle_call_signal(self.sa, SimpleNamespace(id=self.a), {
                'type': 'call.connected', 'call_id': self.cid, 'peer_id': str(self.b),
            })
            self.assertEqual(self.sb.events[-1]['type'], 'call.started')
            with patch.object(ws, 'monotonic', return_value=235):
                await asyncio.gather(ws.finish_call(self.cid, 'cancelled'), ws.finish_call(self.cid, 'cancelled'))
            save.assert_awaited_once_with(self.cid, self.a, self.b, 135, 'cancelled')

    async def test_repeated_connection_does_not_reset_timer(self):
        ws.call_started[self.cid] = (100, '2026-10-02T00:00:00+00:00')
        await ws.handle_call_signal(self.sa, SimpleNamespace(id=self.a), {
            'type': 'call.connected', 'call_id': self.cid, 'peer_id': str(self.b),
        })
        self.assertEqual(ws.call_started[self.cid][0], 100)

    async def test_wrong_tab_cannot_start_timer(self):
        await ws.handle_call_signal(Socket(), SimpleNamespace(id=self.a), {
            'type': 'call.connected', 'call_id': self.cid, 'peer_id': str(self.b),
        })
        self.assertNotIn(self.cid, ws.call_started)

    async def test_declined_call_has_no_duration(self):
        ws.active_calls[self.cid] = (self.a, self.b, self.sa, None)
        with patch.object(ws, 'save_call_message', new_callable=AsyncMock) as save:
            await ws.handle_call_signal(self.sb, SimpleNamespace(id=self.b), {
                'type': 'call.end', 'call_id': self.cid, 'peer_id': str(self.a),
            })
            save.assert_awaited_once_with(self.cid, self.a, self.b, None, 'declined')

    def test_summary(self):
        self.assertEqual(call_history.call_summary(135, 'cancelled'), 'Audio call · 2 min 15 sec')
        self.assertEqual(call_history.call_summary(0, 'cancelled'), 'Audio call · 0 min 00 sec')
        self.assertEqual(call_history.call_summary(None, 'timeout'), 'Missed audio call')
        self.assertEqual(call_history.call_summary(None, 'failed'), 'Audio call · Could not connect')

    async def test_history_is_committed_and_broadcast(self):
        db = AsyncMock()
        db.scalar.return_value = uuid.uuid4()
        db.get.return_value = None
        from unittest.mock import Mock
        db.add = Mock()
        session = AsyncMock()
        session.__aenter__.return_value = db
        with patch.object(call_history, 'AsyncSessionLocal', return_value=session), patch.object(call_history, 'build_message_response', return_value='response'), patch.object(call_history, 'broadcast_message', new_callable=AsyncMock) as broadcast:
            await call_history.save_call_message(self.cid, self.a, self.b, 135, 'cancelled')
            message = db.add.call_args.args[0]
            self.assertEqual(message.content, 'Audio call · 2 min 15 sec')
            self.assertEqual(message.sender_id, self.a)
            db.commit.assert_awaited_once()
            broadcast.assert_awaited_once_with(db, db.scalar.return_value, 'response')


if __name__ == '__main__':
    unittest.main()
