# Realtime Messenger

A small self-hosted messenger that I built as a homelab project.

The main goal of this project is to learn how real-time applications work and get more experience with backend development, WebSockets, authentication, PostgreSQL, Docker and server administration.

The application is currently running on an Ubuntu Server VM inside my Proxmox homelab.

## What works right now

* User registration
* Login
* JWT authentication
* Password hashing with Argon2
* User search
* Private chats
* Message history
* Real-time message delivery with WebSockets
* React frontend
* PostgreSQL database
* Alembic database migrations
* Docker Compose setup

## Stack

Frontend:

* React
* TypeScript
* Vite

Backend:

* Python
* FastAPI
* SQLAlchemy
* asyncpg
* WebSockets
* JWT
* Argon2

Infrastructure:

* Docker
* Docker Compose
* PostgreSQL
* Ubuntu Server
* Proxmox

## How it works

The frontend communicates with the FastAPI backend using normal HTTP requests for things like login, loading chats and sending messages.

WebSockets are used for real-time message delivery.

The whole application currently runs inside one Ubuntu VM:

Docker Compose is used to run the services.

## API

Authentication:


POST /auth/register
POST /auth/login


Users:


GET /users/me
GET /users/search


Chats:


GET /chats
POST /chats/private/{user_id}


Messages:

GET /chats/{chat_id}/messages
POST /chats/{chat_id}/messages


Realtime connection:


WS /ws


## Running it

Clone the repository:


git clone git@github.com:monoreaz/realtime-messenger.git
cd realtime-messenger


Create the environment file:


cp .env.example .env


Generate a PostgreSQL password:


openssl rand -hex 24


Generate a JWT secret:


openssl rand -hex 32


Put them into `.env`.

Example:


POSTGRES_USER=messenger
POSTGRES_PASSWORD=your_password
POSTGRES_DB=messenger

JWT_SECRET_KEY=your_jwt_secret
ACCESS_TOKEN_EXPIRE_MINUTES=30


Start everything:


docker compose up -d --build


Apply database migrations:


docker compose run --rm backend alembic upgrade head


Frontend:


http://SERVER_IP:5173


FastAPI docs:


http://SERVER_IP:8000/docs


## Current state

This is still an early version of the project.

At the moment the basic flow looks like this:


Register
   -
Login
   -
Search user
   -
Create private chat
   -
Send message
   -
Save message to PostgreSQL
   -
Send it to the other user through WebSocket


The WebSocket connection manager currently lives inside one FastAPI process, so the real-time part only works through one backend instance.

That is fine for now, but I want to change this later when I start running the messenger on multiple servers.

## What I want to add next

Some of the things I want to work on:

* Last message preview in the chat list
* Sort chats by recent activity
* Unread message counter
* Read receipts
* Typing indicator
* Online/offline status
* User avatars
* Message editing
* Message deletion
* Replies
* Images and files
* Group chats
* Better mobile layout
* Refresh tokens
* HTTPS and reverse proxy

Later I also want to experiment with running the application across multiple physical servers in my homelab.

Redis would be used to send real-time events between backend instances.

## Why I made this

I wanted something that combines several things I'm interested in:

* Python backend development
* Web development
* Networking
* Linux
* Docker
* Proxmox
* Databases
* WebSockets
* Authentication
* Distributed systems

I mainly started this project to learn and it is still in development, and I'm adding new features as I learn more. If you find any bugs, security vulnerabilities, or have suggestions for improvements, feel free to open an issue.
