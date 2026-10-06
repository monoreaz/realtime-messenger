import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { createGroupChat, searchUsers } from "./api";
import type { Chat, User } from "./api";

export function GroupDialog({ token, currentUserId, chats, onClose, onCreated }: {
    token: string; currentUserId: string; chats: Chat[]; onClose: () => void; onCreated: (chat: Chat) => void;
}) {
    const dialogRef = useRef<HTMLDialogElement>(null);
    const [name, setName] = useState("");
    const [username, setUsername] = useState("");
    const [description, setDescription] = useState("");
    const [query, setQuery] = useState("");
    const [results, setResults] = useState<User[]>([]);
    const [selected, setSelected] = useState<User[]>([]);
    const [searching, setSearching] = useState(false);
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState("");
    useEffect(() => {
        const dialog = dialogRef.current;
        dialog?.showModal();
        return () => dialog?.close();
    }, []);
    useEffect(() => {
        let cancelled = false;
        setResults([]);
        setSearching(Boolean(query.trim()));
        if (!query.trim()) return;
        const timeout = window.setTimeout(() => {
            void searchUsers(token, query.trim()).then(users => {
                if (!cancelled) setResults(users.filter(user => user.id !== currentUserId));
            }).catch(cause => {
                if (!cancelled) setError(cause instanceof Error ? cause.message : "Search failed");
            }).finally(() => { if (!cancelled) setSearching(false); });
        }, 250);
        return () => { cancelled = true; window.clearTimeout(timeout); };
    }, [query, token, currentUserId]);

    const existingContacts = Array.from(new Map(
        chats.filter(chat => chat.type === "private" && chat.peer && chat.peer.id !== currentUserId)
            .map(chat => [chat.peer!.id, chat.peer!] as const),
    ).values());
    const visibleUsers = query.trim() ? results : existingContacts;

    async function submit(event: FormEvent) {
        event.preventDefault();
        if (saving || !name.trim() || selected.length < 2) return;
        setSaving(true);
        setError("");
        try {
            onCreated(await createGroupChat(token, name.trim(), selected.map(user => user.id), username.trim(), description.trim()));
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : "Could not create group");
            setSaving(false);
        }
    }

    return <dialog ref={dialogRef} className="group-dialog" aria-labelledby="group-title" onCancel={event => { if (saving) event.preventDefault(); else onClose(); }}>
        <form onSubmit={submit}>
            <header><h2 id="group-title">New group</h2><button type="button" onClick={onClose} disabled={saving} aria-label="Close">×</button></header>
            <label>Group name<input autoFocus value={name} onChange={event => setName(event.target.value)} maxLength={100} required disabled={saving} placeholder="Friends, family, team…" /></label>
            <label>Group username <small>Optional · 3–32 letters, numbers or underscores</small><input value={username} onChange={event => setUsername(event.target.value.replace(/^@/, "").toLowerCase())} minLength={3} maxLength={32} pattern="[a-z0-9_]{3,32}" disabled={saving} placeholder="group_username" /></label>
            <label>Description <small>Optional</small><textarea value={description} onChange={event => setDescription(event.target.value)} maxLength={500} disabled={saving} rows={3} placeholder="What is this group about?" /></label>
            <label>Add participants<input value={query} onChange={event => setQuery(event.target.value)} disabled={saving} placeholder="Search by username" /></label>
            <div className="group-search-results" aria-live="polite">
                {!query.trim() && <p className="group-contacts-title">Your chats</p>}
                {!query.trim() && !existingContacts.length && <p>Search by username to add participants.</p>}
                {searching ? <p>Searching…</p> : query.trim() && !results.length ? <p>No users found.</p> : visibleUsers.map(user => {
                    const checked = selected.some(item => item.id === user.id);
                    return <label key={user.id}><input type="checkbox" checked={checked} disabled={saving || (!checked && selected.length >= 99)} onChange={() => setSelected(items => checked ? items.filter(item => item.id !== user.id) : [...items, user])} /><span>{user.display_name || user.username}<small>@{user.username}</small></span></label>;
                })}
            </div>
            <p>Select at least two people. You will be added automatically.</p>
            <div className="group-selected">{selected.map(user => <button type="button" key={user.id} disabled={saving} onClick={() => setSelected(items => items.filter(item => item.id !== user.id))} aria-label={`Remove ${user.username}`}>{user.display_name || user.username} ×</button>)}</div>
            {error && <p role="alert" className="group-error">{error}</p>}
            <button className="group-submit" type="submit" disabled={saving || !name.trim() || selected.length < 2}>{saving ? "Creating…" : `Create group · ${selected.length + 1} participants`}</button>
        </form>
    </dialog>;
}
