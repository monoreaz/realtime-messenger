# Base image pinned by digest (python:3.12-slim as of 2026-11-21), so rebuilds do not
# silently pick up a different Python or Debian packages.
FROM python:3.12-slim@sha256:5e2bdb1b0d542d5039cffb8181c65930093949194a65c365dfb1b0fe13a4fa5b

WORKDIR /app

# requirements.lock: exact versions with hashes (generated from requirements.txt).
COPY requirements.lock .

RUN pip install --no-cache-dir --require-hashes --only-binary=:all: -r requirements.lock

COPY app ./app

CMD ["uvicorn", "app.main:app", "--host", "0.0.0.0", "--port", "8000"]
