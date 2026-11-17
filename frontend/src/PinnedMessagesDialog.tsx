import { useEffect, useRef } from "react";
import type { Message } from "./api";
import { Icon } from "./Icon";

type Props = {
    messages: Message[];
    onClose: () => void;
    onSelect: (messageId: string) => void;
};

export function PinnedMessagesDialog({ messages, onClose, onSelect }: Props) {
    const dialogRef = useRef<HTMLDialogElement>(null);
    useEffect(() => {
        const dialog = dialogRef.current;
        dialog?.showModal();
        return () => dialog?.close();
    }, []);

    const ordered = [...messages].sort((a, b) =>
        Date.parse(b.created_at) - Date.parse(a.created_at) || b.id.localeCompare(a.id),
    );

    return (
        <dialog ref={dialogRef} className="pinned-list-dialog" aria-labelledby="pinned-list-title"
            onCancel={onClose}
            onClick={event => { if (event.target === event.currentTarget) onClose(); }}>
            <header className="pinned-list-header">
                <h2 id="pinned-list-title">Pinned messages <span className="dialog-count">{messages.length}</span></h2>
                <button className="icon-button" type="button" onClick={onClose} aria-label="Close pinned messages" autoFocus><Icon name="close" /></button>
            </header>
            {ordered.length === 0 ? <p className="pinned-list-empty">No pinned messages.</p> : (
                <ul className="pinned-list">
                    {ordered.map(message => (
                        <li key={message.id}>
                            <button type="button" className="pinned-list-item" onClick={() => {
                                dialogRef.current?.close();
                                onClose();
                                onSelect(message.id);
                            }}>
                                <span className="pinned-list-icon" aria-hidden="true"><Icon name="pin" /></span>
                                <span className="pinned-list-preview">
                                    <span>{message.content || (message.image_url ? "Photo" : "Video")}</span>
                                    <time dateTime={message.created_at}>{new Date(message.created_at).toLocaleString("en-US", { dateStyle: "medium", timeStyle: "short" })}</time>
                                </span>
                                <Icon name="chevronRight" className="pinned-list-arrow" />
                            </button>
                        </li>
                    ))}
                </ul>
            )}
        </dialog>
    );
}
