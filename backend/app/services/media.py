"""Checks on uploaded media and cleanup of stored files."""
from pathlib import Path

UPLOAD_ROOT = Path("/app/uploads")


def content_matches(content_type: str, head: bytes) -> bool:
    """Whether the file's leading bytes match the declared (allow-listed) type.

    The client-declared Content-Type alone decides nothing: a file is stored only
    if its signature matches, so e.g. HTML cannot be stored under an image name.
    """
    if content_type == "image/jpeg":
        return head.startswith(b"\xff\xd8\xff")
    if content_type == "image/png":
        return head.startswith(b"\x89PNG\r\n\x1a\n")
    if content_type == "image/webp":
        return head[:4] == b"RIFF" and head[8:12] == b"WEBP"
    if content_type == "video/mp4":
        return head[4:8] == b"ftyp"
    if content_type == "video/webm":
        return head.startswith(b"\x1a\x45\xdf\xa3")
    return False


def delete_upload(url: str | None) -> None:
    """Remove a file previously stored under /uploads/<dir>/<name>; ignores anything else."""
    if not url or not url.startswith("/uploads/"):
        return
    relative = Path(url.removeprefix("/uploads/"))
    if relative.is_absolute() or ".." in relative.parts or len(relative.parts) != 2:
        return
    try:
        (UPLOAD_ROOT / relative).unlink(missing_ok=True)
    except OSError:
        pass
