"""Grant administrator access to an existing account from the server console."""
import argparse
import asyncio
from sqlalchemy import select
from app.database import AsyncSessionLocal, engine
from app.models.user import User


async def grant(username):
    try:
        async with AsyncSessionLocal() as db:
            user = await db.scalar(select(User).where(User.username == username.lower()).with_for_update())
            if user is None:
                raise SystemExit("User not found")
            if user.is_blocked:
                raise SystemExit("Cannot grant admin access to a blocked account")
            user.is_admin = True
            await db.commit()
            print(f"Administrator access granted to @{user.username}")
    finally:
        await engine.dispose()


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("username")
    asyncio.run(grant(parser.parse_args().username))
