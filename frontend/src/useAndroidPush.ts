import { useEffect, useRef } from "react";
import { registerPushDevice } from "./api";

interface PushBridge {
    postMessage(message: string): void;
    onmessage: ((event: MessageEvent<string>) => void) | null;
}
declare global { interface Window { MessengerPush?: PushBridge; } }

export function clearAndroidPush() {
    window.MessengerPush?.postMessage(JSON.stringify({ userId: "", chatId: "" }));
}

export function useAndroidPush(
    token: string | null, userId: string | undefined, sessionChecked: boolean,
    chatId: string | undefined, openChat: (id: string) => Promise<void>,
) {
    const openChatRef = useRef(openChat);
    const registered = useRef("");
    useEffect(() => { openChatRef.current = openChat; });
    useEffect(() => {
        const bridge = window.MessengerPush;
        if (!bridge || !sessionChecked) return;
        if (!token) {
            registered.current = "";
            clearAndroidPush();
            return;
        }
        if (!userId) return;
        let disposed = false;
        let registering = false;
        bridge.onmessage = (event) => {
            if (disposed) return;
            let data: { userId?: string; token?: string; binding?: string; openChatId?: string };
            try { data = JSON.parse(event.data); } catch { return; }
            if (data.userId !== userId) return;
            if (data.openChatId) void openChatRef.current(data.openChatId);
            if (!data.token || !data.binding) return;
            const key = `${userId}:${data.token}:${data.binding}:${token}`;
            if (registered.current === key || registering) return;
            registering = true;
            void registerPushDevice(token, data.token, data.binding).then(() => {
                if (!disposed) registered.current = key;
            }).catch(() => {
                // Resume, session refresh and the timer retry transient failures.
            }).finally(() => { registering = false; });
        };
        const sync = () => bridge.postMessage(JSON.stringify({ userId, chatId: chatId ?? "" }));
        sync();
        const timer = window.setInterval(sync, 60_000);
        window.addEventListener("messenger-push-sync", sync);
        window.addEventListener("online", sync);
        return () => {
            disposed = true;
            bridge.onmessage = null;
            window.clearInterval(timer);
            window.removeEventListener("messenger-push-sync", sync);
            window.removeEventListener("online", sync);
        };
    }, [token, userId, sessionChecked, chatId]);
}
