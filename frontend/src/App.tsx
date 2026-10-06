import { Fragment, useEffect, useLayoutEffect, useRef, useState } from "react";
import type { ChangeEvent, ClipboardEvent as ReactClipboardEvent, FormEvent, MouseEvent, PointerEvent as ReactPointerEvent, WheelEvent as ReactWheelEvent } from "react";

import {
    createPrivateChat,
    createWebSocket,
    getChats,
    getCurrentUser,
    getMediaUrl,
    getMessages,
    forgotPassword,
    loginUser,
    logoutSession,
    markChatRead,
    refreshSession,
    registerUser,
    resendVerification,
    resetPassword,
    removeAvatar,
    searchUsers,
    sendImageMessage,
    sendMessage,
    updateProfile,
    uploadAvatar,
    verifyEmail,
    changePassword,
    getSessions,
    logoutOtherSessions,
    revokeSession,
    editMessage,
    deleteMessage,
    getPinnedMessages,
    pinMessage,
    unpinMessage,
    sendVideoMessage,
    type SessionInfo,
    type Chat,
    type Message,
    type User,
} from "./api";

import "./App.css";
import { GroupDialog } from "./GroupDialog";
import { CallMessage } from "./CallMessage";
import { AudioCallDialog } from "./AudioCallDialog";
import { PinnedMessagesDialog } from "./PinnedMessagesDialog";
import { selectPinnedMessage } from "./pinnedMessages";
import { useAudioCall } from "./useAudioCall";


type AuthMode = "login" | "register" | "forgot" | "reset" | "resend";
type WebSocketStatus = "connecting" | "connected" | "disconnected";
type SettingsTab = "profile" | "security" | "sessions" | "privacy";


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


function getVisibleChats(chats: Chat[]): Chat[] {
    return sortChats(
        chats.filter((chat) => chat.type === "group" || chat.last_message !== null),
    );
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

function PeerProfileDialog({ user, onClose }: { user: User; onClose: () => void }) {
    const dialogRef = useRef<HTMLDialogElement | null>(null);

    useEffect(() => {
        const dialog = dialogRef.current;
        dialog?.showModal();
        return () => dialog?.close();
    }, []);

    return (
        <dialog
            ref={dialogRef}
            className="peer-profile-dialog"
            aria-labelledby="peer-profile-title"
            onCancel={onClose}
            onClick={(event) => {
                if (event.target === event.currentTarget) onClose();
            }}
        >
            <div className="peer-profile-card">
                <button
                    className="peer-profile-close"
                    type="button"
                    onClick={onClose}
                    aria-label="Close profile"
                    autoFocus
                >×</button>
                <UserAvatar user={user} className="peer-profile-avatar" />
                <h2 id="peer-profile-title">{getUserDisplayName(user)}</h2>
                <dl className="peer-profile-details">
                    <dt>Username</dt>
                    <dd>@{user.username}</dd>
                    <dt>Bio</dt>
                    <dd>{user.bio?.trim() || "No bio yet"}</dd>
                </dl>
            </div>
        </dialog>
    );
}


function getSessionDeviceName(
    userAgent: string | null,
): string {
    if (!userAgent) {
        return "Unknown device";
    }

    let browser = "Browser";
    let os = "Unknown OS";

    if (userAgent.includes("Edg/")) {
        browser = "Edge";
    } else if (userAgent.includes("Firefox/")) {
        browser = "Firefox";
    } else if (
        userAgent.includes("Chrome/") &&
        !userAgent.includes("Edg/")
    ) {
        browser = "Chrome";
    } else if (
        userAgent.includes("Safari/") &&
        !userAgent.includes("Chrome/")
    ) {
        browser = "Safari";
    }

    if (userAgent.includes("Windows")) {
        os = "Windows";
    } else if (
        userAgent.includes("Macintosh") ||
        userAgent.includes("Mac OS")
    ) {
        os = "macOS";
    } else if (userAgent.includes("Android")) {
        os = "Android";
    } else if (
        userAgent.includes("iPhone") ||
        userAgent.includes("iPad")
    ) {
        os = "iOS";
    } else if (userAgent.includes("Linux")) {
        os = "Linux";
    }

    return `${browser} on ${os}`;
}

function formatSessionDate(
    date: string | null,
): string {
    if (!date) {
        return "Never";
    }

    return new Date(date).toLocaleString();
}


function App() {
    const [token, setToken] = useState<string | null>(null);
    const [sessionChecked, setSessionChecked] = useState(false);

    const [user, setUser] = useState<User | null>(null);
    const [chats, setChats] = useState<Chat[]>([]);
    const [activeChat, setActiveChat] = useState<Chat | null>(null);
    const [draftPeer, setDraftPeer] = useState<User | null>(null);
    const [messages, setMessages] = useState<Message[]>([]);
    const [pinnedMessages, setPinnedMessages] = useState<Message[]>([]);
    const [pinnedListChatId, setPinnedListChatId] = useState<string | null>(null);
    const [pinnedAnchorId, setPinnedAnchorId] = useState<string | null>(null);
    const chatPins = pinnedMessages.filter(message => message.chat_id === activeChat?.id && !message.deleted_at);
    const displayedPin = selectPinnedMessage(chatPins, messages.find(message => message.id === pinnedAnchorId));

    const [mode, setMode] = useState<AuthMode>(() => {
        const params = new URLSearchParams(window.location.search);
        return params.has("reset") ? "reset" : "login";
    });

    const [username, setUsername] = useState("");
    const [email, setEmail] = useState("");
    const [password, setPassword] = useState("");
    const [confirmPassword, setConfirmPassword] = useState("");

    const [currentPassword, setCurrentPassword] = useState("");
    const [profileNewPassword, setProfileNewPassword] = useState("");
    const [profileConfirmPassword, setProfileConfirmPassword] = useState("");
    const [passwordChanging, setPasswordChanging] = useState(false);

    
    const [showPassword, setShowPassword] = useState(false);
    const [showConfirmPassword, setShowConfirmPassword] = useState(false);

    const [rememberMe, setRememberMe] = useState(false);
    const [authNotice, setAuthNotice] = useState("");

    const [resetToken, setResetToken] = useState<string | null>(() => {
        const params = new URLSearchParams(window.location.search);
        return params.get("reset");
    });

    const [newPassword, setNewPassword] = useState("");
    const [resetConfirmPassword, setResetConfirmPassword] = useState("");

    const [searchQuery, setSearchQuery] = useState("");
    const [searchResults, setSearchResults] = useState<User[]>([]);

    const [messageInput, setMessageInput] = useState("");
    const [selectedImage, setSelectedImage] = useState<File | null>(null);
    const [selectedImagePreview, setSelectedImagePreview] = useState<string | null>(null);

    const [selectedVideo, setSelectedVideo] = useState<File | null>(null);
    const [selectedVideoPreview, setSelectedVideoPreview] = useState<string | null>(null);

    const [imageViewerUrl, setImageViewerUrl] = useState<string | null>(null);

    const [imageViewerScale, setImageViewerScale] = useState(1);

    const [imageViewerPosition, setImageViewerPosition] =
        useState({
            x: 0,
            y: 0,
        });

    const [imageViewerDragging, setImageViewerDragging] =
        useState(false);

    const imageViewerDragStartRef = useRef({
        pointerX: 0,
        pointerY: 0,
        imageX: 0,
        imageY: 0,
    });
    const imageViewerImageRef = useRef<HTMLImageElement | null>(null);

    const [replyingTo, setReplyingTo] = useState<Message | null>(null);
    const [editingMessage, setEditingMessage] = useState<Message | null>(null);
    const [messageActionLoading, setMessageActionLoading] = useState<string | null>(null);
    const [contextMenu, setContextMenu] = useState<{
        message: Message;
        x: number;
        y: number;
    } | null>(null);
    const [deleteTarget, setDeleteTarget] = useState<Message | null>(null);

    const messageInputRef = useRef<HTMLInputElement | null>(null);
    const imageInputRef = useRef<HTMLInputElement | null>(null);


    const [authLoading, setAuthLoading] = useState(false);
    const [appLoading, setAppLoading] = useState(true);
    const [messagesLoading, setMessagesLoading] = useState(false);
    const [searchLoading, setSearchLoading] = useState(false);
    const [sending, setSending] = useState(false);

    const [sessions, setSessions] = useState<SessionInfo[]>([]);
    const [sessionsLoading, setSessionsLoading] = useState(false);
    const [sessionActionId, setSessionActionId] = useState<string | null>(null);
    const [loggingOutOthers, setLoggingOutOthers] = useState(false);

    const [error, setError] = useState("");
    const [wsStatus, setWsStatus] = useState<WebSocketStatus>("disconnected");

    const [onlineUserIds, setOnlineUserIds] = useState<Set<string>>(
        () => new Set(),
    );

    const [typingUserIds, setTypingUserIds] = useState<Set<string>>(
        () => new Set(),
    );

    const [profileOpen, setProfileOpen] = useState(false);
    const [groupDialogOpen, setGroupDialogOpen] = useState(false);
    const [groupMembersOpen, setGroupMembersOpen] = useState(false);
    const [peerProfileOpen, setPeerProfileOpen] = useState(false);

    const [settingsTab, setSettingsTab] = useState<SettingsTab>("profile");

    const [profileUsername, setProfileUsername] = useState("");
    const [profileDisplayName, setProfileDisplayName] = useState("");
    const [profileBio, setProfileBio] = useState("");
    const [profileSaving, setProfileSaving] = useState(false);
    const [avatarUploading, setAvatarUploading] = useState(false);
    const [profileMessage, setProfileMessage] = useState("");

    const activeChatIdRef = useRef<string | null>(null);
    const draftPeerIdRef = useRef<string | null>(null);
    const messagesContainerRef = useRef<HTMLDivElement | null>(null);
    const messagesContentRef = useRef<HTMLDivElement | null>(null);
    const keepLatestVisibleRef = useRef(true);
    const replyNavigationRef = useRef(0);
    const [replyTarget, setReplyTarget] = useState<{ id: string; request: number } | null>(null);
    const [showScrollToLatest, setShowScrollToLatest] = useState(false);
    const scrollToLatestOnOpenRef = useRef(false);
    const websocketRef = useRef<WebSocket | null>(null);
    const audioCall = useAudioCall(websocketRef, wsStatus === "connected");

    const typingTimeoutRef = useRef< ReturnType<typeof setTimeout> | null >(null);

    const typingChatIdRef = useRef<string | null>(null);


    useEffect(() => {
        setPeerProfileOpen(false);
    }, [activeChat?.id, draftPeer?.id, user?.id]);


    useLayoutEffect(() => {
        if (!replyTarget) return;
        const container = messagesContainerRef.current;
        const target = document.getElementById(`message-${replyTarget.id}`);
        if (!container || !target || !container.contains(target)) return;
        keepLatestVisibleRef.current = false;
        container.scrollTo({
            top: container.scrollTop + target.getBoundingClientRect().top
                - container.getBoundingClientRect().top
                - (container.clientHeight - target.clientHeight) / 2,
            behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches
                ? "instant" : "smooth",
        });
        target.focus({ preventScroll: true });
        const timeout = window.setTimeout(() => setReplyTarget(null), 2000);
        return () => window.clearTimeout(timeout);
    }, [replyTarget]);

    useEffect(() => {
        replyNavigationRef.current += 1;
        setReplyTarget(null);
        setPinnedListChatId(null);
    }, [activeChat?.id, draftPeer?.id]);

    function updatePinnedAnchor() {
        const container = messagesContainerRef.current;
        if (!container) return;
        if (container.scrollHeight - container.clientHeight - container.scrollTop < 48) {
            setPinnedAnchorId(null);
            return;
        }
        const bottom = container.getBoundingClientRect().top + container.clientTop + container.clientHeight;
        const rows = container.querySelectorAll<HTMLElement>(".message-row[id]");
        let anchorId: string | null = null;
        for (const row of rows) {
            if (row.getBoundingClientRect().top >= bottom) break;
            anchorId = row.id.slice("message-".length);
        }
        setPinnedAnchorId(anchorId);
    }

    useLayoutEffect(() => {
        updatePinnedAnchor();
        const content = messagesContentRef.current;
        const container = messagesContainerRef.current;
        if (!content || !container) return;
        const observer = new ResizeObserver(updatePinnedAnchor);
        observer.observe(content);
        observer.observe(container);
        return () => observer.disconnect();
    }, [activeChat?.id, messages, messagesLoading]);

    async function navigateToReply(messageId: string) {
        if (!token || !activeChat) return;
        const chatId = activeChat.id;
        const request = ++replyNavigationRef.current;
        const isCurrent = () => request === replyNavigationRef.current
            && activeChatIdRef.current === chatId;
        setReplyTarget(null);
        try {
            let history = messages;
            const olderMessages: Message[] = [];
            while (!history.some((message) => message.id === messageId)) {
                const oldest = history[0];
                if (!oldest) break;
                const page = await getMessages(token, chatId, oldest.id);
                if (!isCurrent()) return;
                if (page.length === 0) break;
                olderMessages.unshift(...page);
                history = [...page, ...history];
            }
            if (!isCurrent()) return;
            const target = history.find((message) => message.id === messageId);
            if (!target || target.deleted_at !== null) {
                setError("This message is no longer available.");
                return;
            }
            keepLatestVisibleRef.current = false;
            if (olderMessages.length > 0) {
                setMessages((current) => {
                    const ids = new Set(current.map((message) => message.id));
                    return [...olderMessages.filter((message) => !ids.has(message.id)), ...current];
                });
            }
            setReplyTarget({ id: messageId, request });
        } catch (caughtError) {
            if (isCurrent()) {
                setError(caughtError instanceof Error ? caughtError.message : "Could not load message");
            }
        }
    }

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


    function openProfileSettings() {
        if (!user) {
            return;
        }

        setProfileUsername(user.username);
        setProfileDisplayName(user.display_name ?? "");
        setProfileBio(user.bio ?? "");
        setProfileMessage("");
        setError("");
        setSettingsTab("profile");
        setProfileOpen(true);
    }


    function openSettingsTab(tab: SettingsTab) {
        setSettingsTab(tab);
        setProfileMessage("");
        setError("");

        if (tab === "sessions") {
            void loadSessions();
        }
    }

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

    async function loadSessions() {
    if (!token) {
        return;
    }

    setSessionsLoading(true);

    try {
        const activeSessions = await getSessions(token);
        setSessions(activeSessions);
    } catch (caughtError) {
        if (caughtError instanceof Error) {
            setError(caughtError.message);
        }
    } finally {
        setSessionsLoading(false);
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
        draftPeerIdRef.current = draftPeer?.id ?? null;
    }, [draftPeer]);


    useLayoutEffect(() => {
        if (messagesLoading) {
            return;
        }

        const container = messagesContainerRef.current;
        if (!container) {
            return;
        }

        if (!keepLatestVisibleRef.current && !scrollToLatestOnOpenRef.current) {
            return;
        }

        keepLatestVisibleRef.current = true;
        container.scrollTo({
            top: container.scrollHeight,
            behavior: scrollToLatestOnOpenRef.current ? "instant" : "smooth",
        });
        scrollToLatestOnOpenRef.current = false;
    }, [messages, messagesLoading]);

    useLayoutEffect(() => {
        const container = messagesContainerRef.current;
        const content = messagesContentRef.current;
        if (!container || !content) {
            return;
        }

        keepLatestVisibleRef.current = true;
        setShowScrollToLatest(false);

        // Images and videos can change the history height after it renders.
        const observer = new ResizeObserver(() => {
            if (keepLatestVisibleRef.current) {
                container.scrollTop = container.scrollHeight;
            }
            const isAtBottom =
                container.scrollHeight - container.clientHeight - container.scrollTop < 48;
            keepLatestVisibleRef.current = isAtBottom;
            setShowScrollToLatest(!isAtBottom);
        });
        observer.observe(content);
        observer.observe(container);
        return () => observer.disconnect();
    }, [activeChat?.id, draftPeer?.id, appLoading]);


    useEffect(() => {
        return () => {
            if (selectedImagePreview) {
                URL.revokeObjectURL(selectedImagePreview);
            }
        };
    }, [selectedImagePreview]);


    useEffect(() => {
        return () => {
            if (selectedVideoPreview) {
                URL.revokeObjectURL(selectedVideoPreview);
            }
        };
    }, [selectedVideoPreview]);


    useEffect(() => {
    if (!contextMenu) {
        return;
    }

    function handlePointerDown(event: PointerEvent) {
        const target = event.target as HTMLElement | null;

        if (
            target?.closest(
                ".message-context-menu",
            )
        ) {
            return;
        }

        setContextMenu(null);
    }

    function closeContextMenu() {
        setContextMenu(null);
    }

    function handleKeyDown(event: KeyboardEvent) {
        if (event.key === "Escape") {
            setContextMenu(null);
        }
    }

    window.addEventListener(
        "pointerdown",
        handlePointerDown,
    );

    window.addEventListener(
        "resize",
        closeContextMenu,
    );

    window.addEventListener(
        "keydown",
        handleKeyDown,
    );

    return () => {
        window.removeEventListener(
            "pointerdown",
            handlePointerDown,
        );

        window.removeEventListener(
            "resize",
            closeContextMenu,
        );

        window.removeEventListener(
            "keydown",
            handleKeyDown,
        );
    };
}, [contextMenu]);

    function resetImageViewerTransform() {
        setImageViewerScale(1);
        setImageViewerPosition({
            x: 0,
            y: 0,
        });
        setImageViewerDragging(false);
    }


    function openImageViewer(url: string) {
        setImageViewerUrl(url);
        resetImageViewerTransform();
    }


    async function openVideoFullscreen(video: HTMLVideoElement) {
        const webkitVideo = video as HTMLVideoElement & {
            webkitEnterFullscreen?: () => void;
        };

        const hideInlineControls = () => {
            video.controls = false;
        };

        video.controls = true;

        try {
            if (video.requestFullscreen) {
                await video.requestFullscreen();

                const handleFullscreenChange = () => {
                    if (document.fullscreenElement !== video) {
                        hideInlineControls();
                        document.removeEventListener(
                            "fullscreenchange",
                            handleFullscreenChange,
                        );
                    }
                };

                document.addEventListener(
                    "fullscreenchange",
                    handleFullscreenChange,
                );
            } else if (webkitVideo.webkitEnterFullscreen) {
                video.addEventListener(
                    "webkitendfullscreen",
                    hideInlineControls,
                    { once: true },
                );

                webkitVideo.webkitEnterFullscreen();
            } else {
                hideInlineControls();
                return;
            }

            void video.play().catch(() => undefined);
        } catch {
            hideInlineControls();
        }
    }


    function closeImageViewer() {
        setImageViewerUrl(null);
        resetImageViewerTransform();
    }


    function getChatImageUrls(): string[] {
        return messages
            .map((message) =>
                getMediaUrl(message.image_url),
            )
            .filter(
                (url): url is string =>
                    Boolean(url),
            );
    }


    function clampImageViewerPosition(
        x: number,
        y: number,
        scale: number,
    ) {
        const image = imageViewerImageRef.current;
        const stage = image?.parentElement;

        if (!image || !stage || scale <= 1) {
            return { x: 0, y: 0 };
        }

        const maxX = Math.max(
            0,
            (image.clientWidth * scale - stage.clientWidth) / 2,
        );
        const maxY = Math.max(
            0,
            (image.clientHeight * scale - stage.clientHeight) / 2,
        );

        return {
            x: Math.max(-maxX, Math.min(maxX, x)),
            y: Math.max(-maxY, Math.min(maxY, y)),
        };
    }


    function changeViewerImage(
        direction: number,
    ) {
        if (!imageViewerUrl) {
            return;
        }

        const imageUrls =
            getChatImageUrls();

        if (imageUrls.length <= 1) {
            return;
        }

        const currentIndex =
            imageUrls.indexOf(
                imageViewerUrl,
            );

        if (currentIndex === -1) {
            return;
        }

        const nextIndex =
            (
                currentIndex +
                direction +
                imageUrls.length
            ) %
            imageUrls.length;

        setImageViewerUrl(
            imageUrls[nextIndex],
        );

        resetImageViewerTransform();
    }


    function handleImageViewerWheel(
        event: ReactWheelEvent<HTMLDivElement>,
    ) {
        event.preventDefault();

        const zoomChange =
            event.deltaY < 0
                ? 0.2
                : -0.2;

        const nextScale = Math.min(
            5,
            Math.max(1, imageViewerScale + zoomChange),
        );

        setImageViewerScale(nextScale);
        setImageViewerPosition((position) =>
            clampImageViewerPosition(
                position.x,
                position.y,
                nextScale,
            ),
        );
    }


    function handleImageViewerDoubleClick(
        event: MouseEvent<HTMLImageElement>,
    ) {
        event.stopPropagation();

        setImageViewerScale(
            imageViewerScale > 1 ? 1 : 2,
        );
        setImageViewerPosition({ x: 0, y: 0 });
    }


    function handleImagePointerDown(
        event: ReactPointerEvent<HTMLImageElement>,
    ) {
        if (
            imageViewerScale <= 1 ||
            event.button !== 0
        ) {
            return;
        }

        event.currentTarget.setPointerCapture(
            event.pointerId,
        );

        imageViewerDragStartRef.current = {
            pointerX: event.clientX,
            pointerY: event.clientY,
            imageX:
                imageViewerPosition.x,
            imageY:
                imageViewerPosition.y,
        };

        setImageViewerDragging(true);
    }


    function handleImagePointerMove(
        event: ReactPointerEvent<HTMLImageElement>,
    ) {
        if (!imageViewerDragging) {
            return;
        }

        const start =
            imageViewerDragStartRef.current;

        setImageViewerPosition(
            clampImageViewerPosition(
                start.imageX + event.clientX - start.pointerX,
                start.imageY + event.clientY - start.pointerY,
                imageViewerScale,
            ),
        );
    }


    function handleImagePointerUp(
        event: ReactPointerEvent<HTMLImageElement>,
    ) {
        if (!imageViewerDragging) {
            return;
        }

        if (
            event.currentTarget.hasPointerCapture(
                event.pointerId,
            )
        ) {
            event.currentTarget.releasePointerCapture(
                event.pointerId,
            );
        }

        setImageViewerDragging(false);
    }


    useEffect(() => {
        if (!imageViewerUrl) {
            return;
        }

        if (!getChatImageUrls().includes(imageViewerUrl)) {
            closeImageViewer();
            return;
        }

        function handleKeyDown(event: KeyboardEvent) {
            if (event.key === "Escape") {
                closeImageViewer();
                return;
            }

            if (event.key === "ArrowLeft") {
                event.preventDefault();
                changeViewerImage(-1);
                return;
            }

            if (event.key === "ArrowRight") {
                event.preventDefault();
                changeViewerImage(1);
            }
        }

        const previousOverflow =
            document.body.style.overflow;

        document.body.style.overflow =
            "hidden";

        window.addEventListener(
            "keydown",
            handleKeyDown,
        );

        return () => {
            document.body.style.overflow =
                previousOverflow;

            window.removeEventListener(
                "keydown",
                handleKeyDown,
            );
        };
    }, [imageViewerUrl, messages]);

    useEffect(() => {
        const params = new URLSearchParams(window.location.search);
        const verificationToken = params.get("verify");

        if (!verificationToken) {
            return;
        }

        async function verifyAccountEmail() {
            setAuthLoading(true);
            setError("");

            try {
                await verifyEmail(verificationToken!);
                setMode("login");
                setAuthNotice(
                    "Email verified. You can now sign in.",
                );
            } catch (caughtError) {
                if (caughtError instanceof Error) {
                    setError(caughtError.message);
                } else {
                    setError("Email verification failed");
                }
            } finally {
                const url = new URL(window.location.href);
                url.searchParams.delete("verify");

                window.history.replaceState(
                    {},
                    "",
                    url.pathname + url.search + url.hash,
                );

                setAuthLoading(false);
            }
        }

        void verifyAccountEmail();
    }, []);


    useEffect(() => {
        async function restoreSession() {
            localStorage.removeItem("access_token");

            if (resetToken) {
                setToken(null);
                setUser(null);
                setSessionChecked(true);
                setAppLoading(false);
                return;
            }

            try {
                const session = await refreshSession();
                setToken(session.access_token);
            } catch {
                setToken(null);
            } finally {
                setSessionChecked(true);
            }
        }

        void restoreSession();
    }, [resetToken]);


    useEffect(() => {
        if (!sessionChecked) {
            return;
        }

        if (!token) {
            setUser(null);
            setChats([]);
            setActiveChat(null);
            setDraftPeer(null);
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
                const [currentUser, userChats] = await Promise.all([
                    getCurrentUser(token!),
                    getChats(token!),
                ]);

                setUser(currentUser);

                const sortedChats = getVisibleChats(userChats);
                setChats(sortedChats);
            } catch {
                setToken(null);
                setUser(null);
            } finally {
                setAppLoading(false);
            }
        }

        void loadSession();
    }, [token, sessionChecked, user]);


    useEffect(() => {
        if (!token || !user) {
            return;
        }

        const interval = window.setInterval(
            async () => {
                try {
                    const session = await refreshSession();
                    setToken(session.access_token);
                } catch {
                    setToken(null);
                    setUser(null);
                }
            },
            10 * 60 * 1000,
        );

        return () => {
            window.clearInterval(interval);
        };
    }, [Boolean(token), user?.id]);


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
            if (typeof data.type === "string" && data.type.startsWith("call.")) {
                void audioCall.receive(data);
                return;
            }

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

            if (data.type === "chat.created") {
                void getChats(token).then(items => setChats(getVisibleChats(items)))
                    .catch(() => setError("Could not refresh chats"));
                return;
            }

            if (data.type === "chat.read") {
                setChats((currentChats) =>
                    currentChats.map((chat) =>
                        chat.id === data.chat_id && chat.type === "private"
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
                        currentChat.id !== data.chat_id || currentChat.type !== "private"
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

                const updateChatUser = (chat: Chat): Chat => ({
                    ...chat,
                    peer: chat.peer?.id === updatedUser.id ? updatedUser : chat.peer,
                    members: chat.members.map(member => member.id === updatedUser.id ? updatedUser : member),
                });
                setChats(items => items.map(updateChatUser));
                setActiveChat(chat => chat ? updateChatUser(chat) : null);

                setDraftPeer((currentPeer) =>
                    currentPeer?.id === updatedUser.id
                        ? updatedUser
                        : currentPeer,
                );

                setSearchResults((currentUsers) =>
                    currentUsers.map((searchUser) =>
                        searchUser.id === updatedUser.id
                            ? updatedUser
                            : searchUser,
                    ),
                );

                return;
            }

            if (
                data.type === "message.updated" ||
                data.type === "message.deleted"
            ) {
                const updatedMessage = data.message as Message;

                if (updatedMessage.chat_id === activeChatIdRef.current) {
                    setMessages((currentMessages) =>
                        currentMessages.map((message) =>
                            message.id === updatedMessage.id ? updatedMessage : message,
                        ),
                    );
                    setPinnedMessages((currentPinned) => {
                        const rest = currentPinned.filter((message) => message.id !== updatedMessage.id);
                        return updatedMessage.pinned_at ? [updatedMessage, ...rest] : rest;
                    });
                }

                if (data.type === "message.deleted") {
                    setReplyingTo((currentReply) =>
                        currentReply?.id === updatedMessage.id
                            ? null
                            : currentReply,
                    );

                    setEditingMessage((currentEditingMessage) => {
                        if (currentEditingMessage?.id === updatedMessage.id) {
                            setMessageInput("");
                            return null;
                        }

                        return currentEditingMessage;
                    });

                    setContextMenu((currentMenu) =>
                        currentMenu?.message.id === updatedMessage.id
                            ? null
                            : currentMenu,
                    );

                    setDeleteTarget((currentTarget) =>
                        currentTarget?.id === updatedMessage.id
                            ? null
                            : currentTarget,
                    );
                }

                void getChats(token)
                    .then((updatedChats) => {
                        const normalizedChats = updatedChats.map((chat) =>
                            chat.id === activeChatIdRef.current
                                ? {
                                      ...chat,
                                      unread_count: 0,
                                  }
                                : chat,
                        );

                        setChats(getVisibleChats(normalizedChats));
                    })
                    .catch((error) => {
                        console.error(
                            "Could not synchronize chats after message update:",
                            error,
                        );
                    });

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
                                image_url: incomingMessage.image_url,
                                video_url: incomingMessage.video_url,
                                created_at: incomingMessage.created_at,
                            },
                            unread_count: shouldIncreaseUnread
                                ? chat.unread_count + 1
                                : chat.unread_count,
                        };
                    });

                    return getVisibleChats(updatedChats);
                });

                // Synchronize the sidebar with the backend on every new
                // message. This also adds chats that were created by another
                // user after this page was opened.
                void getChats(token)
                    .then((updatedChats) => {
                        const normalizedChats = updatedChats.map((chat) =>
                            chat.id === activeChatIdRef.current
                                ? {
                                      ...chat,
                                      unread_count: 0,
                                  }
                                : chat,
                        );

                        const visibleChats = getVisibleChats(normalizedChats);
                        setChats(visibleChats);

                        const draftPeerId = draftPeerIdRef.current;
                        const incomingDraftChat =
                            draftPeerId === incomingMessage.sender_id
                                ? visibleChats.find(
                                      (chat) =>
                                          chat.type === "private" && chat.id === incomingMessage.chat_id,
                                  )
                                : undefined;

                        if (incomingDraftChat) {
                            setDraftPeer(null);
                            setActiveChat(incomingDraftChat);
                            activeChatIdRef.current = incomingDraftChat.id;
                            setMessages((currentMessages) => {
                                const alreadyExists = currentMessages.some(
                                    (message) =>
                                        message.id === incomingMessage.id,
                                );

                                return alreadyExists
                                    ? currentMessages
                                    : [...currentMessages, incomingMessage];
                            });

                            if (incomingMessage.sender_id !== currentUserId) {
                                void markChatRead(
                                    token,
                                    incomingMessage.chat_id,
                                );
                            }
                        }
                    })
                    .catch((error) => {
                        console.error(
                            "Could not synchronize chats:",
                            error,
                        );
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
            if (mode === "forgot") {
                await forgotPassword(email);

                setAuthNotice(
                    "If an account with that email exists, a password reset link has been sent.",
                );
                setMode("login");
                return;
            }

            if (mode === "resend") {
                await resendVerification(email);

                setAuthNotice(
                    "If an unverified account with that email exists, a new verification link has been sent.",
                );
                setMode("login");
                return;
            }

            if (mode === "reset") {
                if (!resetToken) {
                    setError("Invalid password reset link");
                    return;
                }

                if (newPassword !== resetConfirmPassword) {
                    setError("Passwords do not match");
                    return;
                }

                await resetPassword(resetToken, newPassword);

                setNewPassword("");
                setResetConfirmPassword("");
                setResetToken(null);

                const url = new URL(window.location.href);
                url.searchParams.delete("reset");

                window.history.replaceState(
                    {},
                    "",
                    url.pathname + url.search + url.hash,
                );

                setMode("login");
                setAuthNotice(
                    "Password changed. You can now sign in with your new password.",
                );
                return;
            }

            if (mode === "register") {
                if (password !== confirmPassword) {
                    setError(
                        "Passwords do not match"
                    );
                    return;
                }
                await registerUser(username, email, password);
                setPassword("");
                setConfirmPassword("");
                setShowPassword(false);
                setShowConfirmPassword(false);
                setMode("login");
                setAuthNotice(
                    `Account created. Verify ${email} before signing in.`,
                );
                return;
            }

            const loginResponse = await loginUser(
                username,
                password,
                rememberMe,
            );

            setAuthNotice("");
            setUser(null);
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
        setEditingMessage(null);
        setContextMenu(null);
        setDeleteTarget(null);
        clearSelectedImage();
        clearSelectedVideo();
        setMessageInput("");
        setDraftPeer(null);
        setActiveChat(chat);
        setTypingUserIds(new Set());

        if (chat.peer) sendWebSocketEvent({
            type: "presence.get",
            user_id: chat.peer.id,
        });
        setGroupMembersOpen(false);

        scrollToLatestOnOpenRef.current = true;
        setMessagesLoading(true);
        setError("");

        try {
            const [chatMessages, chatPinnedMessages] = await Promise.all([
                getMessages(currentToken, chat.id),
                getPinnedMessages(currentToken, chat.id),
            ]);

            setMessages(chatMessages);
            setPinnedMessages(chatPinnedMessages);

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

        const existingChat = chats.find(
            (chat) => chat.peer?.id === targetUser.id,
        );

        setSearchQuery("");
        setSearchResults([]);

        if (existingChat) {
            await openChat(existingChat, token);
            return;
        }

        stopTyping();
        setReplyingTo(null);
        setEditingMessage(null);
        setContextMenu(null);
        setDeleteTarget(null);
        clearSelectedImage();
        clearSelectedVideo();
        setMessageInput("");
        setActiveChat(null);
        setDraftPeer(targetUser);
        setMessages([]);
        setPinnedMessages([]);
        setTypingUserIds(new Set());
        setMessagesLoading(false);

        sendWebSocketEvent({
            type: "presence.get",
            user_id: targetUser.id,
        });

        requestAnimationFrame(() => {
            messageInputRef.current?.focus();
        });
    }


    function startReply(
        message: Message,
    ) {
        if (editingMessage) {
            setEditingMessage(null);
            setMessageInput("");
        }

        setReplyingTo(message);
        setContextMenu(null);

        requestAnimationFrame(() => {
            messageInputRef.current?.focus();
        });
    }


    function startEditing(
        message: Message,
    ) {
        setReplyingTo(null);
        clearSelectedImage();
        clearSelectedVideo();
        setEditingMessage(message);
        setMessageInput(message.content);
        setContextMenu(null);

        requestAnimationFrame(() => {
            messageInputRef.current?.focus();
            messageInputRef.current?.select();
        });
    }


    function cancelEditing() {
        setEditingMessage(null);
        setMessageInput("");
    }


    function openMessageContextMenu(
        event: MouseEvent<HTMLDivElement>,
        message: Message,
    ) {
        event.preventDefault();
        event.stopPropagation();

        const menuWidth = 184;
        const menuHeight = message.sender_id === user?.id ? 180 : 96;

        setContextMenu({
            message,
            x: Math.max(
                8,
                Math.min(event.clientX, window.innerWidth - menuWidth - 8),
            ),
            y: Math.max(
                8,
                Math.min(event.clientY, window.innerHeight - menuHeight - 8),
            ),
        });
    }


    async function togglePinnedMessage(message: Message) {
        if (!token || !activeChat || message.deleted_at) return;
        setContextMenu(null);
        setMessageActionLoading(message.id);
        try {
            const updated = message.pinned_at
                ? await unpinMessage(token, activeChat.id, message.id)
                : await pinMessage(token, activeChat.id, message.id);
            setMessages((current) => current.map((item) => item.id === updated.id ? updated : item));
            setPinnedMessages((current) => updated.pinned_at ? [updated, ...current.filter((item) => item.id !== updated.id)] : current.filter((item) => item.id !== updated.id));
        } catch (caughtError) {
            setError(caughtError instanceof Error ? caughtError.message : "Could not update pinned message");
        } finally { setMessageActionLoading(null); }
    }

    function jumpToMessage(messageId: string) {
        const target = document.getElementById(`message-${messageId}`);
        if (target) { target.scrollIntoView({ behavior: "smooth", block: "center" }); setReplyTarget({ id: messageId, request: ++replyNavigationRef.current }); }
        else void navigateToReply(messageId);
    }

    function requestDeleteMessage(
        message: Message,
    ) {
        setContextMenu(null);
        setDeleteTarget(message);
    }


    async function submitEditedMessage() {
        if (!token || !activeChat || !editingMessage) {
            return;
        }

        const content = messageInput.trim();

        if (!content) {
            return;
        }

        if (content === editingMessage.content) {
            cancelEditing();
            return;
        }

        setMessageActionLoading(editingMessage.id);
        setError("");
        stopTyping();

        try {
            const updatedMessage = await editMessage(
                token,
                activeChat.id,
                editingMessage.id,
                content,
            );

            setMessages((currentMessages) =>
                currentMessages.map((message) =>
                    message.id === updatedMessage.id
                        ? updatedMessage
                        : message,
                ),
            );

            setEditingMessage(null);
            setMessageInput("");

            try {
                const updatedChats = await getChats(token);
                setChats(getVisibleChats(updatedChats));
            } catch (syncError) {
                console.error(
                    "Could not synchronize chats after editing:",
                    syncError,
                );
            }
        } catch (caughtError) {
            if (caughtError instanceof Error) {
                setError(caughtError.message);
            }
        } finally {
            setMessageActionLoading(null);
        }
    }


    async function confirmDeleteMessage() {
        if (!token || !activeChat || !deleteTarget) {
            return;
        }

        const message = deleteTarget;

        setMessageActionLoading(message.id);
        setError("");

        try {
            const deletedMessage = await deleteMessage(
                token,
                activeChat.id,
                message.id,
            );

            setMessages((currentMessages) =>
                currentMessages.map((currentMessage) =>
                    currentMessage.id === deletedMessage.id
                        ? deletedMessage
                        : currentMessage,
                ),
            );

            if (replyingTo?.id === message.id) {
                setReplyingTo(null);
            }

            if (editingMessage?.id === message.id) {
                cancelEditing();
            }

            setDeleteTarget(null);

            try {
                const updatedChats = await getChats(token);
                setChats(getVisibleChats(updatedChats));
            } catch (syncError) {
                console.error(
                    "Could not synchronize chats after deleting:",
                    syncError,
                );
            }
        } catch (caughtError) {
            if (caughtError instanceof Error) {
                setError(caughtError.message);
            }
        } finally {
            setMessageActionLoading(null);
        }
    }


    function clearSelectedImage() {
        setSelectedImage(null);
        setSelectedImagePreview(null);

        if (imageInputRef.current) {
            imageInputRef.current.value = "";
        }
    }

    function clearSelectedVideo() {
        setSelectedVideo(null);
        setSelectedVideoPreview(null);
    }


    function selectVideo(file: File) {
        const allowedTypes = new Set([
            "video/mp4",
            "video/webm",
        ]);

        if (!allowedTypes.has(file.type)) {
            setError("Video must be MP4 or WebM");
            return;
        }

        if (file.size > 50 * 1024 * 1024) {
            setError("Video must be smaller than 50 MB");
            return;
        }

        clearSelectedImage();
        setError("");
        setSelectedVideo(file);
        setSelectedVideoPreview(
            URL.createObjectURL(file),
        );
    }

    function selectImage(file: File) {
        const allowedTypes = new Set([
            "image/jpeg",
            "image/png",
            "image/webp",
        ]);

        if (!allowedTypes.has(file.type)) {
            setError("Image must be JPEG, PNG, or WebP");
            return;
        }

        if (file.size > 10 * 1024 * 1024) {
            setError("Image must be smaller than 10 MB");
            return;
        }

        clearSelectedVideo();

        setError("");
        setSelectedImage(file);
        setSelectedImagePreview(URL.createObjectURL(file));
    }


    function handleAttachmentSelect(
        event: ChangeEvent<HTMLInputElement>,
    ) {
        const file = event.target.files?.[0];

        event.target.value = "";

        if (!file) {
            return;
        }

        if (file.type.startsWith("image/")) {
            selectImage(file);
            return;
        }

        if (file.type.startsWith("video/")) {
            selectVideo(file);
            return;
        }

        setError("File must be JPEG, PNG, WebP, MP4, or WebM");
    }


    function handleImagePaste(
        event: ReactClipboardEvent<HTMLFormElement>,
    ) {
        const imageItem = Array.from(event.clipboardData.items).find(
            (item) =>
                item.kind === "file" &&
                item.type.startsWith("image/"),
        );

        if (!imageItem) {
            return;
        }

        event.preventDefault();

        if (editingMessage || sending) {
            return;
        }

        const file = imageItem.getAsFile();

        if (file) {
            selectImage(file);
        }
    }


    async function handleSendMessage(
        event: FormEvent<HTMLFormElement>,
    ) {
        event.preventDefault();

        if (editingMessage) {
            await submitEditedMessage();
            return;
        }

        if (!token || (!activeChat && !draftPeer)) {
            return;
        }

        const content = messageInput.trim();

        if (!content && !selectedImage && !selectedVideo) {
            return;
        }

        stopTyping();
        setSending(true);
        setError("");

        try {
            let targetChat = activeChat;
            const wasDraft = targetChat === null;

            if (!targetChat) {
                if (!draftPeer) {
                    return;
                }

                targetChat = await createPrivateChat(
                    token,
                    draftPeer.id,
                );
            }

            const newMessage = selectedImage
                ? await sendImageMessage(
                      token,
                      targetChat.id,
                      selectedImage,
                      content,
                      replyingTo?.id ?? null,
                  )
                : selectedVideo
                  ? await sendVideoMessage(
                        token,
                        targetChat.id,
                        selectedVideo,
                        content,
                        replyingTo?.id ?? null,
                    )
                  : await sendMessage(
                        token,
                        targetChat.id,
                        content,
                        replyingTo?.id ?? null,
                    );

            if (wasDraft) {
                setDraftPeer(null);
                setActiveChat(targetChat);
                activeChatIdRef.current = targetChat.id;
            }

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

            try {
                const updatedChats = await getChats(token);
                setChats(getVisibleChats(updatedChats));
            } catch (syncError) {
                console.error(
                    "Could not synchronize chats after sending:",
                    syncError,
                );
            }

            setMessageInput("");
            clearSelectedImage();
            clearSelectedVideo();
            setReplyingTo(null);
        } catch (caughtError) {
            if (caughtError instanceof Error) {
                setError(caughtError.message);
            }
        } finally {
            setSending(false);
        }
    }


    async function handleRevokeSession(
    sessionId: string,
) {
    if (!token) {
        return;
    }

    setSessionActionId(sessionId);
    setError("");
    setProfileMessage("");

    try {
        await revokeSession(
            token,
            sessionId,
        );

        setSessions((currentSessions) =>
            currentSessions.filter(
                (session) =>
                    session.id !== sessionId,
            ),
        );

        setProfileMessage(
            "Session logged out"
        );
    } catch (caughtError) {
        if (caughtError instanceof Error) {
            setError(caughtError.message);
        } else {
            setError(
                "Could not log out session"
            );
        }
    } finally {
        setSessionActionId(null);
    }
}

    async function handleLogoutOtherSessions() {
    if (!token) {
        return;
    }

    setLoggingOutOthers(true);
    setError("");
    setProfileMessage("");

    try {
        await logoutOtherSessions(token);

        setSessions((currentSessions) =>
            currentSessions.filter(
                (session) => session.current,
            ),
        );

        setProfileMessage(
            "Other sessions logged out"
        );
    } catch (caughtError) {
        if (caughtError instanceof Error) {
            setError(caughtError.message);
        } else {
            setError(
                "Could not log out other sessions"
            );
        }
    } finally {
        setLoggingOutOthers(false);
    }
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

    async function handleChangePassword(
    event: FormEvent<HTMLFormElement>,
) {
    event.preventDefault();

    if (!token) {
        return;
    }

    setError("");
    setProfileMessage("");

    if (
        profileNewPassword !==
        profileConfirmPassword
    ) {
        setError("Passwords do not match");
        return;
    }

    if (
        currentPassword ===
        profileNewPassword
    ) {
        setError(
            "New password must be different from current password"
        );
        return;
    }

    setPasswordChanging(true);

    try {
        await changePassword(
            token,
            currentPassword,
            profileNewPassword,
        );

        setCurrentPassword("");
        setProfileNewPassword("");
        setProfileConfirmPassword("");

        await logout();

        setAuthNotice(
            "Password changed. Sign in again."
        );
    } catch (caughtError) {
        if (caughtError instanceof Error) {
            setError(caughtError.message);
        } else {
            setError(
                "Could not change password"
            );
        }
    } finally {
        setPasswordChanging(false);
    }
}

    function closeProfileSettings() {
        setProfileOpen(false);
        setSettingsTab("profile");
        setProfileMessage("");
        setError("");
    }


    async function logout() {
        stopTyping();

        try {
            await logoutSession();
        } catch {
            // Clear local state even if the backend is unavailable.
        }

        setReplyingTo(null);
        setEditingMessage(null);
        setContextMenu(null);
        setDeleteTarget(null);
        clearSelectedImage();
        clearSelectedVideo();
        setMessageInput("");
        setToken(null);
        setUser(null);
        setChats([]);
        setActiveChat(null);
        setDraftPeer(null);
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
        setEditingMessage(null);
        setContextMenu(null);
        setDeleteTarget(null);
        clearSelectedImage();
        clearSelectedVideo();
        setMessageInput("");
        setActiveChat(null);
        setDraftPeer(null);
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


    if (!sessionChecked) {
        return (
            <main className="loading-page">
                Loading Messenger...
            </main>
        );
    }


    if (!token) {
        return (
            <main className="auth-page">
                <section className="auth-card">
                    <div className="logo">M</div>

                    <h1>Messenger</h1>

                    <p className="subtitle">
                        {mode === "login" && "Sign in to continue"}
                        {mode === "register" && "Create your account"}
                        {mode === "forgot" && "Reset your password"}
                        {mode === "reset" && "Choose a new password"}
                        {mode === "resend" && "Resend verification email"}
                    </p>

                    <form onSubmit={handleAuthSubmit}>
                        {(mode === "login" || mode === "register") && (
                            <>
                                <label htmlFor="username">
                                    {mode === "login"
                                        ? "Username or email"
                                        : "Username"}
                                </label>

                                <input
                                    id="username"
                                    type="text"
                                    value={username}
                                    onChange={(event) =>
                                        setUsername(event.target.value)
                                    }
                                    placeholder={
                                        mode === "login"
                                            ? "Username or email"
                                            : "Username"
                                    }
                                    autoComplete="username"
                                    required
                                />
                            </>
                        )}

                        {(mode === "register" || mode === "forgot" || mode === "resend") && (
                            <>
                                <label htmlFor="email">
                                    Email
                                </label>

                                <input
                                    id="email"
                                    type="email"
                                    value={email}
                                    onChange={(event) =>
                                        setEmail(event.target.value)
                                    }
                                    autoComplete="email"
                                    required
                                />
                            </>
                        )}

                        {(mode === "login" || mode === "register") && (
                            <>
                                <label htmlFor="password">
                                    Password
                                </label>

                                <div className="password-input">
                                    <input
                                        id="password"
                                        type={
                                            showPassword
                                                ? "text"
                                                : "password"
                                        }
                                        value={password}
                                        onChange={(event) =>
                                            setPassword(
                                                event.target.value
                                            )
                                        }
                                        minLength={8}
                                        autoComplete={
                                            mode === "login"
                                                ? "current-password"
                                                : "new-password"
                                        }
                                        required
                                    />

                                    <button
                                        type="button"
                                        className="password-toggle"
                                        onClick={() =>
                                            setShowPassword(
                                                (current) =>
                                                    !current
                                            )
                                        }
                                        aria-label={
                                            showPassword
                                                ? "Hide password"
                                                : "Show password"
                                        }
                                    >
                                        {showPassword
                                            ? "🙈"
                                            : "👁"}
                                    </button>
                        </div>
                            </>
                        )}

                        {mode === "reset" && (
                            <>
                                <label htmlFor="new-password">
                                    New password
                                </label>

                                <input
                                    id="new-password"
                                    type="password"
                                    value={newPassword}
                                    onChange={(event) =>
                                        setNewPassword(event.target.value)
                                    }
                                    autoComplete="new-password"
                                    minLength={8}
                                    maxLength={128}
                                    required
                                />

                                <label htmlFor="confirm-password">
                                    Confirm password
                                </label>

                                <input
                                    id="confirm-password"
                                    type="password"
                                    value={resetConfirmPassword}
                                    onChange={(event) =>
                                        setResetConfirmPassword(event.target.value)
                                    }
                                    autoComplete="new-password"
                                    minLength={8}
                                    maxLength={128}
                                    required
                                />
                            </>
                        )}

                        {mode === "register" && (
    <>
                        <label htmlFor="confirm-password">
                            Confirm password
                        </label>

                        <div className="password-input">
                            <input
                                id="confirm-password"
                                type={
                                    showConfirmPassword
                                        ? "text"
                                        : "password"
                                }
                                value={confirmPassword}
                                onChange={(event) =>
                                    setConfirmPassword(
                                        event.target.value
                                    )
                                }
                                minLength={8}
                                autoComplete="new-password"
                                required
                            />

                            <button
                                type="button"
                                className="password-toggle"
                                onClick={() =>
                                    setShowConfirmPassword(
                                        (current) =>
                                            !current
                                    )
                                }
                                aria-label={
                                    showConfirmPassword
                                        ? "Hide password"
                                        : "Show password"
                                }
                            >
                                {showConfirmPassword
                                    ? "🙈"
                                    : "👁"}
                            </button>
                        </div>
                    </>
                )}

                        {mode === "login" && (
                            <label className="remember-me">
                                <input
                                    type="checkbox"
                                    checked={rememberMe}
                                    onChange={(event) =>
                                        setRememberMe(event.target.checked)
                                    }
                                />

                                <span>Remember me</span>
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

                        <button
                            className="primary-button"
                            type="submit"
                            disabled={authLoading}
                        >
                            {authLoading
                                ? "Please wait..."
                                : mode === "login"
                                  ? "Sign in"
                                  : mode === "register"
                                    ? "Create account"
                                    : mode === "forgot"
                                      ? "Send reset link"
                                      : mode === "resend"
                                        ? "Resend verification email"
                                        : "Change password"}
                        </button>
                    </form>

                    {mode === "login" && (
                        <>
                            <button
                                className="mode-button"
                                type="button"
                                onClick={() => {
                                    setError("");
                                    setAuthNotice("");
                                    setMode("forgot");
                                }}
                            >
                                Forgot password?
                            </button>

                            <button
                                className="mode-button"
                                type="button"
                                onClick={() => {
                                    setError("");
                                    setAuthNotice("");
                                    setMode("resend");
                                }}
                            >
                                Resend verification email
                            </button>
                        </>
                    )}

                    {(mode === "forgot" || mode === "resend") && (
                        <button
                            className="mode-button"
                            type="button"
                            onClick={() => {
                                setError("");
                                setMode("login");
                            }}
                        >
                            Back to sign in
                        </button>
                    )}

                    {mode === "reset" && (
                        <button
                            className="mode-button"
                            type="button"
                            onClick={() => {
                                setError("");
                                setMode("login");
                            }}
                        >
                            Back to sign in
                        </button>
                    )}

                    {(mode === "login" || mode === "register") && (
                        <button
                            className="mode-button"
                            type="button"
                            onClick={() => {
                                setError("");
                                setAuthNotice("");
                                setPassword("");
                                setConfirmPassword("");
                                setShowPassword(false);
                                setShowConfirmPassword(false);
                                setMode(
                                    mode === "login"
                                        ? "register"
                                        : "login",
                                );
                            }}
                        >
                            {mode === "login"
                                ? "Create an account"
                                : "Already have an account?"}
                        </button>
                    )}
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


    const currentPeer = activeChat?.peer ?? draftPeer;
    const senderName = (senderId: string) => {
        if (senderId === user.id) return "You";
        const sender = activeChat?.members.find(member => member.id === senderId) ?? currentPeer;
        return sender ? getUserDisplayName(sender) : "Participant";
    };


    return (
        <main className={`messenger ${activeChat || currentPeer ? "chat-open" : ""}`}>
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
                        <button className="settings-button" type="button" onClick={() => setGroupDialogOpen(true)} aria-label="Create group" title="Create group">＋</button>
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
                                {chat.peer ? <UserAvatar user={chat.peer} className="chat-avatar" /> : <div className="chat-avatar group-avatar" aria-hidden="true">👥</div>}

                                <div className="chat-info">
                                    <div className="chat-title-row">
                                        <strong>
                                            {chat.type === "group" ? chat.name : chat.peer ? getUserDisplayName(chat.peer) : "Chat"}
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
                                                  }${
                                                      chat.last_message.content ||
                                                      (chat.last_message.image_url
                                                          ? "Photo"
                                                          : chat.last_message.video_url
                                                            ? "Video"
                                                            : "Message")
                                                  }`
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

            {groupDialogOpen && token && <GroupDialog token={token} currentUserId={user.id} chats={chats} onClose={() => setGroupDialogOpen(false)} onCreated={chat => {
                setGroupDialogOpen(false);
                setChats(items => sortChats([chat, ...items.filter(item => item.id !== chat.id)]));
                void openChat(chat);
            }} />}
            {audioCall.error && !audioCall.call && <div className="call-notice" role="status">{audioCall.error} <button onClick={audioCall.play}>Enable audio</button><button onClick={audioCall.dismissError} aria-label="Close">×</button></div>}
            {audioCall.call && <AudioCallDialog name={audioCall.call.name} phase={audioCall.call.phase} elapsedSeconds={audioCall.elapsedSeconds} muted={audioCall.muted} onAccept={() => void audioCall.accept()} onEnd={() => audioCall.end()} onToggleMute={audioCall.toggleMute} error={audioCall.error} onPlayAudio={audioCall.play} />}
            <section className="chat-panel">
                {activeChat || currentPeer ? (
                    <>
                        <header className="chat-header">
                            <button className="mobile-back-button" type="button" onClick={closeChat} aria-label="Back to chats">
                                ←
                            </button>

                            {activeChat?.type === "group" ? <>
                                <div className="chat-avatar group-avatar" aria-hidden="true">👥</div>
                                <div className="chat-peer-info">
                                    <button className="peer-profile-trigger" type="button" onClick={() => setGroupMembersOpen(open => !open)} aria-expanded={groupMembersOpen}>
                                        {activeChat.name}
                                    </button>
                                    <span className="peer-status">{typingUserIds.size ? "Someone is typing..." : `${activeChat.members.length} participants`}</span>
                                </div>
                            </> : currentPeer && <>
                            <UserAvatar
                                user={currentPeer}
                                className="chat-avatar"
                            />

                            <div className="chat-peer-info">
                                <button
                                    className="peer-profile-trigger"
                                    type="button"
                                    onClick={() => setPeerProfileOpen(true)}
                                    aria-haspopup="dialog"
                                    aria-label={`View profile of ${getUserDisplayName(currentPeer)}`}
                                >
                                    {getUserDisplayName(currentPeer)}
                                </button>

                                <span
                                    className={
                                        typingUserIds.has(
                                            currentPeer.id,
                                        )
                                            ? "peer-status typing"
                                            : onlineUserIds.has(
                                                  currentPeer.id,
                                              )
                                              ? "peer-status online"
                                              : "peer-status"
                                    }
                                >
                                    {typingUserIds.has(
                                        currentPeer.id,
                                    )
                                        ? "typing..."
                                        : onlineUserIds.has(
                                              currentPeer.id,
                                          )
                                          ? "Online"
                                          : "Offline"}
                                </span>
                            </div>
                            <button className="call-button" type="button" aria-label="Start audio call" title="Start audio call" disabled={wsStatus !== "connected" || Boolean(audioCall.call)} onClick={() => {
                                if (!token) return;
                                const peer = currentPeer;
                                void createPrivateChat(token, peer.id)
                                    .then(() => audioCall.start(peer.id, getUserDisplayName(peer)))
                                    .catch((cause: unknown) => setError(cause instanceof Error ? cause.message : "Could not start the call"));
                            }}>
                                <svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><path d="M22 16.9v3a2 2 0 0 1-2.2 2A19.8 19.8 0 0 1 3.1 5.2 2 2 0 0 1 5.1 3h3a2 2 0 0 1 2 1.7c.1 1 .4 1.9.7 2.8a2 2 0 0 1-.5 2.1L9 10.9a16 16 0 0 0 4.1 4.1l1.3-1.3a2 2 0 0 1 2.1-.5c.9.3 1.8.6 2.8.7a2 2 0 0 1 2.7 3Z"/></svg>
                            </button>
                            </>}
                        </header>
                        {activeChat?.type === "group" && groupMembersOpen && (
                            <div className="group-members" aria-label="Group participants">
                                {activeChat.members.map(member => <div key={member.id}><UserAvatar user={member} className="small-avatar" /><span>{getUserDisplayName(member)}{member.id === user.id ? " (you)" : ""}<small>@{member.username}</small></span></div>)}
                            </div>
                        )}
                        {displayedPin && (
                            <div className="pinned-message-toolbar">
                                <button className="pinned-message-strip" type="button" onClick={() => jumpToMessage(displayedPin.id)}>
                                    <span className="pinned-message-icon">📌</span>
                                    <span><strong>{chatPins.length} pinned message{chatPins.length === 1 ? "" : "s"}</strong><small>{displayedPin.content || (displayedPin.image_url ? "Photo" : "Video")}</small></span>
                                    <span aria-hidden="true">›</span>
                                </button>
                                <button className="pinned-list-toggle" type="button" aria-label="Show all pinned messages" title="All pinned messages" aria-haspopup="dialog" onClick={() => setPinnedListChatId(activeChat?.id ?? null)}>
                                    <svg width="22" height="22" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" aria-hidden="true"><path d="M8 6h13M8 12h13M8 18h13M3 6h1M3 12h1M3 18h1" /></svg>
                                    <span>{chatPins.length}</span>
                                </button>
                            </div>
                        )}
                        {pinnedListChatId && pinnedListChatId === activeChat?.id && (
                            <PinnedMessagesDialog messages={chatPins} onClose={() => setPinnedListChatId(null)} onSelect={jumpToMessage} />
                        )}

                        <div className="messages-viewport">
                        <div
                            className="messages"
                            ref={messagesContainerRef}
                            onScroll={(event) => {
                                const container = event.currentTarget;
                                keepLatestVisibleRef.current =
                                    container.scrollHeight - container.clientHeight -
                                    container.scrollTop < 48;
                                setShowScrollToLatest(!keepLatestVisibleRef.current);
                                updatePinnedAnchor();
                            }}
                        >
                            <div ref={messagesContentRef}>
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

                                    const imageUrl = getMediaUrl(
                                        message.image_url,
                                    );

                                    const videoUrl = getMediaUrl(
                                        message.video_url,
                                    );

                                    if (message.deleted_at !== null) {
                                        return null;
                                    }

                                    const peerLastReadAt =
                                        activeChat?.peer_last_read_at ?? null;

                                    const isRead =
                                        isOwnMessage &&
                                        peerLastReadAt !== null &&
                                        new Date(
                                            message.created_at,
                                        ).getTime() <=
                                            new Date(
                                                peerLastReadAt,
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

                                            <div
                                                id={`message-${message.id}`}
                                                tabIndex={-1}
                                                className={`message-row ${isOwnMessage ? "own" : ""} ${replyTarget?.id === message.id ? "reply-target" : ""}`}
                                                onContextMenu={(event) =>
                                                    openMessageContextMenu(
                                                        event,
                                                        message,
                                                    )
                                                }
                                            >
                                                <div
                                                    className={`message-bubble ${message.is_call ? "call-message-bubble" : ""} ${
                                                        imageUrl || videoUrl
                                                            ? "media-message"
                                                            : ""
                                                    }`}
                                                >
                                                    {activeChat?.type === "group" && !isOwnMessage && <strong className="group-message-author">{senderName(message.sender_id)}</strong>}
                                                    {message.reply_to_message && (
                                                        <button
                                                            className="message-reply"
                                                            type="button"
                                                            aria-label="Go to original message"
                                                            onClick={() => void navigateToReply(message.reply_to_message!.id)}
                                                        >
                                                            <strong>
                                                                {message.reply_to_message.sender_id ===
                                                                user.id
                                                                    ? "You"
                                                                    : senderName(message.reply_to_message.sender_id)}
                                                            </strong>

                                                            <span>
                                                                {message.reply_to_message.content ||
                                                                    (message.reply_to_message.image_url
                                                                        ? "Photo"
                                                                        : message.reply_to_message.video_url
                                                                          ? "Video"
                                                                          : "Message")}
                                                            </span>
                                                        </button>
                                                    )}

                                                    {(imageUrl || videoUrl) && (
                                                        <div
                                                            className={`message-media ${
                                                                videoUrl ? "video" : "image"
                                                            }`}
                                                        >
                                                            {imageUrl && (
                                                                <button
                                                                    className="message-image-button"
                                                                    type="button"
                                                                    onClick={(event) => {
                                                                        event.stopPropagation();
                                                                        openImageViewer(imageUrl);
                                                                    }}
                                                                    aria-label="Open image"
                                                                >
                                                                    <img
                                                                        className="message-image"
                                                                        src={imageUrl}
                                                                        alt="Sent attachment"
                                                                        loading="lazy"
                                                                    />
                                                                </button>
                                                            )}

                                                            {videoUrl && (
                                                                <video
                                                                    className="message-video"
                                                                    src={videoUrl}
                                                                    preload="metadata"
                                                                    playsInline
                                                                    onClick={(event) => {
                                                                        event.stopPropagation();
                                                                        void openVideoFullscreen(
                                                                            event.currentTarget,
                                                                        );
                                                                    }}
                                                                    aria-label="Open video fullscreen"
                                                                />
                                                            )}

                                                            {message.content && (
                                                                <div className="message-caption">
                                                                    {message.content}
                                                                </div>
                                                            )}

                                                            <div className={`message-media-meta ${message.content ? "with-caption" : ""}`}>
                                                            {message.pinned_at && <span className="message-pinned-badge" title="Pinned" aria-label="Pinned message">📌</span>}
                                                                {message.edited_at && (
                                                                    <span>edited</span>
                                                                )}

                                                                <span>
                                                                    {formatTime(
                                                                        message.created_at,
                                                                    )}
                                                                </span>

                                                                {isOwnMessage && (
                                                                    <span
                                                                        className={`message-receipt ${
                                                                            isRead ? "read" : ""
                                                                        }`}
                                                                        title={isRead ? "Read" : "Sent"}
                                                                    >
                                                                        {isRead ? "✓✓" : "✓"}
                                                                    </span>
                                                                )}
                                                            </div>
                                                        </div>
                                                    )}

                                                    {message.content && !imageUrl && !videoUrl && (
                                                        message.is_call
                                                            ? <CallMessage content={message.content} outgoing={isOwnMessage} />
                                                            : <div className="message-content">{message.content}</div>
                                                    )}

                                                    {!imageUrl && !videoUrl && (
                                                        <div className="message-meta">
                                                            {message.pinned_at && <span className="message-pinned-badge" title="Pinned" aria-label="Pinned message">📌</span>}
                                                            {message.edited_at && (
                                                                <span className="message-edited">
                                                                    edited
                                                                </span>
                                                            )}

                                                            <span className="message-time">
                                                                {formatTime(
                                                                    message.created_at,
                                                                )}
                                                            </span>

                                                            {isOwnMessage && (
                                                                <span
                                                                    className={`message-receipt ${
                                                                        isRead ? "read" : ""
                                                                    }`}
                                                                    title={isRead ? "Read" : "Sent"}
                                                                >
                                                                    {isRead ? "✓✓" : "✓"}
                                                                </span>
                                                            )}
                                                        </div>
                                                    )}
                                                </div>
                                            </div>
                                        </Fragment>
                                    );
                                })
                            )}

                            </div>
                        </div>

                        {showScrollToLatest && !messagesLoading && messages.length > 0 && (
                            <button
                                className="scroll-to-latest"
                                type="button"
                                aria-label="Scroll to latest message"
                                title="Scroll to latest message"
                                onClick={() => {
                                    const container = messagesContainerRef.current;
                                    if (!container) return;
                                    container.scrollTo({
                                        top: container.scrollHeight,
                                        behavior: window.matchMedia("(prefers-reduced-motion: reduce)").matches
                                            ? "instant" : "smooth",
                                    });
                                }}
                            >
                                <svg width="24" height="24" viewBox="0 0 24 24" fill="none" aria-hidden="true">
                                    <path d="M12 5v14m-6-6 6 6 6-6" stroke="currentColor" strokeWidth="2" strokeLinecap="round" strokeLinejoin="round" />
                                </svg>
                            </button>
                        )}
                        </div>

                        <div className="message-composer">
                            {editingMessage && (
                                <div className="editing-preview">
                                    <div className="editing-preview-content">
                                        <strong>Editing message</strong>
                                        <span>{editingMessage.content}</span>
                                    </div>

                                    <button
                                        type="button"
                                        onClick={cancelEditing}
                                        aria-label="Cancel editing"
                                    >
                                        ×
                                    </button>
                                </div>
                            )}

                            {replyingTo && (
                                <div className="replying-preview">
                                    <div className="replying-preview-content">
                                        <strong>
                                            Replying to{" "}
                                            {replyingTo.sender_id === user.id
                                                ? "yourself"
                                                : senderName(replyingTo.sender_id)}
                                        </strong>

                                        <span>
                                            {replyingTo.content ||
                                                (replyingTo.image_url
                                                    ? "Photo"
                                                    : replyingTo.video_url
                                                      ? "Video"
                                                      : "Message")}
                                        </span>
                                    </div>

                                    <button
                                        type="button"
                                        onClick={() => setReplyingTo(null)}
                                        aria-label="Cancel reply"
                                    >
                                        ×
                                    </button>
                                </div>
                            )}

                            {selectedImage && selectedImagePreview && (
                                <div className="selected-image-preview">
                                    <img
                                        src={selectedImagePreview}
                                        alt="Selected"
                                    />

                                    <div className="selected-image-info">
                                        <strong>{selectedImage.name}</strong>
                                        <span>
                                            {(selectedImage.size / 1024 / 1024).toFixed(1)} MB
                                        </span>
                                    </div>

                                    <button
                                        type="button"
                                        onClick={clearSelectedImage}
                                        aria-label="Remove selected image"
                                    >
                                        ×
                                    </button>
                                </div>
                            )}

                            {selectedVideo && selectedVideoPreview && (
                                <div className="selected-image-preview selected-video-preview">
                                    <div className="selected-video-thumbnail">
                                        <video
                                            src={selectedVideoPreview}
                                            preload="metadata"
                                            muted
                                            playsInline
                                        />
                                        <span aria-hidden="true">▶</span>
                                    </div>

                                    <div className="selected-image-info">
                                        <strong>{selectedVideo.name}</strong>
                                        <span>
                                            {(selectedVideo.size / 1024 / 1024).toFixed(1)} MB · Video
                                        </span>
                                    </div>

                                    <button
                                        type="button"
                                        onClick={clearSelectedVideo}
                                        aria-label="Remove selected video"
                                    >
                                        ×
                                    </button>
                                </div>
                            )}

                            <form
                                className="message-form"
                                onSubmit={handleSendMessage}
                                onPaste={handleImagePaste}
                            >
                                {!editingMessage && (
                                    <label
                                        className="attachment-button"
                                        title="Attach photo or video"
                                    >
                                        📎
                                        <input
                                            ref={imageInputRef}
                                            type="file"
                                            accept="image/jpeg,image/png,image/webp,video/mp4,video/webm"
                                            onChange={handleAttachmentSelect}
                                            disabled={sending}
                                            hidden
                                        />
                                    </label>
                                )}

                                <input
                                    ref={messageInputRef}
                                    type="text"
                                    placeholder={
                                        editingMessage
                                            ? "Edit message..."
                                            : selectedImage || selectedVideo
                                              ? "Add a caption..."
                                              : replyingTo
                                                ? "Write a reply..."
                                                : "Write a message..."
                                    }
                                    value={messageInput}
                                    onChange={handleMessageInputChange}
                                    maxLength={4000}
                                    autoComplete="off"
                                />

                                <button
                                    type="submit"
                                    disabled={
                                        editingMessage
                                            ? messageActionLoading === editingMessage.id ||
                                              !messageInput.trim()
                                            : sending ||
                                              (!messageInput.trim() &&
                                                  !selectedImage &&
                                                  !selectedVideo)
                                    }
                                >
                                    ➤
                                </button>
                            </form>
                        </div>

                        {contextMenu && (
                            <div
                                className="message-context-menu"
                                style={{
                                    left: contextMenu.x,
                                    top: contextMenu.y,
                                }}
                                onClick={(event) => event.stopPropagation()}
                            >
                                <button
                                    type="button"
                                    onClick={() => startReply(contextMenu.message)}
                                >
                                    <span className="message-context-icon">↩</span>
                                    <span>Reply</span>
                                </button>

                                <button type="button" onClick={() => void togglePinnedMessage(contextMenu.message)}>
                                    <span className="message-context-icon">{contextMenu.message.pinned_at ? "−" : "📌"}</span>
                                    <span>{contextMenu.message.pinned_at ? "Unpin" : "Pin message"}</span>
                                </button>

                                {contextMenu.message.sender_id === user.id &&
                                    contextMenu.message.content && (
                                        <button
                                            type="button"
                                            onClick={() =>
                                                startEditing(contextMenu.message)
                                            }
                                        >
                                            <span className="message-context-icon">✎</span>
                                            <span>Edit</span>
                                        </button>
                                    )}

                                {contextMenu.message.sender_id === user.id && (
                                    <button
                                        className="danger"
                                        type="button"
                                        onClick={() =>
                                            requestDeleteMessage(
                                                contextMenu.message,
                                            )
                                        }
                                    >
                                        <span className="message-context-icon">×</span>
                                        <span>Delete</span>
                                    </button>
                                )}
                            </div>
                        )}

                        {deleteTarget && (
                            <div
                                className="message-delete-backdrop"
                                onMouseDown={() => {
                                    if (
                                        messageActionLoading !==
                                        deleteTarget.id
                                    ) {
                                        setDeleteTarget(null);
                                    }
                                }}
                            >
                                <div
                                    className="message-delete-modal"
                                    onMouseDown={(event) =>
                                        event.stopPropagation()
                                    }
                                >
                                    <h3>Delete message?</h3>
                                    <p>
                                        This message will be deleted for
                                        everyone.
                                    </p>

                                    <div className="message-delete-actions">
                                        <button
                                            type="button"
                                            onClick={() => setDeleteTarget(null)}
                                            disabled={
                                                messageActionLoading ===
                                                deleteTarget.id
                                            }
                                        >
                                            Cancel
                                        </button>

                                        <button
                                            className="danger"
                                            type="button"
                                            onClick={() =>
                                                void confirmDeleteMessage()
                                            }
                                            disabled={
                                                messageActionLoading ===
                                                deleteTarget.id
                                            }
                                        >
                                            {messageActionLoading ===
                                            deleteTarget.id
                                                ? "Deleting..."
                                                : "Delete"}
                                        </button>
                                    </div>
                                </div>
                            </div>
                        )}
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
                    <section className="settings-window">
                        <header className="settings-header">
                            <div>
                                <h2>Settings</h2>
                                <span>@{user.username}</span>
                            </div>

                            <button className="settings-close-button" type="button" onClick={closeProfileSettings} aria-label="Close settings">
                                ×
                            </button>
                        </header>

                        <div className="settings-layout">
                            <nav className="settings-sidebar" aria-label="Settings">
                                <button className={`settings-tab ${settingsTab === "profile" ? "active" : ""}`} type="button" onClick={() => openSettingsTab("profile")}>
                                    <span className="settings-tab-icon">👤</span>
                                    <span>Profile</span>
                                </button>

                                <button className={`settings-tab ${settingsTab === "security" ? "active" : ""}`} type="button" onClick={() => openSettingsTab("security")}>
                                    <span className="settings-tab-icon">🔒</span>
                                    <span>Security</span>
                                </button>

                                <button className={`settings-tab ${settingsTab === "sessions" ? "active" : ""}`} type="button" onClick={() => openSettingsTab("sessions")}>
                                    <span className="settings-tab-icon">💻</span>
                                    <span>Active sessions</span>
                                </button>

                                <button className={`settings-tab ${settingsTab === "privacy" ? "active" : ""}`} type="button" onClick={() => openSettingsTab("privacy")}>
                                    <span className="settings-tab-icon">🛡</span>
                                    <span>Privacy</span>
                                </button>
                            </nav>

                            <div className="settings-content">
                                {settingsTab === "profile" && (
                                    <section className="settings-page">
                                        <div className="settings-page-header">
                                            <div>
                                                <h3>Profile</h3>
                                                <p>Manage how your profile appears to other users.</p>
                                            </div>
                                        </div>

                                        <div className="profile-avatar-section">
                                            <UserAvatar user={user} className="profile-avatar" />

                                            <div className="profile-avatar-actions">
                                                <label className="avatar-upload-button">
                                                    {avatarUploading ? "Uploading..." : "Change photo"}
                                                    <input type="file" accept="image/jpeg,image/png,image/webp" onChange={handleAvatarChange} disabled={avatarUploading} hidden />
                                                </label>

                                                {user.avatar_url && (
                                                    <button className="remove-avatar-button" type="button" onClick={() => void handleRemoveAvatar()} disabled={avatarUploading}>
                                                        Remove photo
                                                    </button>
                                                )}
                                            </div>
                                        </div>

                                        <form className="profile-form settings-form" onSubmit={handleProfileSave}>
                                            <label htmlFor="profile-display-name">Display name</label>
                                            <input id="profile-display-name" type="text" value={profileDisplayName} onChange={(event) => setProfileDisplayName(event.target.value)} maxLength={64} placeholder="Your name" autoComplete="name" />

                                            <label htmlFor="profile-username">Username</label>
                                            <div className="username-input">
                                                <span>@</span>
                                                <input id="profile-username" type="text" value={profileUsername} onChange={(event) => setProfileUsername(event.target.value)} minLength={3} maxLength={32} autoComplete="username" required />
                                            </div>

                                            <label htmlFor="profile-bio">Bio</label>
                                            <textarea id="profile-bio" value={profileBio} onChange={(event) => setProfileBio(event.target.value)} maxLength={160} placeholder="Tell something about yourself" rows={4} />

                                            <div className="bio-counter">{profileBio.length}/160</div>

                                            {error && <div className="error-message">{error}</div>}
                                            {profileMessage && <div className="profile-success">{profileMessage}</div>}

                                            <button className="primary-button" type="submit" disabled={profileSaving || !profileUsername.trim()}>
                                                {profileSaving ? "Saving..." : "Save changes"}
                                            </button>
                                        </form>
                                    </section>
                                )}

                                {settingsTab === "security" && (
                                    <section className="settings-page">
                                        <div className="settings-page-header">
                                            <div>
                                                <h3>Security</h3>
                                                <p>Manage your password and account security.</p>
                                            </div>
                                        </div>

                                        <div className="settings-card">
                                            <div className="settings-card-heading">
                                                <h4>Change password</h4>
                                                <p>Changing your password will sign you out on every device.</p>
                                            </div>

                                            <form className="profile-form settings-form password-form" onSubmit={handleChangePassword}>
                                                <label htmlFor="current-password">Current password</label>
                                                <input id="current-password" type="password" value={currentPassword} onChange={(event) => setCurrentPassword(event.target.value)} minLength={8} maxLength={128} autoComplete="current-password" required />

                                                <label htmlFor="profile-new-password">New password</label>
                                                <input id="profile-new-password" type="password" value={profileNewPassword} onChange={(event) => setProfileNewPassword(event.target.value)} minLength={8} maxLength={128} autoComplete="new-password" required />

                                                <label htmlFor="profile-confirm-password">Confirm new password</label>
                                                <input id="profile-confirm-password" type="password" value={profileConfirmPassword} onChange={(event) => setProfileConfirmPassword(event.target.value)} minLength={8} maxLength={128} autoComplete="new-password" required />

                                                {error && <div className="error-message">{error}</div>}

                                                <button className="primary-button" type="submit" disabled={passwordChanging || !currentPassword || !profileNewPassword || !profileConfirmPassword}>
                                                    {passwordChanging ? "Changing password..." : "Change password"}
                                                </button>
                                            </form>
                                        </div>

                                        <div className="settings-card future-security-card">
                                            <div className="settings-card-heading">
                                                <h4>Two-factor authentication</h4>
                                                <p>Add another layer of protection to your account.</p>
                                            </div>

                                            <span className="settings-coming-soon">Coming later</span>
                                        </div>
                                    </section>
                                )}

                                {settingsTab === "sessions" && (
                                    <section className="settings-page">
                                        <div className="settings-page-header sessions-page-header">
                                            <div>
                                                <h3>Active sessions</h3>
                                                <p>Manage devices currently signed in to your account.</p>
                                            </div>

                                            <button className="refresh-sessions-button" type="button" onClick={() => void loadSessions()} disabled={sessionsLoading}>
                                                {sessionsLoading ? "Loading..." : "Refresh"}
                                            </button>
                                        </div>

                                        {error && <div className="error-message settings-message">{error}</div>}
                                        {profileMessage && <div className="profile-success settings-message">{profileMessage}</div>}

                                        {sessionsLoading && sessions.length === 0 ? (
                                            <div className="sessions-empty">Loading sessions...</div>
                                        ) : sessions.length === 0 ? (
                                            <div className="sessions-empty">No active sessions</div>
                                        ) : (
                                            <div className="sessions-list">
                                                {sessions.map((session) => (
                                                    <div className={`session-item ${session.current ? "current" : ""}`} key={session.id}>
                                                        <div className="session-device-icon">
                                                            {session.current ? "●" : "○"}
                                                        </div>

                                                        <div className="session-info">
                                                            <div className="session-title">
                                                                {getSessionDeviceName(session.user_agent)}

                                                                {session.current && (
                                                                    <span className="current-session-badge">This device</span>
                                                                )}
                                                            </div>

                                                            <div className="session-details">
                                                                Last active: {session.current ? "Now" : formatSessionDate(session.last_used_at ?? session.created_at)}
                                                            </div>

                                                            <div className="session-details">
                                                                Signed in: {formatSessionDate(session.created_at)}
                                                            </div>

                                                            {session.remember_me && (
                                                                <div className="session-remembered">Remember me enabled</div>
                                                            )}
                                                        </div>

                                                        {!session.current && (
                                                            <button className="session-logout-button" type="button" disabled={sessionActionId === session.id} onClick={() => void handleRevokeSession(session.id)}>
                                                                {sessionActionId === session.id ? "Logging out..." : "Log out"}
                                                            </button>
                                                        )}
                                                    </div>
                                                ))}
                                            </div>
                                        )}

                                        {sessions.some((session) => !session.current) && (
                                            <button className="logout-others-button" type="button" onClick={() => void handleLogoutOtherSessions()} disabled={loggingOutOthers}>
                                                {loggingOutOthers ? "Logging out..." : "Log out all other sessions"}
                                            </button>
                                        )}
                                    </section>
                                )}

                                {settingsTab === "privacy" && (
                                    <section className="settings-page">
                                        <div className="settings-page-header">
                                            <div>
                                                <h3>Privacy</h3>
                                                <p>Control what other users can see and how they can interact with you.</p>
                                            </div>
                                        </div>

                                        <div className="privacy-options">
                                            <div className="privacy-option disabled">
                                                <div>
                                                    <strong>Last seen</strong>
                                                    <span>Choose who can see when you were last online.</span>
                                                </div>
                                                <span className="settings-coming-soon">Coming later</span>
                                            </div>

                                            <div className="privacy-option disabled">
                                                <div>
                                                    <strong>Online status</strong>
                                                    <span>Control who can see when you are online.</span>
                                                </div>
                                                <span className="settings-coming-soon">Coming later</span>
                                            </div>

                                            <div className="privacy-option disabled">
                                                <div>
                                                    <strong>Read receipts</strong>
                                                    <span>Choose whether other users can see when you read their messages.</span>
                                                </div>
                                                <span className="settings-coming-soon">Coming later</span>
                                            </div>

                                            <div className="privacy-option disabled">
                                                <div>
                                                    <strong>Profile photo</strong>
                                                    <span>Choose who can see your profile picture.</span>
                                                </div>
                                                <span className="settings-coming-soon">Coming later</span>
                                            </div>

                                            <div className="privacy-option disabled">
                                                <div>
                                                    <strong>Blocked users</strong>
                                                    <span>Manage people you do not want to receive messages from.</span>
                                                </div>
                                                <span className="settings-coming-soon">Coming later</span>
                                            </div>
                                        </div>
                                    </section>
                                )}
                            </div>
                        </div>
                    </section>
                </div>
            )}

            {peerProfileOpen && currentPeer && (
                <PeerProfileDialog
                    user={currentPeer}
                    onClose={() => setPeerProfileOpen(false)}
                />
            )}

            {imageViewerUrl && (() => {
                const imageUrls =
                    getChatImageUrls();

                const currentImageIndex =
                    imageUrls.indexOf(
                        imageViewerUrl,
                    );

                if (currentImageIndex === -1) {
                    return null;
                }

                return (
                    <div
                        className="image-viewer"
                        onClick={
                            closeImageViewer
                        }
                        onWheel={
                            handleImageViewerWheel
                        }
                    >
                        <button
                            className="image-viewer-close"
                            type="button"
                            onClick={
                                closeImageViewer
                            }
                            aria-label="Close image"
                        >
                            ×
                        </button>

                        {imageUrls.length > 1 && (
                            <>
                                <button
                                    className="image-viewer-navigation previous"
                                    type="button"
                                    onClick={(event) => {
                                        event.stopPropagation();
                                        changeViewerImage(-1);
                                    }}
                                    aria-label="Previous image"
                                >
                                    ‹
                                </button>

                                <button
                                    className="image-viewer-navigation next"
                                    type="button"
                                    onClick={(event) => {
                                        event.stopPropagation();
                                        changeViewerImage(1);
                                    }}
                                    aria-label="Next image"
                                >
                                    ›
                                </button>
                            </>
                        )}

                        <div
                            className="image-viewer-stage"
                            onClick={(event) =>
                                event.stopPropagation()
                            }
                        >
                            <img
                                ref={imageViewerImageRef}
                                src={imageViewerUrl}
                                alt="Full size attachment"
                                draggable={false}
                                className={imageViewerDragging
                                    ? "dragging"
                                    : imageViewerScale > 1
                                      ? "zoomed"
                                      : ""}
                                style={{
                                    transform: `translate(${imageViewerPosition.x}px, ${imageViewerPosition.y}px) scale(${imageViewerScale})`,
                                }}
                                onDoubleClick={
                                    handleImageViewerDoubleClick
                                }
                                onPointerDown={
                                    handleImagePointerDown
                                }
                                onPointerMove={
                                    handleImagePointerMove
                                }
                                onPointerUp={
                                    handleImagePointerUp
                                }
                                onPointerCancel={
                                    handleImagePointerUp
                                }
                                onLostPointerCapture={() =>
                                    setImageViewerDragging(false)
                                }
                            />
                        </div>

                        <div className="image-viewer-info">
                            {currentImageIndex + 1} /{" "}
                            {imageUrls.length}

                            {imageViewerScale > 1 && (
                                <span>
                                    {" "}
                                    ·{" "}
                                    {Math.round(
                                        imageViewerScale *
                                            100,
                                    )}
                                    %
                                </span>
                            )}
                        </div>
                    </div>
                );
            })()}
        </main>
    );
}


export default App;
