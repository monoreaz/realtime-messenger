import { useEffect, useRef } from "react";

type Props = {
    name: string;
    phase: "incoming" | "dialing" | "connecting" | "connected";
    elapsedSeconds: number;
    muted: boolean;
    error: string;
    onPlayAudio: () => void;
    onAccept: () => void;
    onEnd: () => void;
    onToggleMute: () => void;
};

export function AudioCallDialog({ name, phase, elapsedSeconds, muted, error, onPlayAudio, onAccept, onEnd, onToggleMute }: Props) {
    const dialogRef = useRef<HTMLDialogElement>(null);
    useEffect(() => {
        dialogRef.current?.showModal();
        const dialog = dialogRef.current;
        return () => dialog?.close();
    }, []);
    const hours = Math.floor(elapsedSeconds / 3600);
    const minutes = Math.floor(elapsedSeconds / 60) % 60;
    const seconds = elapsedSeconds % 60;
    const duration = `${hours ? `${hours}:` : ""}${String(minutes).padStart(2, "0")}:${String(seconds).padStart(2, "0")}`;
    const status = { incoming: "Incoming audio call", dialing: "Calling…", connecting: "Connecting…", connected: "Connected" }[phase];
    return (
        <dialog ref={dialogRef} className="audio-call-dialog" aria-labelledby="audio-call-name" onCancel={event => event.preventDefault()}>
            <div className="audio-call-kind">AUDIO CALL</div>
            <div className={`audio-call-avatar ${phase === "connected" ? "is-connected" : ""}`} aria-hidden="true">{name.slice(0, 1).toUpperCase()}</div>
            <h2 id="audio-call-name">{name}</h2>
            <p className="audio-call-status" aria-live="polite"><span />{status}</p>
            <div className="audio-call-duration" role="timer" aria-label="Call duration">{phase === "connected" ? duration : "—:—"}</div>
            <p className="audio-call-hint">{phase === "incoming" ? "Answer to start your conversation" : muted ? "Your microphone is muted" : phase === "connected" ? "Microphone is on" : "Waiting for the connection"}</p>
            {error && <div className="audio-call-error" role="status">{error}<button type="button" onClick={onPlayAudio}>Enable audio</button></div>}
            <div className="audio-call-controls">
                {phase === "incoming" ? <button type="button" className="audio-call-control accept" onClick={onAccept}><span aria-hidden="true">☎</span>Answer</button> :
                    <button type="button" className={`audio-call-control ${muted ? "is-muted" : ""}`} aria-pressed={muted} disabled={phase !== "connected"} onClick={onToggleMute}>
                        <span aria-hidden="true"><svg width="24" height="24" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8"><rect x="9" y="2" width="6" height="12" rx="3" /><path d="M5 10v2a7 7 0 0 0 14 0v-2M12 19v3M8 22h8" />{muted && <path d="m3 3 18 18" />}</svg></span>{muted ? "Unmute" : "Mute"}
                    </button>}
                <button type="button" className="audio-call-control end" onClick={onEnd}><span aria-hidden="true">☎</span>{phase === "incoming" ? "Decline" : "End call"}</button>
            </div>
        </dialog>
    );
}
