import { useEffect, useRef, useState } from "react";
import { getMediaUrl, type User } from "./api";
import { avatarInitials, avatarStyle } from "./avatar";
import { Icon } from "./Icon";

export function PeerProfileDialog({ user, blocked, busy, onBlockChange, onClose }: {
    user: User;
    blocked?: boolean;
    busy?: boolean;
    /** Omit to hide the block controls, e.g. in group member lists. */
    onBlockChange?: (block: boolean) => Promise<void>;
    onClose: () => void;
}) {
    const dialogRef = useRef<HTMLDialogElement | null>(null);
    const [confirmingBlock, setConfirmingBlock] = useState(false);
    const name = user.display_name?.trim() || user.username;

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
            onCancel={event => { event.stopPropagation(); onClose(); }}
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
                ><Icon name="close" /></button>
                <div className="avatar peer-profile-avatar" style={avatarStyle(user.id)}>{getMediaUrl(user.avatar_url) ? <img src={getMediaUrl(user.avatar_url)!} alt="" /> : <span className="avatar-initials">{avatarInitials(user.display_name?.trim() || user.username)}</span>}</div>
                <h2 id="peer-profile-title">{user.display_name?.trim() || user.username}</h2>
                <dl className="peer-profile-details">
                    <dt>Username</dt>
                    <dd>@{user.username}</dd>
                    <dt>Bio</dt>
                    <dd>{user.bio?.trim() || "No bio yet"}</dd>
                </dl>
                {onBlockChange && (confirmingBlock ? (
                    <div className="peer-block-confirm" role="region" aria-label="Confirm block">
                        <p>Block {name}? They won't be able to message or call you, and you won't see each other's online status.</p>
                        <div>
                            <button type="button" onClick={() => setConfirmingBlock(false)} disabled={busy}>Cancel</button>
                            <button
                                type="button"
                                className="danger"
                                disabled={busy}
                                onClick={() => void onBlockChange(true).then(() => setConfirmingBlock(false))}
                            >
                                {busy ? "Blocking..." : "Block"}
                            </button>
                        </div>
                    </div>
                ) : (
                    <button
                        type="button"
                        className={`peer-block-button${blocked ? "" : " danger"}`}
                        disabled={busy}
                        onClick={() => (blocked ? void onBlockChange(false) : setConfirmingBlock(true))}
                    >
                        {blocked ? (busy ? "Unblocking..." : "Unblock user") : "Block user"}
                    </button>
                ))}
            </div>
        </dialog>
    );
}

