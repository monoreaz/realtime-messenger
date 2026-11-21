"""Release blockers from SECURITY_AUDIT.md: private media access (M-1) and group invites (L-8).

Media tests prove that being signed in is not enough: the requesting user must be allowed
to see the specific file (a member of its chat, the message not deleted). They cover
Bearer and cookie authentication, Range requests, reply previews, avatars, revoked
sessions and leaving a group.
"""
import os
import tempfile
import time
import uuid
from datetime import datetime, timedelta, timezone
from pathlib import Path
from unittest.mock import AsyncMock, patch

from test_security import PASSWORD, AuditTestCase, multipart, png

from app.api import auth, chats
from app.core.security import create_access_token, hash_password
from app.models.auth_session import AuthSession
from app.models.chat import Chat, ChatMember
from app.models.user import User
from app.models.user_block import UserBlock
from app.realtime.manager import manager


class MediaAccessTests(AuditTestCase):
    async def asyncSetUp(self):
        await super().asyncSetUp()
        self.c = User(username=f"sec_c_{uuid.uuid4().hex[:10]}", password_hash=hash_password(PASSWORD))
        self.db.add(self.c)
        await self.db.flush()
        self.token_a = create_access_token(str(self.a.id))
        self.token_b = create_access_token(str(self.b.id))
        self.token_c = create_access_token(str(self.c.id))
        self.content = png()
        self.message = await self.upload(self.chat, self.token_a)
        self.url = self.message["image_url"]

    async def upload(self, chat, token, content=None, content_type="image/png", kind="image", **form):
        body, ctype = multipart(content or self.content, content_type, f"file.{content_type.split('/')[1]}")
        status, _, message = await self.call("POST", f"/chats/{chat.id}/messages/{kind}", token=token, body=body, content_type=ctype)
        self.assertEqual(status, 201, message)
        return message

    async def cookie_for(self, user, **session):
        raw = uuid.uuid4().hex
        self.db.add(AuthSession(user_id=user.id, token_hash=auth.hash_raw_token(raw),
                                expires_at=datetime.now(timezone.utc) + timedelta(days=1), **session))
        await self.db.flush()
        return f"{auth.REFRESH_COOKIE_NAME}={raw}"

    async def get(self, url, **kwargs):
        return await self.call("GET", url, **kwargs)

    async def test_members_can_download_with_bearer_or_cookie(self):
        status, headers, data = await self.get(self.url, token=self.token_a)
        self.assertEqual((status, data), (200, self.content))
        self.assertEqual(headers["content-type"], ["image/png"])
        self.assertIn("private", headers["cache-control"][0])
        self.assertEqual(headers["x-content-type-options"], ["nosniff"])
        # Browsers load <img>/<video> with the same-origin refresh cookie.
        status, _, data = await self.get(self.url, cookie=await self.cookie_for(self.b))
        self.assertEqual((status, data), (200, self.content))

    async def test_unrelated_user_cannot_download_another_chats_file(self):
        self.assertEqual((await self.get(self.url, token=self.token_c))[0], 404)
        self.assertEqual((await self.get(self.url, cookie=await self.cookie_for(self.c)))[0], 404)
        # Range requests get the same answer and no bytes.
        status, headers, data = await self.get(self.url, token=self.token_c, headers=[(b"range", b"bytes=0-7")])
        self.assertEqual(status, 404)
        self.assertNotIn("content-range", headers)

    async def test_anonymous_and_revoked_requests_are_rejected(self):
        self.assertEqual((await self.get(self.url))[0], 401)
        self.assertEqual((await self.get(self.url, headers=[(b"range", b"bytes=0-7")]))[0], 401)
        self.assertEqual((await self.get(self.url, cookie=f"{auth.REFRESH_COOKIE_NAME}=forged"))[0], 401)
        revoked = await self.cookie_for(self.b, revoked_at=datetime.now(timezone.utc))
        self.assertEqual((await self.get(self.url, cookie=revoked))[0], 401)
        _, token, cookie = await self.login(self.b)
        await self.call("POST", "/auth/logout", cookie=cookie)
        self.assertEqual((await self.get(self.url, token=token))[0], 401)
        self.b.is_blocked = True
        await self.db.flush()
        self.assertEqual((await self.get(self.url, token=self.token_b))[0], 401)

    async def test_range_requests_are_served_after_the_checks(self):
        status, headers, data = await self.get(self.url, token=self.token_b, headers=[(b"range", b"bytes=0-7")])
        self.assertEqual(status, 206)
        self.assertEqual(data, self.content[:8])
        self.assertEqual(headers["content-range"], [f"bytes 0-7/{len(self.content)}"])

    async def test_videos_and_reply_previews_follow_the_chat(self):
        video = b"\x00\x00\x00\x18ftypmp42" + b"\x00" * 64
        message = await self.upload(self.chat, self.token_a, video, "video/mp4", "video")
        status, headers, data = await self.get(message["video_url"], token=self.token_b, headers=[(b"range", b"bytes=4-11")])
        self.assertEqual((status, data), (206, b"ftypmp42"))
        self.assertEqual((await self.get(message["video_url"], token=self.token_c))[0], 404)
        # A reply shows the replied image with the same URL: still only for members.
        status, _, reply = await self.call("POST", f"/chats/{self.chat.id}/messages", token=self.token_b,
                                           json_body={"content": "nice", "reply_to_message_id": self.message["id"]})
        self.assertEqual(reply["reply_to_message"]["image_url"], self.url)
        self.assertEqual((await self.get(self.url, token=self.token_c))[0], 404)

    async def test_deleted_messages_files_are_gone_for_everyone(self):
        await self.call("DELETE", f"/chats/{self.chat.id}/messages/{self.message['id']}", token=self.token_a)
        self.assertEqual((await self.get(self.url, token=self.token_a))[0], 404)
        self.assertEqual((await self.get(self.url, token=self.token_b))[0], 404)

    async def test_leaving_a_group_ends_access_to_its_files(self):
        group = Chat(type="group", name="G")
        self.db.add(group)
        await self.db.flush()
        self.db.add_all([ChatMember(chat_id=group.id, user_id=user.id, group_role="admin" if user is self.a else "member")
                         for user in (self.a, self.b, self.c)])
        await self.db.flush()
        with patch.object(manager, "send_to_users", new_callable=AsyncMock):
            message = await self.upload(group, self.token_a)
            self.assertEqual((await self.get(message["image_url"], token=self.token_c))[0], 200)
            self.assertEqual((await self.call("POST", f"/chats/{group.id}/leave", token=self.token_c))[0], 204)
        self.assertEqual((await self.get(message["image_url"], token=self.token_c))[0], 404)
        self.assertEqual((await self.get(message["image_url"], token=self.token_b))[0], 200)

    async def test_avatars_need_sign_in_and_must_be_current(self):
        body, ctype = multipart(self.content, "image/png", "me.png")
        status, _, profile = await self.call("POST", "/users/me/avatar", token=self.token_a, body=body, content_type=ctype)
        self.assertEqual(status, 200)
        avatar = profile["avatar_url"]
        self.assertEqual((await self.get(avatar, token=self.token_c))[0], 200)
        self.assertEqual((await self.get(avatar))[0], 401)
        await self.call("DELETE", "/users/me/avatar", token=self.token_a)
        self.assertEqual((await self.get(avatar, token=self.token_a))[0], 404)

    async def test_paths_outside_the_upload_folders_are_refused(self):
        for path in ("/uploads/messages/..%2F..%2Fetc%2Fpasswd", "/uploads/../main.py", "/uploads/other/x.png",
                     "/uploads/messages/x.html", f"{self.url}%00.png"):
            self.assertIn((await self.get(path, token=self.token_a))[0], (404, 422), path)


class GroupInviteTests(AuditTestCase):
    async def asyncSetUp(self):
        await super().asyncSetUp()
        self.c = User(username=f"sec_c_{uuid.uuid4().hex[:10]}", password_hash="unused")
        self.d = User(username=f"sec_d_{uuid.uuid4().hex[:10]}", password_hash="unused")
        self.db.add_all([self.c, self.d])
        await self.db.flush()
        self.token = create_access_token(str(self.a.id))

    async def create(self, members, token=None):
        return await self.call("POST", "/chats/group", token=token or self.token,
                               json_body={"name": "Group", "member_ids": [str(user.id) for user in members]})

    async def test_blocked_users_cannot_be_added_in_either_direction(self):
        self.db.add(UserBlock(blocker_id=self.c.id, blocked_id=self.a.id))
        await self.db.flush()
        status, _, body = await self.create([self.b, self.c])
        self.assertEqual(status, 403)
        self.assertNotIn(self.c.username, str(body))
        self.db.add(UserBlock(blocker_id=self.a.id, blocked_id=self.d.id))
        await self.db.flush()
        self.assertEqual((await self.create([self.b, self.d]))[0], 403)

    async def test_suspended_accounts_cannot_be_added(self):
        self.c.is_blocked = True
        await self.db.flush()
        self.assertEqual((await self.create([self.b, self.c]))[0], 404)

    async def test_group_creation_is_rate_limited(self):
        statuses = [(await self.create([self.b, self.c]))[0] for _ in range(chats.GROUP_CREATE_LIMIT + 2)]
        self.assertEqual(statuses[0], 201)
        self.assertEqual(statuses[-1], 429)

    async def test_members_can_leave_and_admin_role_passes_on(self):
        status, _, group = await self.create([self.b, self.c])
        group_id = group["id"]
        token_b = create_access_token(str(self.b.id))
        for token in (token_b, create_access_token(str(self.c.id))):
            self.assertEqual((await self.call("POST", f"/chats/{group_id}/invitation/accept", token=token))[0], 200)
        # The admin leaves: the longest-standing remaining member becomes admin.
        self.assertEqual((await self.call("POST", f"/chats/{group_id}/leave", token=self.token))[0], 204)
        roles = {member.user_id: member.group_role for member in
                 (await self.db.scalars(chats.select(ChatMember).where(ChatMember.chat_id == uuid.UUID(group_id)))).all()}
        self.assertEqual(set(roles), {self.b.id, self.c.id})
        self.assertIn("admin", roles.values())
        # The leaver can no longer read or post.
        self.assertEqual((await self.call("GET", f"/chats/{group_id}/messages", token=self.token))[0], 404)
        self.assertEqual((await self.call("POST", f"/chats/{group_id}/messages", token=self.token, json_body={"content": "x"}))[0], 404)
        self.assertEqual((await self.call("POST", f"/chats/{group_id}/leave", token=self.token))[0], 404)
        # Leaving a private chat is not a thing.
        self.assertEqual((await self.call("POST", f"/chats/{self.chat.id}/leave", token=token_b))[0], 404)
        # When the last member leaves, the group is deleted.
        await self.call("POST", f"/chats/{group_id}/leave", token=token_b)
        await self.call("POST", f"/chats/{group_id}/leave", token=create_access_token(str(self.c.id)))
        self.assertIsNone(await self.db.get(Chat, uuid.UUID(group_id)))


class OrphanCleanupTests(AuditTestCase):
    async def test_only_old_unreferenced_files_are_reported(self):
        from app.cli import cleanup_uploads
        with tempfile.TemporaryDirectory() as root:
            root = Path(root)
            (root / "messages").mkdir()
            (root / "avatars").mkdir()
            kept = root / "messages" / "kept.png"
            orphan = root / "messages" / "orphan.png"
            fresh = root / "avatars" / "fresh.png"
            for path in (kept, orphan, fresh):
                path.write_bytes(b"x")
            old = time.time() - 3600
            os.utime(kept, (old, old))
            os.utime(orphan, (old, old))
            found = cleanup_uploads.find_orphans({"/uploads/messages/kept.png"}, root)
            self.assertEqual(found, [orphan])


class InvitationConsentTests(AuditTestCase):
    """Groups only invite; nobody becomes a member (or sees anything) without accepting."""

    async def asyncSetUp(self):
        await super().asyncSetUp()
        suffix = uuid.uuid4().hex[:10]
        self.c = User(username=f"sec_c_{suffix}", password_hash="unused")
        self.d = User(username=f"sec_d_{suffix}", password_hash="unused")
        self.db.add_all([self.c, self.d])
        await self.db.flush()
        self.tokens = {user.id: create_access_token(str(user.id)) for user in (self.a, self.b, self.c, self.d)}
        status, _, group = await self.call("POST", "/chats/group", token=self.tokens[self.a.id],
                                           json_body={"name": "Team", "member_ids": [str(self.b.id), str(self.c.id)]})
        self.assertEqual(status, 201)
        self.group_id = group["id"]
        self.assertEqual([member["id"] for member in group["members"]], [str(self.a.id)])
        # The creator posts a text and an image before anyone accepted.
        await self.call("POST", f"/chats/{self.group_id}/messages", token=self.tokens[self.a.id], json_body={"content": "secret"})
        body, ctype = multipart(png(), "image/png", "pic.png")
        status, _, message = await self.call("POST", f"/chats/{self.group_id}/messages/image",
                                             token=self.tokens[self.a.id], body=body, content_type=ctype)
        self.image_url = message["image_url"]

    def token(self, user):
        return self.tokens[user.id]

    async def assert_no_access(self, user):
        g, t = self.group_id, self.token(user)
        self.assertEqual((await self.call("GET", f"/chats/{g}/messages", token=t))[0], 404)
        self.assertEqual((await self.call("POST", f"/chats/{g}/messages", token=t, json_body={"content": "x"}))[0], 404)
        self.assertEqual((await self.call("GET", f"/chats/{g}/pinned-messages", token=t))[0], 404)
        self.assertEqual((await self.call("POST", f"/chats/{g}/read", token=t))[0], 404)
        self.assertEqual((await self.call("GET", self.image_url, token=t))[0], 404)
        self.assertEqual((await self.call("GET", self.image_url, token=t, headers=[(b"range", b"bytes=0-3")]))[0], 404)
        status, _, chats_list = await self.call("GET", "/chats", token=t)
        self.assertNotIn(g, [chat["id"] for chat in chats_list])

    async def test_pending_invitation_grants_no_access(self):
        status, _, invitations = await self.call("GET", "/chats/invitations", token=self.token(self.b))
        self.assertEqual(status, 200)
        self.assertEqual([(i["chat_id"], i["inviter"]["id"], i["member_count"]) for i in invitations],
                         [(self.group_id, str(self.a.id), 1)])
        await self.assert_no_access(self.b)
        # New messages are not delivered to pending invitees.
        manager.send_to_users.reset_mock()
        await self.call("POST", f"/chats/{self.group_id}/messages", token=self.token(self.a), json_body={"content": "more"})
        recipients = {user_id for call in manager.send_to_users.call_args_list
                      if call.args[1]["type"] == "message.new" for user_id in call.args[0]}
        self.assertEqual(recipients, {self.a.id})

    async def test_only_the_invitee_can_accept(self):
        path = f"/chats/{self.group_id}/invitation/accept"
        for user in (self.a, self.d):  # the inviter/admin and an outsider have no invitation
            self.assertEqual((await self.call("POST", path, token=self.token(user)))[0], 404)
        self.assertEqual((await self.call("POST", f"/chats/{uuid.uuid4()}/invitation/accept", token=self.token(self.b)))[0], 404)
        await self.assert_no_access(self.b)
        status, _, chat = await self.call("POST", path, token=self.token(self.b))
        self.assertEqual(status, 200)
        self.assertEqual({member["id"] for member in chat["members"]}, {str(self.a.id), str(self.b.id)})
        self.assertEqual((await self.call("GET", f"/chats/{self.group_id}/messages", token=self.token(self.b)))[0], 200)
        self.assertEqual((await self.call("GET", self.image_url, token=self.token(self.b)))[0], 200)
        self.assertEqual((await self.call("POST", path, token=self.token(self.b)))[0], 404)  # used up
        await self.assert_no_access(self.c)  # still pending

    async def test_declined_invitations_cannot_be_accepted_later(self):
        self.assertEqual((await self.call("POST", f"/chats/{self.group_id}/invitation/decline", token=self.token(self.c)))[0], 204)
        self.assertEqual((await self.call("POST", f"/chats/{self.group_id}/invitation/accept", token=self.token(self.c)))[0], 404)
        self.assertEqual((await self.call("GET", "/chats/invitations", token=self.token(self.c)))[2], [])
        await self.assert_no_access(self.c)
        self.assertEqual((await self.call("POST", f"/chats/{self.group_id}/invitation/decline", token=self.token(self.d)))[0], 404)

    async def test_blocking_the_inviter_withdraws_the_invitation(self):
        self.assertEqual((await self.call("PUT", f"/users/me/blocked/{self.a.id}", token=self.token(self.c)))[0], 200)
        self.assertEqual((await self.call("GET", "/chats/invitations", token=self.token(self.c)))[2], [])
        self.assertEqual((await self.call("POST", f"/chats/{self.group_id}/invitation/accept", token=self.token(self.c)))[0], 404)
