export interface User {
    id: string;
    username: string;
    created_at: string;
}

export interface LoginResponse {
    access_token: string;
    token_type: string;
}

export interface Chat {
    id: string;
    type: string;
    peer: User;
    created_at: string;
}

export interface Message {
    id: string;
    chat_id: string;
    sender_id: string;
    content: string;
    created_at: string;
}

const isDevelopmentServer = window.location.port === "5173";

const API_BASE_URL = isDevelopmentServer
    ? `http://${window.location.hostname}:8000`
    : "/api";


async function getErrorMessage(response: Response, fallback: string): Promise<string> {
    try {
        const data = await response.json();

        if (typeof data.detail === "string") {
            return data.detail;
        }
    } catch {
        return fallback;
    }

    return fallback;
}


export async function registerUser(username: string, password: string): Promise<User> {
    const response = await fetch(`${API_BASE_URL}/auth/register`, {
        method: "POST",
        headers: {
            "Content-Type": "application/json",
        },
        body: JSON.stringify({
            username,
            password,
        }),
    });

    if (!response.ok) {
        throw new Error(
            await getErrorMessage(response, "Registration failed"),
        );
    }

    return response.json();
}


export async function loginUser(username: string, password: string): Promise<LoginResponse> {
    const body = new URLSearchParams();

    body.set("username", username);
    body.set("password", password);

    const response = await fetch(`${API_BASE_URL}/auth/login`, {
        method: "POST",
        headers: {
            "Content-Type": "application/x-www-form-urlencoded",
        },
        body,
    });

    if (!response.ok) {
        throw new Error(
            await getErrorMessage(response, "Login failed"),
        );
    }

    return response.json();
}


export async function getCurrentUser(token: string): Promise<User> {
    const response = await fetch(`${API_BASE_URL}/users/me`, {
        headers: {
            Authorization: `Bearer ${token}`,
        },
    });

    if (!response.ok) {
        throw new Error(
            await getErrorMessage(response, "Could not load current user"),
        );
    }

    return response.json();
}


export async function searchUsers(token: string, username: string): Promise<User[]> {
    const query = new URLSearchParams({
        username,
    });

    const response = await fetch(`${API_BASE_URL}/users/search?${query.toString()}`, {
        headers: {
            Authorization: `Bearer ${token}`,
        },
    });

    if (!response.ok) {
        throw new Error(
            await getErrorMessage(response, "Could not search users"),
        );
    }

    return response.json();
}


export async function getChats(token: string): Promise<Chat[]> {
    const response = await fetch(`${API_BASE_URL}/chats`, {
        headers: {
            Authorization: `Bearer ${token}`,
        },
    });

    if (!response.ok) {
        throw new Error(
            await getErrorMessage(response, "Could not load chats"),
        );
    }

    return response.json();
}


export async function createPrivateChat(token: string, userId: string): Promise<Chat> {
    const response = await fetch(`${API_BASE_URL}/chats/private/${userId}`, {
        method: "POST",
        headers: {
            Authorization: `Bearer ${token}`,
        },
    });

    if (!response.ok) {
        throw new Error(
            await getErrorMessage(response, "Could not create chat"),
        );
    }

    return response.json();
}


export async function getMessages(token: string, chatId: string): Promise<Message[]> {
    const response = await fetch(`${API_BASE_URL}/chats/${chatId}/messages`, {
        headers: {
            Authorization: `Bearer ${token}`,
        },
    });

    if (!response.ok) {
        throw new Error(
            await getErrorMessage(response, "Could not load messages"),
        );
    }

    return response.json();
}


export async function sendMessage(token: string, chatId: string, content: string): Promise<Message> {
    const response = await fetch(`${API_BASE_URL}/chats/${chatId}/messages`, {
        method: "POST",
        headers: {
            Authorization: `Bearer ${token}`,
            "Content-Type": "application/json",
        },
        body: JSON.stringify({
            content,
        }),
    });

    if (!response.ok) {
        throw new Error(
            await getErrorMessage(response, "Could not send message"),
        );
    }

    return response.json();
}


export function createWebSocket(): WebSocket {
    const protocol = window.location.protocol === "https:" ? "wss" : "ws";

    const host = isDevelopmentServer
        ? `${window.location.hostname}:8000`
        : window.location.host;

    return new WebSocket(
        `${protocol}://${host}/ws`,
    );
}