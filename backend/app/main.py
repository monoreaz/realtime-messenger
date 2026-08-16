from contextlib import asynccontextmanager

from fastapi import FastAPI, HTTPException
from sqlalchemy import text

from app.api.auth import router as auth_router
from app.database import engine

from app.api.users import router as users_router

from app.api.chats import router as chats_router
from app.api.messages import router as messages_router
from app.api.ws import router as ws_router

from fastapi.middleware.cors import CORSMiddleware
@asynccontextmanager
async def lifespan(app: FastAPI):
    yield
    await engine.dispose()


app = FastAPI(
    title="Messenger API",
    version="0.1.0",
    lifespan=lifespan,
)

app.add_middleware(
    CORSMiddleware,
    allow_origins=[
        "http://192.168.0.111:5173",
        "http://localhost:5173",
        "http://127.0.0.1:5173",
        "http://100.91.201.40:5173",
    ],
    allow_credentials=True,
    allow_methods=[
        "GET",
        "POST",
        "OPTIONS",
    ],
    allow_headers=[
        "Authorization",
        "Content-Type",
    ],
)

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