import { useEffect, useRef } from "react";
import { getMediaUrl, type User } from "./api";
import { avatarInitials, avatarStyle } from "./avatar";
import { Icon } from "./Icon";

export function PeerProfileDialog({ user, onClose }: { user: User; onClose: () => void }) {
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
            </div>
        </dialog>
    );
}

