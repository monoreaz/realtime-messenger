"""Find (and optionally delete) uploaded files that no message or profile references.

Before the 2026-11 security fixes, deleting a message left its file on disk, still
downloadable by URL. Those files, and any other unreferenced uploads, are orphans.

    python -m app.cli.cleanup_uploads            # dry run: counts and sizes only
    python -m app.cli.cleanup_uploads --delete   # remove the orphans

Files younger than the grace period are skipped: an upload is written to disk just
before its message is committed.
"""
import argparse
import asyncio
import time
from pathlib import Path

from sqlalchemy import select

from app.database import AsyncSessionLocal, engine
from app.models.message import Message
from app.models.user import User

UPLOAD_ROOT = Path("/app/uploads")
FOLDERS = ("messages", "avatars")
GRACE_SECONDS = 10 * 60


async def referenced_urls() -> set[str]:
    async with AsyncSessionLocal() as db:
        urls = set()
        for column in (Message.image_url, Message.video_url, User.avatar_url):
            urls.update(url for url in (await db.scalars(select(column).where(column.is_not(None)))).all())
        return urls


def find_orphans(referenced: set[str], root: Path = UPLOAD_ROOT, now: float | None = None) -> list[Path]:
    now = time.time() if now is None else now
    orphans = []
    for folder in FOLDERS:
        directory = root / folder
        if not directory.is_dir():
            continue
        for path in directory.iterdir():
            if not path.is_file() or now - path.stat().st_mtime < GRACE_SECONDS:
                continue
            if f"/uploads/{folder}/{path.name}" not in referenced:
                orphans.append(path)
    return orphans


async def main(delete: bool) -> None:
    try:
        orphans = find_orphans(await referenced_urls())
    finally:
        await engine.dispose()
    total = sum(path.stat().st_size for path in orphans)
    for folder in FOLDERS:
        count = sum(1 for path in orphans if path.parent.name == folder)
        print(f"{folder}: {count} unreferenced file(s)")
    print(f"total: {len(orphans)} file(s), {total / 1024 / 1024:.1f} MiB")
    if delete:
        for path in orphans:
            path.unlink(missing_ok=True)
        print("deleted")
    else:
        print("dry run; pass --delete to remove them")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--delete", action="store_true", help="delete the unreferenced files")
    asyncio.run(main(parser.parse_args().delete))
