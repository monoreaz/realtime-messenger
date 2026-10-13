"""Requires Python Playwright and a running Compose stack with TURN.
Creates two temporary accounts, checks audio through the real UI in both directions,
and deletes its accounts/chat afterwards. Uses fake microphones and forces relay.
Optional CHROMIUM_EXECUTABLE and MESSENGER_TEST_URL select browser and local URL.
"""
import asyncio, json, subprocess, os
from pathlib import Path
from playwright.async_api import async_playwright

ROOT=str(Path(__file__).resolve().parents[1])
def backend(script):
    return subprocess.check_output(['docker','compose','exec','-T','backend','python','-'], input=script.encode(), cwd=ROOT).decode()

SETUP='''import asyncio,json,uuid
from datetime import datetime, timezone
from app.database import AsyncSessionLocal
from app.models.user import User
from app.models.chat import Chat,ChatMember
from app.models.message import Message
from app.core.security import create_access_token
async def main():
    async with AsyncSessionLocal() as db:
        users=[User(username="calltest_"+uuid.uuid4().hex[:12],password_hash="unusable",email_verified_at=datetime.now(timezone.utc)) for _ in range(2)]
        db.add_all(users);await db.flush()
        chat=Chat(type="private",direct_key=":".join(sorted(str(u.id) for u in users)))
        db.add(chat);await db.flush()
        db.add_all([ChatMember(chat_id=chat.id,user_id=u.id) for u in users]);db.add(Message(chat_id=chat.id,sender_id=users[0].id,content="Temporary call test"));await db.commit()
        print(json.dumps({"chat":str(chat.id),"users":[{"id":str(u.id),"name":u.username,"token":create_access_token(str(u.id))} for u in users]}))
asyncio.run(main())
'''
async def main():
    fixture=json.loads(backend(SETUP))
    try:
        async with async_playwright() as p:
            browser=await p.chromium.launch(executable_path=os.getenv("CHROMIUM_EXECUTABLE"),headless=True,args=['--no-sandbox','--use-fake-device-for-media-stream','--use-fake-ui-for-media-stream'])
            pages=[]
            for i,user in enumerate(fixture['users']):
                context=await browser.new_context(viewport={'width':390,'height':844} if i else {'width':1280,'height':900})
                await context.add_init_script('''window.testPeers=[];const Original=window.RTCPeerConnection;window.RTCPeerConnection=class extends Original{constructor(config){super({...config,iceTransportPolicy:'relay'});window.testPeers.push(this);}};''')
                page=await context.new_page()
                async def refresh(route,request,token=user['token']):
                    await route.fulfill(json={'access_token':token,'token_type':'bearer'})
                await page.route('**/api/auth/refresh',refresh)
                await page.goto(os.getenv("MESSENGER_TEST_URL", "http://127.0.0.1:8080"))

                await page.locator('.chat-item').filter(has_text=fixture['users'][1-i]['name']).click()
                await page.get_by_role('button',name='Start audio call',exact=True).wait_for()
                pages.append(page)
            caller,receiver=pages
            for direction in range(2):
                await caller.get_by_role('button',name='Start audio call',exact=True).click()
                await receiver.get_by_role('button',name='Answer',exact=True).click(timeout=15000)
                for page in pages:
                    await page.wait_for_function("document.querySelector('.audio-call-status')?.textContent==='Connected'",timeout=25000)
                await caller.wait_for_timeout(1800)
                reports=[]
                for page in pages:
                    report=await page.evaluate('''async()=>{const pc=window.testPeers.at(-1); const stats=await pc.getStats();let bytes=0,relay=false;stats.forEach(s=>{if(s.type==='inbound-rtp'&&s.kind==='audio')bytes+=s.bytesReceived;if(s.type==='candidate-pair'&&s.state==='succeeded'&&s.nominated)relay=stats.get(s.localCandidateId)?.candidateType==='relay';});return{state:pc.connectionState,bytes,relay};}''')
                    reports.append(report)
                    assert report['bytes']>0 and report['relay'],report
                print('app call direction',direction,json.dumps(reports),flush=True)
                await caller.get_by_role('button',name='End call',exact=True).click()
                for page in pages: await page.locator('.audio-call-dialog').wait_for(state='detached')
                caller,receiver=receiver,caller
            await browser.close()
    finally:
        ids=[u['id'] for u in fixture['users']]
        cleanup='''import asyncio,uuid
from sqlalchemy import delete
from app.database import AsyncSessionLocal
from app.models.user import User
from app.models.chat import Chat
async def main():
    async with AsyncSessionLocal() as db:
        await db.execute(delete(Chat).where(Chat.id==uuid.UUID(CHAT)))
        await db.execute(delete(User).where(User.id.in_([uuid.UUID(i) for i in IDS])))
        await db.commit()
asyncio.run(main())
'''.replace('CHAT',repr(fixture['chat'])).replace('IDS',repr(ids))
        backend(cleanup)
        print('Temporary test accounts and chat removed',flush=True)

asyncio.run(main())
