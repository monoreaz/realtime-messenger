import { Fragment, useEffect, useRef, useState } from "react";
import type { ChangeEvent, FormEvent } from "react";

import {
    createPrivateChat,
    createWebSocket,
    getChats,
    getCurrentUser,
    getMediaUrl,
    getMessages,
    loginUser,
    markChatRead,
    registerUser,
    removeAvatar,
    searchUsers,
    sendMessage,
    updateProfile,
    uploadAvatar,
    verifyEmail,
    logoutSession,
    refreshSession,
    type Chat,
    type Message,
    type User,
} from "./api";

import "./App.css";


type AuthMode = "login" | "register";
type WebSocketStatus = "connecting" | "connected" | "disconnected";


function sortChats(chats: Chat[]): Chat[] {
    return [...chats].sort((firstChat, secondChat) => {
        const firstDate =
            firstChat.last_message?.created_at ??
            firstChat.created_at;

        const secondDate =
            secondChat.last_message?.created_at ??
            secondChat.created_at;

        return (
            new Date(secondDate).getTime() -
            new Date(firstDate).getTime()
        );
    });
}


function getUserDisplayName(user: User): string {
    return user.display_name?.trim() || user.username;
}


function UserAvatar({
    user,
    className,
}: {
    user: User;
    className: string;
}) {
    const avatarUrl = getMediaUrl(user.avatar_url);

    return (
        <div className={className}>
            {avatarUrl ? (
                <img src={avatarUrl} alt="" />
            ) : (
                user.username.charAt(0).toUpperCase()
            )}
        </div>
    );
}


function App() {
    const [token, setToken] = useState<string | null>(null);

    const [sessionChecked, setSessionChecked] = useState(false);

    const [user, setUser] = useState<User | null>(null);
    const [chats, setChats] = useState<Chat[]>([]);
    const [activeChat, setActiveChat] = useState<Chat | null>(null);
    const [messages, setMessages] = useState<Message[]>([]);

    const [mode, setMode] = useState<AuthMode>("login");
    const [username, setUsername] = useState("");
    const [password, setPassword] = useState("");

    const [rememberMe, setRememberMe] = useState(false);

    const [email, setEmail] = useState("");
    const [authNotice, setAuthNotice] = useState("");

    const [searchQuery, setSearchQuery] = useState("");
    const [searchResults, setSearchResults] = useState<User[]>([]);

    const [messageInput, setMessageInput] = useState("");

    const [replyingTo, setReplyingTo] = useState<Message | null>(null);

    const messageInputRef = useRef<HTMLInputElement | null>(null);

    const [authLoading, setAuthLoading] = useState(false);
    const [appLoading, setAppLoading] = useState(true);
    const [messagesLoading, setMessagesLoading] = useState(false);
    const [searchLoading, setSearchLoading] = useState(false);
    const [sending, setSending] = useState(false);

    const [error, setError] = useState("");
    const [wsStatus, setWsStatus] = useState<WebSocketStatus>("disconnected");

    const [onlineUserIds, setOnlineUserIds] = useState<Set<string>>(
        () => new Set(),
    );

    const [typingUserIds, setTypingUserIds] = useState<Set<string>>(
        () => new Set(),
    );

    const [profileOpen, setProfileOpen] = useState(false);
    const [profileUsername, setProfileUsername] = useState("");
    const [profileDisplayName, setProfileDisplayName] = useState("");
    const [profileBio, setProfileBio] = useState("");
    const [profileSaving, setProfileSaving] = useState(false);
    const [avatarUploading, setAvatarUploading] = useState(false);
    const [profileMessage, setProfileMessage] = useState("");

    const activeChatIdRef = useRef<string | null>(null);
    const messagesEndRef = useRef<HTMLDivElement | null>(null);
    const websocketRef = useRef<WebSocket | null>(null);

    const typingTimeoutRef = useRef< ReturnType<typeof setTimeout> | null >(null);

    const typingChatIdRef = useRef<string | null>(null);


    function sendWebSocketEvent(
        data: Record<string, unknown>,
    ) {
        const websocket = websocketRef.current;

        if (
            !websocket ||
            websocket.readyState !== WebSocket.OPEN
        ) {
            return;
        }

        websocket.send(JSON.stringify(data));
    }

useEffect(() => {
    async function restoreSession() {
        localStorage.removeItem(
            "access_token",
        );

        try {
            const session =
                await refreshSession();

            setToken(
                session.access_token,
            );
        } catch {
            setToken(null);
        } finally {
            setSessionChecked(true);
        }
    }

    void restoreSession();
}, []);

useEffect(() => {
    const params = new URLSearchParams(
        window.location.search
    );

    const verificationToken =
        params.get("verify");

    if (!verificationToken) {
        return;
    }

    async function verify() {
        setError("");
        setAuthNotice("");

        try {
            await verifyEmail(
                verificationToken!
            );

            setMode("login");

            setAuthNotice(
                "Email verified. You can now sign in."
            );
        } catch (caughtError) {
            if (
                caughtError instanceof Error
            ) {
                setError(
                    caughtError.message
                );
            }
        } finally {
            params.delete("verify");

            const query =
                params.toString();

            window.history.replaceState(
                {},
                "",
                query
                    ? `${window.location.pathname}?${query}`
                    : window.location.pathname,
            );
        }
    }

    void verify();
}, []);

useEffect(() => {
    if (!token || !user) {
        return;
    }

    const interval = window.setInterval(
        async () => {
            try {
                const session =
                    await refreshSession();

                setToken(
                    session.access_token,
                );
            } catch {
                setToken(null);
                setUser(null);
            }
        },
        10 * 60 * 1000,
    );

    return () => {
        window.clearInterval(
            interval,
        );
    };
}, [Boolean(token), user?.id]);


    function stopTyping() {
        if (typingTimeoutRef.current) {
            clearTimeout(typingTimeoutRef.current);
            typingTimeoutRef.current = null;
        }

        if (typingChatIdRef.current) {
            sendWebSocketEvent({
                type: "typing.stop",
                chat_id: typingChatIdRef.current,
            });

            typingChatIdRef.current = null;
        }
    }


    function handleMessageInputChange(
        event: ChangeEvent<HTMLInputElement>,
    ) {
        const value = event.target.value;

        setMessageInput(value);

        if (!activeChat) {
            return;
        }

        if (!value.trim()) {
            stopTyping();
            return;
        }

        if (typingChatIdRef.current !== activeChat.id) {
            stopTyping();

            typingChatIdRef.current = activeChat.id;

            sendWebSocketEvent({
                type: "typing.start",
                chat_id: activeChat.id,
            });
        }

        if (typingTimeoutRef.current) {
            clearTimeout(typingTimeoutRef.current);
        }

        typingTimeoutRef.current = setTimeout(() => {
            stopTyping();
        }, 1800);
    }


    useEffect(() => {
        activeChatIdRef.current = activeChat?.id ?? null;
    }, [activeChat]);


    useEffect(() => {
        messagesEndRef.current?.scrollIntoView({
            behavior: "smooth",
        });
    }, [messages]);


    useEffect(() => {
    if (!sessionChecked) {
        return;
    }

    if (!token) {
        setUser(null);
        setChats([]);
        setActiveChat(null);
        setMessages([]);
        setAppLoading(false);

        return;
    }

    if (user) {
        setAppLoading(false);
        return;
    }

    async function loadSession() {
        setAppLoading(true);

        try {
            const [
                currentUser,
                userChats,
            ] = await Promise.all([
                getCurrentUser(token!),
                getChats(token!),
            ]);

            setUser(currentUser);

            const sortedChats =
                sortChats(userChats);

            setChats(sortedChats);

            if (
                sortedChats.length > 0
            ) {
                await openChat(
                    sortedChats[0],
                    token!,
                );
            }
        } catch {
            setToken(null);
            setUser(null);
        } finally {
            setAppLoading(false);
        }
    }

    void loadSession();
}, [
    token,
    sessionChecked,
    user,
]);


    useEffect(() => {
        if (!token || !user) {
            return;
        }

        const currentUserId = user.id;
        const websocket = createWebSocket();

        websocketRef.current = websocket;
        setWsStatus("connecting");

        websocket.addEventListener("open", () => {
            websocket.send(
                JSON.stringify({
                    type: "auth",
                    token,
                }),
            );
        });

        websocket.addEventListener("message", (event) => {
            const data = JSON.parse(event.data);

            if (data.type === "connection.ready") {
                setWsStatus("connected");
                return;
            }

            if (data.type === "presence.snapshot") {
                setOnlineUserIds(
                    new Set(data.online_user_ids as string[]),
                );
                return;
            }

            if (data.type === "presence.online") {
                setOnlineUserIds((current) => {
                    const updated = new Set(current);
                    updated.add(data.user_id);
                    return updated;
                });
                return;
            }

            if (data.type === "presence.offline") {
                setOnlineUserIds((current) => {
                    const updated = new Set(current);
                    updated.delete(data.user_id);
                    return updated;
                });

                setTypingUserIds((current) => {
                    const updated = new Set(current);
                    updated.delete(data.user_id);
                    return updated;
                });

                return;
            }

            if (data.type === "presence.state") {
                setOnlineUserIds((current) => {
                    const updated = new Set(current);

                    if (data.online) {
                        updated.add(data.user_id);
                    } else {
                        updated.delete(data.user_id);
                    }

                    return updated;
                });

                return;
            }

            if (data.type === "typing.start") {
                if (data.chat_id === activeChatIdRef.current) {
                    setTypingUserIds((current) => {
                        const updated = new Set(current);
                        updated.add(data.user_id);
                        return updated;
                    });
                }

                return;
            }

            if (data.type === "typing.stop") {
                setTypingUserIds((current) => {
                    const updated = new Set(current);
                    updated.delete(data.user_id);
                    return updated;
                });

                return;
            }

            if (data.type === "chat.read") {
                setChats((currentChats) =>
                    currentChats.map((chat) =>
                        chat.id === data.chat_id
                            ? {
                                  ...chat,
                                  peer_last_read_at: data.read_at,
                              }
                            : chat,
                    ),
                );

                setActiveChat((currentChat) => {
                    if (
                        !currentChat ||
                        currentChat.id !== data.chat_id
                    ) {
                        return currentChat;
                    }

                    return {
                        ...currentChat,
                        peer_last_read_at: data.read_at,
                    };
                });

                return;
            }

            if (data.type === "profile.updated") {
                const updatedUser = data.user as User;

                setChats((currentChats) =>
                    currentChats.map((chat) =>
                        chat.peer.id === updatedUser.id
                            ? {
                                  ...chat,
                                  peer: updatedUser,
                              }
                            : chat,
                    ),
                );

                setActiveChat((currentChat) => {
                    if (
                        !currentChat ||
                        currentChat.peer.id !== updatedUser.id
                    ) {
                        return currentChat;
                    }

                    return {
                        ...currentChat,
                        peer: updatedUser,
                    };
                });

                setSearchResults((currentUsers) =>
                    currentUsers.map((searchUser) =>
                        searchUser.id === updatedUser.id
                            ? updatedUser
                            : searchUser,
                    ),
                );

                return;
            }

            if (data.type === "message.new") {
                const incomingMessage = data.message as Message;

                setTypingUserIds((current) => {
                    const updated = new Set(current);
                    updated.delete(incomingMessage.sender_id);
                    return updated;
                });

                const isActiveChat =
                    incomingMessage.chat_id ===
                    activeChatIdRef.current;

                setChats((currentChats) => {
                    const updatedChats = currentChats.map((chat) => {
                        if (chat.id !== incomingMessage.chat_id) {
                            return chat;
                        }

                        const shouldIncreaseUnread =
                            incomingMessage.sender_id !== currentUserId &&
                            !isActiveChat;

                        return {
                            ...chat,
                            last_message: {
                                id: incomingMessage.id,
                                sender_id: incomingMessage.sender_id,
                                content: incomingMessage.content,
                                created_at: incomingMessage.created_at,
                            },
                            unread_count: shouldIncreaseUnread
                                ? chat.unread_count + 1
                                : chat.unread_count,
                        };
                    });

                    return sortChats(updatedChats);
                });

                if (!isActiveChat) {
                    return;
                }

                setMessages((currentMessages) => {
                    const alreadyExists = currentMessages.some(
                        (message) =>
                            message.id === incomingMessage.id,
                    );

                    if (alreadyExists) {
                        return currentMessages;
                    }

                    return [
                        ...currentMessages,
                        incomingMessage,
                    ];
                });

                if (incomingMessage.sender_id !== currentUserId) {
                    void markChatRead(
                        token,
                        incomingMessage.chat_id,
                    );
                }
            }
        });

        websocket.addEventListener("close", () => {
            setWsStatus("disconnected");
        });

        websocket.addEventListener("error", () => {
            setWsStatus("disconnected");
        });

        return () => {
            stopTyping();
            websocket.close();

            if (websocketRef.current === websocket) {
                websocketRef.current = null;
            }
        };
    }, [token, user?.id]);


    async function handleAuthSubmit(
        event: FormEvent<HTMLFormElement>,
    ) {
        event.preventDefault();

        setError("");
        setAuthLoading(true);

        try {
            if (mode === "register") {
                await registerUser(
                    username,
                    email,
                    password,
                );

                setPassword("");
                setMode("login");

                setAuthNotice(
                    `Account created. Verify ${email} before signing in.`
                );

                return;
            }

            const loginResponse = await loginUser(
                username,
                password,
                rememberMe,
            );

            // localStorage.setItem(
            //     "access_token",
            //     loginResponse.access_token,
            // );

            setUser(null);

                setToken(
                    loginResponse.access_token,
                );


            setToken(loginResponse.access_token);
            setPassword("");
        } catch (caughtError) {
            if (caughtError instanceof Error) {
                setError(caughtError.message);
            } else {
                setError("Something went wrong");
            }
        } finally {
            setAuthLoading(false);
        }
    }


    async function openChat(
        chat: Chat,
        currentToken = token,
    ) {
        if (!currentToken) {
            return;
        }

        stopTyping();
        setReplyingTo(null);
        setActiveChat(chat);
        setTypingUserIds(new Set());

        sendWebSocketEvent({
            type: "presence.get",
            user_id: chat.peer.id,
        });

        setMessagesLoading(true);
        setError("");

        try {
            const chatMessages = await getMessages(
                currentToken,
                chat.id,
            );

            setMessages(chatMessages);

            await markChatRead(
                currentToken,
                chat.id,
            );

            setChats((currentChats) =>
                currentChats.map((currentChat) =>
                    currentChat.id === chat.id
                        ? {
                              ...currentChat,
                              unread_count: 0,
                          }
                        : currentChat,
                ),
            );
        } catch (caughtError) {
            if (caughtError instanceof Error) {
                setError(caughtError.message);
            }
        } finally {
            setMessagesLoading(false);
        }
    }


    async function handleSearch(
        event: FormEvent<HTMLFormElement>,
    ) {
        event.preventDefault();

        if (!token || !searchQuery.trim()) {
            return;
        }

        setSearchLoading(true);
        setError("");

        try {
            const users = await searchUsers(
                token,
                searchQuery.trim(),
            );

            setSearchResults(users);
        } catch (caughtError) {
            if (caughtError instanceof Error) {
                setError(caughtError.message);
            }
        } finally {
            setSearchLoading(false);
        }
    }


    async function handleUserSelect(targetUser: User) {
        if (!token) {
            return;
        }

        setError("");

        try {
            const chat = await createPrivateChat(
                token,
                targetUser.id,
            );

            setChats((currentChats) => {
                const alreadyExists = currentChats.some(
                    (existingChat) =>
                        existingChat.id === chat.id,
                );

                if (alreadyExists) {
                    return currentChats.map((existingChat) =>
                        existingChat.id === chat.id
                            ? chat
                            : existingChat,
                    );
                }

                return sortChats([
                    chat,
                    ...currentChats,
                ]);
            });

            setSearchQuery("");
            setSearchResults([]);

            await openChat(chat, token);
        } catch (caughtError) {
            if (caughtError instanceof Error) {
                setError(caughtError.message);
            }
        }
    }

    function startReply(
        message: Message,
    ) {
        setReplyingTo(message);

        requestAnimationFrame(() => {
            messageInputRef.current?.focus();
        });
    }


    async function handleSendMessage(
        event: FormEvent<HTMLFormElement>,
    ) {
        event.preventDefault();

        if (!token || !activeChat) {
            return;
        }

        const content = messageInput.trim();

        if (!content) {
            return;
        }

        stopTyping();
        setSending(true);
        setError("");

        try {
            const newMessage = await sendMessage(
                token,
                activeChat.id,
                content,
                replyingTo?.id ?? null,
            );

            setMessages((currentMessages) => {
                const alreadyExists = currentMessages.some(
                    (message) => message.id === newMessage.id,
                );

                if (alreadyExists) {
                    return currentMessages;
                }

                return [
                    ...currentMessages,
                    newMessage,
                ];
            });

            setMessageInput("");
            setReplyingTo(null);
        } catch (caughtError) {
            if (caughtError instanceof Error) {
                setError(caughtError.message);
            }
        } finally {
            setSending(false);
        }
    }


    function openProfileSettings() {
        if (!user) {
            return;
        }

        setProfileUsername(user.username);
        setProfileDisplayName(user.display_name ?? "");
        setProfileBio(user.bio ?? "");
        setProfileMessage("");
        setError("");
        setProfileOpen(true);
    }


    async function handleProfileSave(
        event: FormEvent<HTMLFormElement>,
    ) {
        event.preventDefault();

        if (!token) {
            return;
        }

        setProfileSaving(true);
        setProfileMessage("");
        setError("");

        try {
            const updatedUser = await updateProfile(
                token,
                {
                    username: profileUsername.trim(),
                    display_name:
                        profileDisplayName.trim() || null,
                    bio: profileBio.trim() || null,
                },
            );

            setUser(updatedUser);
            setProfileUsername(updatedUser.username);
            setProfileDisplayName(
                updatedUser.display_name ?? "",
            );
            setProfileBio(updatedUser.bio ?? "");
            setProfileMessage("Profile saved");
        } catch (caughtError) {
            if (caughtError instanceof Error) {
                setError(caughtError.message);
            } else {
                setError("Could not update profile");
            }
        } finally {
            setProfileSaving(false);
        }
    }


    async function handleAvatarChange(
        event: ChangeEvent<HTMLInputElement>,
    ) {
        const file = event.target.files?.[0];

        if (!token || !file) {
            return;
        }

        setAvatarUploading(true);
        setProfileMessage("");
        setError("");

        try {
            const updatedUser = await uploadAvatar(
                token,
                file,
            );

            setUser(updatedUser);
            setProfileMessage("Avatar updated");
        } catch (caughtError) {
            if (caughtError instanceof Error) {
                setError(caughtError.message);
            } else {
                setError("Could not upload avatar");
            }
        } finally {
            setAvatarUploading(false);
            event.target.value = "";
        }
    }


    async function handleRemoveAvatar() {
        if (!token) {
            return;
        }

        setAvatarUploading(true);
        setProfileMessage("");
        setError("");

        try {
            const updatedUser = await removeAvatar(token);

            setUser(updatedUser);
            setProfileMessage("Avatar removed");
        } catch (caughtError) {
            if (caughtError instanceof Error) {
                setError(caughtError.message);
            } else {
                setError("Could not remove avatar");
            }
        } finally {
            setAvatarUploading(false);
        }
    }


    function closeProfileSettings() {
        setProfileOpen(false);
        setProfileMessage("");
        setError("");
    }


async function logout() {
    stopTyping();

    try {
        await logoutSession();
    } catch {
        // Clear local state even if
        // the backend is unavailable.
    }

    setReplyingTo(null);
    setToken(null);
    setUser(null);
    setChats([]);
    setActiveChat(null);
    setMessages([]);
    setSearchResults([]);
    setSearchQuery("");
    setProfileOpen(false);
    setWsStatus("disconnected");
    setOnlineUserIds(new Set());
    setTypingUserIds(new Set());
}


    function closeChat() {
        stopTyping();
        setReplyingTo(null);
        setActiveChat(null);
        setMessages([]);
        setTypingUserIds(new Set());
    }


    function formatTime(date: string) {
        return new Date(date).toLocaleTimeString([], {
            hour: "2-digit",
            minute: "2-digit",
        });
    }


    function isSameDay(
        firstDate: string | Date,
        secondDate: string | Date,
    ) {
        const first = new Date(firstDate);
        const second = new Date(secondDate);

        return (
            first.getFullYear() === second.getFullYear() &&
            first.getMonth() === second.getMonth() &&
            first.getDate() === second.getDate()
        );
    }


    function formatMessageDate(date: string) {
        const messageDate = new Date(date);
        const today = new Date();
        const yesterday = new Date(today);

        yesterday.setDate(today.getDate() - 1);

        if (isSameDay(messageDate, today)) {
            return "Today";
        }

        if (isSameDay(messageDate, yesterday)) {
            return "Yesterday";
        }

        return messageDate.toLocaleDateString([], {
            day: "numeric",
            month: "long",
            year: "numeric",
        });
    }


    if (!token) {
        return (
            <main className="auth-page">
                <section className="auth-card">
                    <div className="logo">M</div>

                    <h1>Messenger</h1>

                    <p className="subtitle">
                        {mode === "login"
                            ? "Sign in to continue"
                            : "Create your account"}
                    </p>

                    <form onSubmit={handleAuthSubmit}>
                        <label htmlFor="username">
                            Username
                        </label>
                            {mode === "register" && (
                            <>
                                <label htmlFor="email">
                                    Email
                                </label>

                                <input
                                    id="email"
                                    type="email"
                                    value={email}
                                    onChange={(event) =>
                                        setEmail(
                                            event.target.value
                                        )
                                    }
                                    autoComplete="email"
                                    required
                                />
                            </>
                        )}
                        <input id="username" type="text" value={username} onChange={(event) => setUsername(event.target.value)} minLength={3} maxLength={32} autoComplete="username" required />

                        <label htmlFor="password">
                            Password
                        </label>

                        <input id="password" type="password" value={password} onChange={(event) => setPassword(event.target.value)} minLength={8} autoComplete={mode === "login" ? "current-password" : "new-password"} required />
                            {mode === "login" && (
                            <label className="remember-me">
                                <input
                                    type="checkbox"
                                    checked={rememberMe}
                                    onChange={(event) =>
                                        setRememberMe(
                                            event.target.checked
                                        )
                                    }
                                />

                                <span>
                                    Remember me
                                </span>
                            </label>
                        )}
                        {authNotice && (
                            <div className="profile-success">
                                {authNotice}
                            </div>
                        )}

                        {error && (
                            <div className="error-message">
                                {error}
                            </div>
                        )}

                        <button className="primary-button" type="submit" disabled={authLoading}>
                            {authLoading
                                ? "Please wait..."
                                : mode === "login"
                                  ? "Sign in"
                                  : "Create account"}
                        </button>
                    </form>

                    <button className="mode-button" type="button" onClick={() => {
                        setError("");
                        setMode(
                            mode === "login"
                                ? "register"
                                : "login",
                        );
                    }}>
                        {mode === "login"
                            ? "Create an account"
                            : "Already have an account?"}
                    </button>
                </section>
            </main>
        );
    }


    if (appLoading || !user) {
        return (
            <main className="loading-page">
                Loading Messenger...
            </main>
        );
    }


    return (
        <main className={`messenger ${activeChat ? "chat-open" : ""}`}>
            <aside className="sidebar">
                <header className="sidebar-header">
                    <div className="current-user">
                        <UserAvatar
                            user={user}
                            className="small-avatar"
                        />

                        <div>
                            <strong>
                                {getUserDisplayName(user)}
                            </strong>

                            <div className={`connection-status ${wsStatus}`}>
                                {wsStatus === "connected"
                                    ? "Online"
                                    : wsStatus}
                            </div>
                        </div>
                    </div>

                    <div className="sidebar-actions">
                        <button className="settings-button" type="button" onClick={openProfileSettings} aria-label="Profile settings">
                            ⚙
                        </button>

                        <button className="logout-button" type="button" onClick={logout}>
                            Log out
                        </button>
                    </div>
                </header>

                <div className="search-area">
                    <form className="search-form" onSubmit={handleSearch}>
                        <input type="text" placeholder="Search users..." value={searchQuery} onChange={(event) => {
                            setSearchQuery(event.target.value);

                            if (!event.target.value) {
                                setSearchResults([]);
                            }
                        }} />

                        <button type="submit" disabled={searchLoading || !searchQuery.trim()}>
                            Search
                        </button>
                    </form>

                    {searchResults.length > 0 && (
                        <div className="search-results">
                            {searchResults.map((searchUser) => (
                                <button className="search-result" type="button" key={searchUser.id} onClick={() => void handleUserSelect(searchUser)}>
                                    <UserAvatar
                                        user={searchUser}
                                        className="chat-avatar"
                                    />

                                    <div className="search-result-user">
                                        <strong>
                                            {getUserDisplayName(
                                                searchUser,
                                            )}
                                        </strong>

                                        <span>
                                            @{searchUser.username}
                                        </span>
                                    </div>
                                </button>
                            ))}
                        </div>
                    )}
                </div>

                <div className="chat-list">
                    {chats.length === 0 ? (
                        <div className="empty-sidebar">
                            Search for someone to start a conversation.
                        </div>
                    ) : (
                        chats.map((chat) => (
                            <button className={`chat-item ${activeChat?.id === chat.id ? "active" : ""}`} type="button" key={chat.id} onClick={() => void openChat(chat)}>
                                <UserAvatar
                                    user={chat.peer}
                                    className="chat-avatar"
                                />

                                <div className="chat-info">
                                    <div className="chat-title-row">
                                        <strong>
                                            {getUserDisplayName(
                                                chat.peer,
                                            )}
                                        </strong>

                                        {chat.last_message && (
                                            <span className="chat-time">
                                                {formatTime(
                                                    chat.last_message.created_at,
                                                )}
                                            </span>
                                        )}
                                    </div>

                                    <div className="chat-preview-row">
                                        <span className="chat-preview">
                                            {chat.last_message
                                                ? `${
                                                      chat.last_message.sender_id === user.id
                                                          ? "You: "
                                                          : ""
                                                  }${chat.last_message.content}`
                                                : "No messages yet"}
                                        </span>

                                        {chat.unread_count > 0 && (
                                            <span className="unread-badge">
                                                {chat.unread_count > 99
                                                    ? "99+"
                                                    : chat.unread_count}
                                            </span>
                                        )}
                                    </div>
                                </div>
                            </button>
                        ))
                    )}
                </div>
            </aside>

            <section className="chat-panel">
                {activeChat ? (
                    <>
                        <header className="chat-header">
                            <button className="mobile-back-button" type="button" onClick={closeChat} aria-label="Back to chats">
                                ←
                            </button>

                            <UserAvatar
                                user={activeChat.peer}
                                className="chat-avatar"
                            />

                            <div>
                                <strong>
                                    {getUserDisplayName(
                                        activeChat.peer,
                                    )}
                                </strong>

                                <span
                                    className={
                                        typingUserIds.has(
                                            activeChat.peer.id,
                                        )
                                            ? "peer-status typing"
                                            : onlineUserIds.has(
                                                  activeChat.peer.id,
                                              )
                                              ? "peer-status online"
                                              : "peer-status"
                                    }
                                >
                                    {typingUserIds.has(
                                        activeChat.peer.id,
                                    )
                                        ? "typing..."
                                        : onlineUserIds.has(
                                              activeChat.peer.id,
                                          )
                                          ? "Online"
                                          : "Offline"}
                                </span>
                            </div>
                        </header>

                        <div className="messages">
                            {messagesLoading ? (
                                <div className="chat-placeholder">
                                    Loading messages...
                                </div>
                            ) : messages.length === 0 ? (
                                <div className="chat-placeholder">
                                    No messages yet. Say hello.
                                </div>
                            ) : (
                                messages.map((message, index) => {
                                    const isOwnMessage =
                                        message.sender_id === user.id;

                                    const isRead =
                                        isOwnMessage &&
                                        activeChat.peer_last_read_at !== null &&
                                        new Date(
                                            message.created_at,
                                        ).getTime() <=
                                            new Date(
                                                activeChat.peer_last_read_at,
                                            ).getTime();

                                    const previousMessage =
                                        index > 0
                                            ? messages[index - 1]
                                            : null;

                                    const showDate =
                                        !previousMessage ||
                                        !isSameDay(
                                            previousMessage.created_at,
                                            message.created_at,
                                        );

                                    return (
                                        <Fragment key={message.id}>
                                            {showDate && (
                                                <div className="message-date">
                                                    <span>
                                                        {formatMessageDate(
                                                            message.created_at,
                                                        )}
                                                    </span>
                                                </div>
                                            )}

                                            <div className={`message-row ${isOwnMessage ? "own" : ""}`}>
                                                <button
                                                    className="reply-message-button"
                                                    type="button"
                                                    onClick={() =>
                                                        startReply(message)
                                                    }
                                                    title="Reply"
                                                    aria-label="Reply to message"
                                                >
                                                    ↩
                                                </button>
                                                <div className="message-bubble">
                                                    {message.reply_to_message && (
                                                        <div className="message-reply">
                                                            <strong>
                                                                {message.reply_to_message.sender_id ===
                                                                user.id
                                                                    ? "You"
                                                                    : getUserDisplayName(
                                                                        activeChat.peer,
                                                                    )}
                                                            </strong>

                                                            <span>
                                                                {
                                                                    message.reply_to_message
                                                                        .content
                                                                }
                                                            </span>
                                                        </div>
                                                    )}
                                                    <div className="message-content">
                                                        {message.content}
                                                    </div>

                                                    <div className="message-meta">
                                                        <span className="message-time">
                                                            {formatTime(
                                                                message.created_at,
                                                            )}
                                                        </span>

                                                        {isOwnMessage && (
                                                            <span className={`message-receipt ${isRead ? "read" : ""}`} title={isRead ? "Read" : "Sent"}>
                                                                {isRead
                                                                    ? "✓✓"
                                                                    : "✓"}
                                                            </span>
                                                        )}
                                                    </div>
                                                </div>
                                            </div>
                                        </Fragment>
                                    );
                                })
                            )}

                            <div ref={messagesEndRef} />
                        </div>

                        <div className="message-composer">
    {replyingTo && (
        <div className="replying-preview">
            <div className="replying-preview-content">
                <strong>
                    Replying to{" "}
                    {replyingTo.sender_id ===
                    user.id
                        ? "yourself"
                        : getUserDisplayName(
                              activeChat.peer,
                          )}
                </strong>

                <span>
                    {replyingTo.content}
                </span>
            </div>

            <button
                type="button"
                onClick={() =>
                    setReplyingTo(null)
                }
                aria-label="Cancel reply"
            >
                ×
            </button>
        </div>
    )}

    <form
        className="message-form"
        onSubmit={handleSendMessage}
    >
        <input
            ref={messageInputRef}
            type="text"
            placeholder={
                replyingTo
                    ? "Write a reply..."
                    : "Write a message..."
            }
            value={messageInput}
            onChange={
                handleMessageInputChange
            }
            maxLength={4000}
            autoComplete="off"
        />

        <button
            type="submit"
            disabled={
                sending ||
                !messageInput.trim()
            }
        >
            ➤
        </button>
    </form>
</div>
                    </>
                ) : (
                    <div className="no-chat-selected">
                        <div className="logo">M</div>
                        <h2>Select a chat</h2>

                        <p>
                            Choose a conversation or search for a user.
                        </p>
                    </div>
                )}

                {error && !profileOpen && (
                    <div className="global-error">
                        {error}

                        <button type="button" onClick={() => setError("")}>
                            ×
                        </button>
                    </div>
                )}
            </section>

            {profileOpen && (
                <div className="profile-overlay">
                    <section className="profile-settings">
                        <header className="profile-settings-header">
                            <h2>Profile</h2>

                            <button type="button" onClick={closeProfileSettings} aria-label="Close">
                                ×
                            </button>
                        </header>

                        <div className="profile-avatar-section">
                            <UserAvatar
                                user={user}
                                className="profile-avatar"
                            />

                            <div className="profile-avatar-actions">
                                <label className="avatar-upload-button">
                                    {avatarUploading
                                        ? "Uploading..."
                                        : "Change photo"}

                                    <input type="file" accept="image/jpeg,image/png,image/webp" onChange={handleAvatarChange} disabled={avatarUploading} />
                                </label>

                                {user.avatar_url && (
                                    <button className="remove-avatar-button" type="button" onClick={() => void handleRemoveAvatar()} disabled={avatarUploading}>
                                        Remove photo
                                    </button>
                                )}
                            </div>
                        </div>

                        <form className="profile-form" onSubmit={handleProfileSave}>
                            <label htmlFor="profile-display-name">
                                Display name
                            </label>

                            <input id="profile-display-name" type="text" value={profileDisplayName} onChange={(event) => setProfileDisplayName(event.target.value)} maxLength={64} placeholder="Your name" autoComplete="name" />

                            <label htmlFor="profile-username">
                                Username
                            </label>

                            <div className="username-input">
                                <span>@</span>

                                <input id="profile-username" type="text" value={profileUsername} onChange={(event) => setProfileUsername(event.target.value)} minLength={3} maxLength={32} autoComplete="username" required />
                            </div>

                            <label htmlFor="profile-bio">
                                Bio
                            </label>

                            <textarea id="profile-bio" value={profileBio} onChange={(event) => setProfileBio(event.target.value)} maxLength={160} placeholder="Tell something about yourself" rows={4} />

                            <div className="bio-counter">
                                {profileBio.length}/160
                            </div>

                            {error && (
                                <div className="error-message">
                                    {error}
                                </div>
                            )}

                            {profileMessage && (
                                <div className="profile-success">
                                    {profileMessage}
                                </div>
                            )}

                            <button className="primary-button" type="submit" disabled={profileSaving || !profileUsername.trim()}>
                                {profileSaving
                                    ? "Saving..."
                                    : "Save changes"}
                            </button>
                        </form>
                    </section>
                </div>
            )}
        </main>
    );
}


export default App;
