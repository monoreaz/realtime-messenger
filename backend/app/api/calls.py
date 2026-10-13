import base64
import hashlib
import hmac
import os
import time
from pathlib import Path

from fastapi import APIRouter, Depends, HTTPException, Response
from app.core.dependencies import get_current_user
from app.models.user import User

router = APIRouter(tags=["calls"])


def ice_configuration(user_id):
    servers = [{"urls": "stun:stun.l.google.com:19302"}]
    urls = [url.strip() for url in os.getenv("TURN_URLS", "").split(",") if url.strip()]
    if not urls:
        return {"iceServers": servers}
    try:
        secret = Path(os.environ["TURN_SECRET_FILE"]).read_text().strip()
        if not secret:
            raise ValueError("Empty TURN secret")
    except (KeyError, OSError, ValueError):
        raise HTTPException(status_code=503, detail="Call relay is not configured")
    username = f"{int(time.time()) + 3600}:{user_id}"
    credential = base64.b64encode(hmac.new(secret.encode(), username.encode(), hashlib.sha1).digest()).decode()
    servers.append({"urls": urls, "username": username, "credential": credential})
    return {"iceServers": servers}


@router.get("/calls/ice")
async def call_ice(response: Response, user: User = Depends(get_current_user)):
    response.headers["Cache-Control"] = "no-store"
    return ice_configuration(user.id)
