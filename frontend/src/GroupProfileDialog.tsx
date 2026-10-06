import { useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { getMediaUrl, updateGroupProfile } from "./api";
import type { Chat } from "./api";
import "./GroupProfileDialog.css";

export function GroupProfileDialog({ chat, token, currentUserId, onClose, onUpdated }: {
    chat: Chat; token: string; currentUserId: string; onClose: () => void; onUpdated: (chat: Chat) => void;
}) {
    const dialogRef = useRef<HTMLDialogElement>(null);
    const [editing, setEditing] = useState(false);
    const [name, setName] = useState(chat.name ?? "");
    const [username, setUsername] = useState(chat.username ?? "");
    const [description, setDescription] = useState(chat.description ?? "");
    const [saving, setSaving] = useState(false);
    const [error, setError] = useState("");
    const isAdmin = chat.members.some(member => member.id === currentUserId && member.group_role === "admin");
    const members = [...chat.members].sort((a, b) => Number(b.group_role === "admin") - Number(a.group_role === "admin"));

    useEffect(() => {
        const dialog = dialogRef.current;
        dialog?.showModal();
        return () => dialog?.close();
    }, []);

    function startEditing() {
        setName(chat.name ?? "");
        setUsername(chat.username ?? "");
        setDescription(chat.description ?? "");
        setError("");
        setEditing(true);
    }

    async function save(event: FormEvent) {
        event.preventDefault();
        if (!name.trim() || saving) return;
        setSaving(true);
        setError("");
        try {
            const updated = await updateGroupProfile(token, chat.id, {
                name: name.trim(), username: username.trim() || null, description: description.trim() || null,
            });
            onUpdated(updated);
            setEditing(false);
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : "Could not save group");
        } finally {
            setSaving(false);
        }
    }

    return <dialog ref={dialogRef} className="group-profile-dialog" aria-labelledby="group-profile-title" onCancel={event => {
        if (saving || editing) { event.preventDefault(); if (!saving) setEditing(false); }
        else onClose();
    }}>
        <header className="group-profile-header"><h2 id="group-profile-title">{editing ? "Edit group" : "Group info"}</h2><button type="button" onClick={onClose} disabled={saving} aria-label="Close group info">×</button></header>
        {editing ? <form className="group-profile-form" onSubmit={save}>
            <label>Group name<input autoFocus value={name} onChange={event => setName(event.target.value)} required maxLength={100} disabled={saving} /></label>
            <label>Group username<div className="group-username-input"><span aria-hidden="true">@</span><input aria-label="Group username" value={username} onChange={event => setUsername(event.target.value.replace(/^@/, "").toLowerCase())} minLength={3} maxLength={32} pattern="[a-z0-9_]{3,32}" placeholder="group_username" disabled={saving} /></div><small>3–32 letters, numbers or underscores. Optional.</small></label>
            <label>Description<textarea value={description} onChange={event => setDescription(event.target.value)} rows={4} maxLength={500} placeholder="What is this group about?" disabled={saving} /><small>{description.length}/500</small></label>
            {error && <p className="group-profile-error" role="alert">{error}</p>}
            <div className="group-profile-actions"><button type="button" disabled={saving} onClick={() => setEditing(false)}>Cancel</button><button className="group-profile-save" type="submit" disabled={saving || !name.trim()}>{saving ? "Saving…" : "Save changes"}</button></div>
        </form> : <>
            <section className="group-profile-hero"><div className="group-profile-avatar" aria-hidden="true">👥</div><h3>{chat.name}</h3><span>{chat.members.length} participants</span></section>
            <section className="group-profile-details" aria-label="Group details"><div><span>Username</span><strong className={chat.username ? "group-profile-username" : "group-profile-muted"}>{chat.username ? `@${chat.username}` : "Not set"}</strong></div><div><span>Description</span><p className={chat.description ? "" : "group-profile-muted"}>{chat.description || "No description yet."}</p></div>
                {isAdmin && <button type="button" className="group-profile-edit" onClick={startEditing}>Edit group info</button>}
            </section>
            <section className="group-profile-members" aria-labelledby="group-members-title"><h3 id="group-members-title">Participants <span>{chat.members.length}</span></h3><ul>{members.map(member => {
                const avatar = getMediaUrl(member.avatar_url);
                const displayName = member.display_name || member.username;
                return <li key={member.id}><div className="group-member-avatar">{avatar ? <img src={avatar} alt="" /> : displayName.slice(0, 1).toUpperCase()}</div><div className="group-member-name"><strong>{displayName}{member.id === currentUserId ? " (you)" : ""}</strong><small>@{member.username}</small></div>{member.group_role === "admin" && <span className="group-admin-badge">Admin</span>}</li>;
            })}</ul></section>
        </>}
    </dialog>;
}
