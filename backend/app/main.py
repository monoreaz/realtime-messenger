import os
from contextlib import asynccontextmanager

from fastapi import FastAPI, HTTPException
from sqlalchemy import text

from app.api.calls import router as calls_router
from app.api.push import router as push_router
from app.api.admin import router as admin_router
from app.api.auth import router as auth_router
from app.core.body_limit import BodySizeLimitMiddleware, SecurityHeadersMiddleware
from app.database import engine

from app.api.users import router as users_router

from app.api.chats import router as chats_router
from app.api.messages import router as messages_router
from app.api.ws import router as ws_router

from fastapi.middleware.cors import CORSMiddleware

from pathlib import Path
from fastapi.staticfiles import StaticFiles

@asynccontextmanager
async def lifespan(app: FastAPI):
    yield
    await engine.dispose()


# Interactive API docs reveal the full API surface; only enable them for development.
API_DOCS = os.getenv("ENABLE_API_DOCS", "false").lower() == "true"

app = FastAPI(
    title="Messenger API",
    version="0.1.0",
    lifespan=lifespan,
    docs_url="/docs" if API_DOCS else None,
    redoc_url="/redoc" if API_DOCS else None,
    openapi_url="/openapi.json" if API_DOCS else None,
)

UPLOAD_DIR = Path("/app/uploads")

UPLOAD_DIR.mkdir(
    parents=True,
    exist_ok=True,
)

app.mount(
    "/uploads",
    StaticFiles(
        directory=UPLOAD_DIR,
    ),
    name="uploads",
)

# The web app is served from the same origin as the API, so no cross-origin access is
# needed in production. Development origins can be listed in CORS_ALLOWED_ORIGINS.
CORS_ALLOWED_ORIGINS = [
    origin.strip()
    for origin in os.getenv("CORS_ALLOWED_ORIGINS", "").split(",")
    if origin.strip()
]

app.add_middleware(
    CORSMiddleware,
    allow_origins=CORS_ALLOWED_ORIGINS,
    allow_credentials=True,
    allow_methods=[
        "GET",
        "POST",
        "PATCH",
        "DELETE",
        "OPTIONS",
    ],
    allow_headers=[
        "Authorization",
        "Content-Type",
    ],
)

app.add_middleware(SecurityHeadersMiddleware)
# Added last so it runs first: oversized bodies are refused before anything reads them.
app.add_middleware(BodySizeLimitMiddleware)

app.include_router(calls_router)
app.include_router(push_router)
app.include_router(admin_router)
app.include_router(auth_router)
app.include_router(users_router)
app.include_router(chats_router)
app.include_router(messages_router)
app.include_router(ws_router)

@app.get("/")
async def root():
    return {
        "name": "Messenger API",
        "version": "0.1.0",
    }


@app.get("/health")
async def health():
    try:
        async with engine.connect() as connection:
            await connection.execute(text("SELECT 1"))
    except Exception:
        raise HTTPException(
            status_code=503,
            detail="Database unavailable",
        )

    return {
        "status": "ok",
        "database": "ok",
    }