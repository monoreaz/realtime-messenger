import { useEffect, useRef, useState } from "react";

type Call = { id: string; peerId: string; name: string; startedAt?: number; phase: "incoming" | "dialing" | "connecting" | "connected" };
type Signal = { type: string; call_id: string; user_id: string; name?: string; sdp?: RTCSessionDescriptionInit; candidate?: RTCIceCandidateInit; reason?: string; started_at?: string; duration_seconds?: number | null };

export function useAudioCall(socket: React.RefObject<WebSocket | null>, enabled: boolean) {
    const [call, setCall] = useState<Call | null>(null);
    const [error, setError] = useState("");
    const [elapsedSeconds, setElapsedSeconds] = useState(0);
    const [muted, setMuted] = useState(false);
    const current = useRef<Call | null>(null);
    const pc = useRef<RTCPeerConnection | null>(null);
    const stream = useRef<MediaStream | null>(null);
    const audio = useRef<HTMLAudioElement | null>(null);
    const candidates = useRef<RTCIceCandidateInit[]>([]);
    const timer = useRef<ReturnType<typeof setTimeout> | null>(null);

    function send(data: object) {
        if (socket.current?.readyState !== WebSocket.OPEN) throw new Error("No connection to the server");
        socket.current.send(JSON.stringify(data));
    }
    function isCurrent(id: string) { return current.current?.id === id; }
    function update(value: Call | null) { current.current = value; setCall(value); }
    function end(notify = true, reason?: string) {
        const old = current.current;
        if (notify && old && socket.current?.readyState === WebSocket.OPEN) send({ type: "call.end", call_id: old.id, peer_id: old.peerId, reason });
        update(null);
        if (timer.current) clearTimeout(timer.current);
        pc.current?.close(); pc.current = null;
        stream.current?.getTracks().forEach(track => track.stop()); stream.current = null;
        if (audio.current) { audio.current.pause(); audio.current.srcObject = null; audio.current = null; }
        candidates.current = [];
        setMuted(false);
        setElapsedSeconds(0);
    }
    function fail(reason: unknown) { setError(reason instanceof Error ? reason.message : "Could not connect the call"); end(true, "failed"); }
    function begin(value: Call) {
        setError(""); update(value);
        timer.current = setTimeout(() => { setError("No answer or connection unavailable"); end(true, "timeout"); }, 60000);
    }
    async function prepare(value: Call) {
        if (!navigator.mediaDevices?.getUserMedia) throw new Error("Open the site over HTTPS and allow microphone access to make a call");
        const media = await navigator.mediaDevices.getUserMedia({ audio: true, video: false });
        if (!isCurrent(value.id)) { media.getTracks().forEach(track => track.stop()); return null; }
        stream.current = media;
        const iceServers: RTCIceServer[] = [{ urls: "stun:stun.l.google.com:19302" }];
        const turnUrl = import.meta.env.VITE_TURN_URL;
        if (turnUrl) iceServers.push({ urls: turnUrl, username: import.meta.env.VITE_TURN_USERNAME, credential: import.meta.env.VITE_TURN_CREDENTIAL });
        const connection = new RTCPeerConnection({ iceServers }); pc.current = connection;
        media.getTracks().forEach(track => connection.addTrack(track, media));
        connection.onicecandidate = event => {
            if (event.candidate && isCurrent(value.id)) {
                try { send({ type: "call.ice", call_id: value.id, peer_id: value.peerId, candidate: event.candidate.toJSON() }); } catch (e) { fail(e); }
            }
        };
        connection.ontrack = event => {
            const player = audio.current ?? new Audio(); audio.current = player;
            player.srcObject = event.streams[0] ?? new MediaStream([event.track]);
            void player.play().catch(() => setError("Click Enable audio to hear the other person"));
        };
        connection.onconnectionstatechange = () => {
            if (!isCurrent(value.id)) return;
            if (connection.connectionState === "connected") {
                if (timer.current) clearTimeout(timer.current);
                update({ ...value, phase: "connected", startedAt: current.current?.startedAt ?? Date.now() });
                try { send({ type: "call.connected", call_id: value.id, peer_id: value.peerId }); } catch (e) { fail(e); }
            } else if (connection.connectionState === "failed") fail(new Error("Connection lost. Please try calling again"));
        };
        return connection;
    }
    async function start(peerId: string, name: string) {
        if (current.current) return;
        const value: Call = { id: crypto.randomUUID(), peerId, name, phase: "dialing" };
        begin(value);
        try {
            const connection = await prepare(value); if (!connection) return;
            await connection.setLocalDescription(await connection.createOffer());
            if (!isCurrent(value.id)) return;
            send({ type: "call.offer", call_id: value.id, peer_id: peerId, sdp: connection.localDescription });
        } catch (e) { if (isCurrent(value.id)) fail(e); }
    }
    const offer = useRef<RTCSessionDescriptionInit | null>(null);
    async function accept() {
        const value = current.current;
        if (!value || value.phase !== "incoming" || !offer.current) return;
        update({ ...value, phase: "connecting" });
        try {
            // Unlock playback in the user's click gesture, before the remote track arrives.
            audio.current = new Audio(); audio.current.autoplay = true;
            const connection = await prepare(value); if (!connection) return;
            await connection.setRemoteDescription(offer.current);
            for (const candidate of candidates.current.splice(0)) await connection.addIceCandidate(candidate);
            await connection.setLocalDescription(await connection.createAnswer());
            if (!isCurrent(value.id)) return;
            send({ type: "call.answer", call_id: value.id, peer_id: value.peerId, sdp: connection.localDescription });
        } catch (e) { if (isCurrent(value.id)) fail(e); }
    }
    async function receive(data: Signal) {
        if (data.type === "call.offer") {
            if (current.current) {
                send({ type: "call.end", call_id: data.call_id, peer_id: data.user_id, reason: "busy" }); return;
            }
            offer.current = data.sdp ?? null;
            begin({ id: data.call_id, peerId: data.user_id, name: data.name ?? "Caller", phase: "incoming" }); return;
        }
        const value = current.current;
        if (!value || data.call_id !== value.id) return;
        if (data.type === "call.end") {
            end(false);
            setError(data.reason === "busy" ? "This person is busy" : data.reason === "offline" ? "This person is offline" : "");
            return;
        }
        if (data.type === "call.started" && data.started_at) {
            const startedAt = Date.parse(data.started_at);
            if (Number.isFinite(startedAt)) {
                if (timer.current) clearTimeout(timer.current);
                update({ ...value, phase: "connected", startedAt });
            }
            return;
        }
        if (data.user_id !== value.peerId) return;
        try {
            if (data.type === "call.answer" && data.sdp && pc.current?.signalingState === "have-local-offer") {
                await pc.current.setRemoteDescription(data.sdp);
                for (const candidate of candidates.current.splice(0)) await pc.current.addIceCandidate(candidate);
                if (current.current?.phase !== "connected") update({ ...value, phase: "connecting" });
            } else if (data.type === "call.ice" && data.candidate) {
                if (pc.current?.remoteDescription) await pc.current.addIceCandidate(data.candidate);
                else candidates.current.push(data.candidate);
            }
        } catch (e) { if (isCurrent(value.id)) fail(e); }
    }
    // Call lifetime follows the socket; callbacks use refs to the active resources.
    // eslint-disable-next-line react-hooks/exhaustive-deps
    useEffect(() => { if (!enabled) end(false); return () => end(); }, [enabled]);
    useEffect(() => {
        if (!call?.startedAt) return;
        const startedAt = call.startedAt;
        const tick = () => setElapsedSeconds(Math.max(0, Math.floor((Date.now() - startedAt) / 1000)));
        tick();
        const interval = window.setInterval(tick, 1000);
        return () => window.clearInterval(interval);
    }, [call?.startedAt]);
    return { call, error, muted, elapsedSeconds, start, accept, end, receive,
        dismissError: () => setError(""),
        play: () => { void audio.current?.play().then(() => setError("")).catch(() => setError("Could not enable audio")); },
        toggleMute: () => { stream.current?.getAudioTracks().forEach(track => { track.enabled = !track.enabled; }); setMuted(value => !value); },
    };
}
