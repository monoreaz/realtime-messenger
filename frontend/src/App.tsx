import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";

import {
    createPrivateChat,
    createWebSocket,
    getChats,
    getCurrentUser,
    getMessages,
    loginUser,
    registerUser,
    searchUsers,
    sendMessage,
    type Chat,
    type Message,
    type User,
    markChatRead,
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

function App() {
    const [token, setToken] = useState<string | null>(
        localStorage.getItem("access_token"),
    );

    const [user, setUser] = useState<User | null>(null);
    const [chats, setChats] = useState<Chat[]>([]);
    const [activeChat, setActiveChat] = useState<Chat | null>(null);
    const [messages, setMessages] = useState<Message[]>([]);

    const [mode, setMode] = useState<AuthMode>("login");
    const [username, setUsername] = useState("");
    const [password, setPassword] = useState("");

    const [searchQuery, setSearchQuery] = useState("");
    const [searchResults, setSearchResults] = useState<User[]>([]);

    const [messageInput, setMessageInput] = useState("");

    const [authLoading, setAuthLoading] = useState(false);
    const [appLoading, setAppLoading] = useState(Boolean(token));
    const [messagesLoading, setMessagesLoading] = useState(false);
    const [searchLoading, setSearchLoading] = useState(false);
    const [sending, setSending] = useState(false);

    const [error, setError] = useState("");
    const [wsStatus, setWsStatus] = useState<WebSocketStatus>("disconnected");

    const activeChatIdRef = useRef<string | null>(null);
    const messagesEndRef = useRef<HTMLDivElement | null>(null);


    useEffect(() => {
        activeChatIdRef.current = activeChat?.id ?? null;
    }, [activeChat]);


    useEffect(() => {
        messagesEndRef.current?.scrollIntoView({
            behavior: "smooth",
        });
    }, [messages]);


    useEffect(() => {
        if (!token) {
            setAppLoading(false);
            return;
        }

        async function loadSession() {
            try {
                const [currentUser, userChats] = await Promise.all([
                    getCurrentUser(token!),
                    getChats(token!),
                ]);

                setUser(currentUser);
                setChats(userChats);

                if (userChats.length > 0) {
                    await openChat(userChats[0], token!);
                }
            } catch {
                logout();
            } finally {
                setAppLoading(false);
            }
        }

        void loadSession();
    }, [token]);


    useEffect(() => {
        if (!token || !user) {
            return;
        }

        const websocket = createWebSocket();

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

            if (data.type === "message.new") {
                const incomingMessage = data.message as Message;

                const isActiveChat =
                    incomingMessage.chat_id === activeChatIdRef.current;

                setChats((currentChats) => {
                    const updatedChats = currentChats.map((chat) => {
                        if (chat.id !== incomingMessage.chat_id) {
                            return chat;
                        }

                        const shouldIncreaseUnread =
                            incomingMessage.sender_id !== user.id &&
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

                if (incomingMessage.sender_id !== user.id) {
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
            websocket.close();
        };
    }, [token, user]);


    async function handleAuthSubmit(event: FormEvent<HTMLFormElement>) {
        event.preventDefault();

        setError("");
        setAuthLoading(true);

        try {
            if (mode === "register") {
                await registerUser(
                    username,
                    password,
                );
            }

            const loginResponse = await loginUser(
                username,
                password,
            );

            localStorage.setItem(
                "access_token",
                loginResponse.access_token,
            );

            setToken(
                loginResponse.access_token,
            );

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


    async function openChat(chat: Chat, currentToken = token) {
        if (!currentToken) {
            return;
        }

        setActiveChat(chat);
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


    async function handleSearch(event: FormEvent<HTMLFormElement>) {
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
                    (existingChat) => existingChat.id === chat.id,
                );

                if (alreadyExists) {
                    return currentChats;
                }

                return [
                    chat,
                    ...currentChats,
                ];
            });

            setSearchQuery("");
            setSearchResults([]);

            await openChat(
                chat,
                token,
            );
        } catch (caughtError) {
            if (caughtError instanceof Error) {
                setError(caughtError.message);
            }
        }
    }


    async function handleSendMessage(event: FormEvent<HTMLFormElement>) {
        event.preventDefault();

        if (!token || !activeChat) {
            return;
        }

        const content = messageInput.trim();

        if (!content) {
            return;
        }

        setSending(true);
        setError("");

        try {
            const newMessage = await sendMessage(
                token,
                activeChat.id,
                content,
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
        } catch (caughtError) {
            if (caughtError instanceof Error) {
                setError(caughtError.message);
            }
        } finally {
            setSending(false);
        }
    }


    function logout() {
        localStorage.removeItem(
            "access_token",
        );

        setToken(null);
        setUser(null);
        setChats([]);
        setActiveChat(null);
        setMessages([]);
        setSearchResults([]);
        setSearchQuery("");
        setWsStatus("disconnected");
    }
      function closeChat() {
          setActiveChat(null);
          setMessages([]);
      }

    function formatTime(date: string) {
        return new Date(date).toLocaleTimeString([], {
            hour: "2-digit",
            minute: "2-digit",
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
                        <label htmlFor="username">Username</label>

                        <input id="username" type="text" value={username} onChange={(event) => setUsername(event.target.value)} minLength={3} maxLength={32} autoComplete="username" required />

                        <label htmlFor="password">Password</label>

                        <input id="password" type="password" value={password} onChange={(event) => setPassword(event.target.value)} minLength={8} autoComplete={mode === "login" ? "current-password" : "new-password"} required />

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
                        setMode(mode === "login" ? "register" : "login");
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
                        <div className="small-avatar">
                            {user.username.charAt(0).toUpperCase()}
                        </div>

                        <div>
                            <strong>{user.username}</strong>

                            <div className={`connection-status ${wsStatus}`}>
                                {wsStatus === "connected" ? "Online" : wsStatus}
                            </div>
                        </div>
                    </div>

                    <button className="logout-button" type="button" onClick={logout}>
                        Log out
                    </button>
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
                                    <div className="chat-avatar">
                                        {searchUser.username.charAt(0).toUpperCase()}
                                    </div>

                                    <span>{searchUser.username}</span>
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
                                <div className="chat-avatar">
                                    {chat.peer.username.charAt(0).toUpperCase()}
                                </div>

                                <div className="chat-info">
                                  <div className="chat-title-row">
                                      <strong>
                                          {chat.peer.username}
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
                            <div className="chat-avatar">
                                {activeChat.peer.username.charAt(0).toUpperCase()}
                            </div>

                            <div>
                                <strong>{activeChat.peer.username}</strong>
                                <span>Private chat</span>
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
                                messages.map((message) => {
                                    const isOwnMessage = message.sender_id === user.id;

                                    return (
                                        <div className={`message-row ${isOwnMessage ? "own" : ""}`} key={message.id}>
                                            <div className="message-bubble">
                                                <div className="message-content">
                                                    {message.content}
                                                </div>

                                                <span className="message-time">
                                                    {formatTime(message.created_at)}
                                                </span>
                                            </div>
                                        </div>
                                    );
                                })
                            )}

                            <div ref={messagesEndRef} />
                        </div>

                        <form className="message-form" onSubmit={handleSendMessage}>
                            <input type="text" placeholder="Write a message..." value={messageInput} onChange={(event) => setMessageInput(event.target.value)} maxLength={4000} autoComplete="off" />

                            <button type="submit" disabled={sending || !messageInput.trim()}>
                                ➤
                            </button>
                        </form>
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

                {error && (
                    <div className="global-error">
                        {error}

                        <button type="button" onClick={() => setError("")}>
                            ×
                        </button>
                    </div>
                )}
            </section>
        </main>
    );
}


export default App;