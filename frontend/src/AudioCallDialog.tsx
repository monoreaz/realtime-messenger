import { useEffect, useRef } from "react";
import { avatarInitials } from "./avatar";
import { Icon } from "./Icon";

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
            <div className={`audio-call-avatar ${phase === "connected" ? "is-connected" : ""}`} aria-hidden="true"><span className="avatar-initials">{avatarInitials(name)}</span></div>
            <h2 id="audio-call-name">{name}</h2>
            <p className="audio-call-status" aria-live="polite"><span />{status}</p>
            <div className="audio-call-duration" role="timer" aria-label="Call duration">{phase === "connected" ? duration : "—:—"}</div>
            <p className="audio-call-hint">{phase === "incoming" ? "Answer to start your conversation" : muted ? "Your microphone is muted" : phase === "connected" ? "Microphone is on" : "Waiting for the connection"}</p>
            {error && <div className="audio-call-error" role="status">{error}<button type="button" onClick={onPlayAudio}>Enable audio</button></div>}
            <div className="audio-call-controls">
                {phase === "incoming" ? <button type="button" className="audio-call-control accept" onClick={onAccept}><span aria-hidden="true"><Icon name="phone" /></span>Answer</button> :
                    <button type="button" className={`audio-call-control ${muted ? "is-muted" : ""}`} aria-pressed={muted} disabled={phase !== "connected"} onClick={onToggleMute}>
                        <span aria-hidden="true"><Icon name={muted ? "micOff" : "mic"} /></span>{muted ? "Unmute" : "Mute"}
                    </button>}
                <button type="button" className="audio-call-control end" onClick={onEnd}><span aria-hidden="true"><Icon name="phone" className="hang-up" /></span>{phase === "incoming" ? "Decline" : "End call"}</button>
            </div>
        </dialog>
    );
}
