import { useCallback, useEffect, useRef, useState } from "react";
import type { FormEvent } from "react";
import { getAdminStats, getAdminUsers, setAdminUserBlocked } from "./api";
import type { AdminStats, AdminUser } from "./api";
import "./AdminPanel.css";

const PAGE_SIZE = 25;

export function AdminPanel({ token, currentUserId, onClose }: {
    token: string; currentUserId: string; onClose: () => void;
}) {
    const dialogRef = useRef<HTMLDialogElement>(null);
    const requestRef = useRef(0);
    const [stats, setStats] = useState<AdminStats | null>(null);
    const [users, setUsers] = useState<AdminUser[]>([]);
    const [total, setTotal] = useState(0);
    const [query, setQuery] = useState("");
    const [search, setSearch] = useState("");
    const [state, setState] = useState<"all" | "active" | "blocked">("all");
    const [offset, setOffset] = useState(0);
    const [loading, setLoading] = useState(true);
    const [saving, setSaving] = useState(false);
    const [target, setTarget] = useState<AdminUser | null>(null);
    const [error, setError] = useState("");
    const [notice, setNotice] = useState("");

    useEffect(() => {
        const dialog = dialogRef.current;
        const requests = requestRef;
        dialog?.showModal();
        return () => { dialog?.close(); requests.current++; };
    }, []);

    const refresh = useCallback(async () => {
        const request = ++requestRef.current;
        setLoading(true);
        setError("");
        try {
            const [overview, page] = await Promise.all([
                getAdminStats(token), getAdminUsers(token, search, state, offset, PAGE_SIZE),
            ]);
            if (request !== requestRef.current) return;
            if (offset > 0 && page.total <= offset) {
                setOffset(Math.max(0, Math.ceil(page.total / PAGE_SIZE) - 1) * PAGE_SIZE);
                return;
            }
            setStats(overview);
            setUsers(page.users);
            setTotal(page.total);
        } catch (cause) {
            if (request === requestRef.current) setError(cause instanceof Error ? cause.message : "Could not load admin panel");
        } finally {
            if (request === requestRef.current) setLoading(false);
        }
    }, [token, search, state, offset]);
    useEffect(() => { void refresh(); }, [refresh]);

    function submitSearch(event: FormEvent) {
        event.preventDefault();
        setOffset(0);
        setSearch(query.trim());
    }

    async function changeBlock() {
        if (!target || saving) return;
        setSaving(true);
        setError("");
        setNotice("");
        try {
            const updated = await setAdminUserBlocked(token, target.id, !target.is_blocked);
            setTarget(null);
            setNotice(`@${updated.username} ${updated.is_blocked ? "blocked" : "unblocked"}.`);
            await refresh();
        } catch (cause) {
            setError(cause instanceof Error ? cause.message : "Could not update account");
        } finally {
            setSaving(false);
        }
    }

    return <dialog ref={dialogRef} className="admin-panel" aria-labelledby="admin-title" onCancel={event => {
        if (saving || target) { event.preventDefault(); if (!saving) setTarget(null); }
        else onClose();
    }}>
        <header className="admin-header">
            <div><span className="admin-eyebrow">MESSENGER</span><h2 id="admin-title">Admin panel</h2><p>Overview and account management</p></div>
            <button type="button" className="admin-close" aria-label="Close admin panel" disabled={saving} onClick={onClose}>×</button>
        </header>
        {error && <p className="admin-error" role="alert">{error} <button type="button" onClick={() => void refresh()} disabled={loading || saving}>Retry</button></p>}
        {notice && <p className="admin-notice" role="status">{notice}</p>}
        <section className="admin-stats" aria-label="Messenger statistics" aria-busy={loading}>
            {([
                ["Users", stats?.users], ["Online now", stats?.online_users],
                ["Groups", stats?.groups], ["Private chats", stats?.private_chats],
                ["Messages", stats?.messages], ["Blocked", stats?.blocked_users],
            ] as const).map(([label, value]) => <div key={label}><span>{label}</span><strong>{value?.toLocaleString() ?? "—"}</strong></div>)}
        </section>
        <section className="admin-accounts" aria-labelledby="admin-users-title">
            <div className="admin-section-title"><h3 id="admin-users-title">Users <span>{total}</span></h3><button type="button" onClick={() => void refresh()} disabled={loading || saving || Boolean(target)}>Refresh</button></div>
            <form className="admin-filters" onSubmit={submitSearch}>
                <input aria-label="Search users" placeholder="Name, username or email" value={query} onChange={event => setQuery(event.target.value)} maxLength={100} disabled={saving || Boolean(target)} />
                <select aria-label="Account status" value={state} disabled={saving || Boolean(target)} onChange={event => { setState(event.target.value as typeof state); setOffset(0); }}><option value="all">All accounts</option><option value="active">Active</option><option value="blocked">Blocked</option></select>
                <button type="submit" disabled={loading || saving || Boolean(target)}>Search</button>
            </form>
            {target && <div className="admin-confirm" role="region" aria-label="Confirm account change">
                <strong>{target.is_blocked ? "Unblock" : "Block"} @{target.username}?</strong>
                <p>{target.is_blocked ? "This user will be able to sign in again." : "The user will be signed out and unable to sign in or send messages. Their messages will remain."}</p>
                <button type="button" className={target.is_blocked ? "" : "admin-danger"} disabled={saving} onClick={() => void changeBlock()}>{saving ? "Saving…" : target.is_blocked ? "Unblock account" : "Block account"}</button>
                <button type="button" disabled={saving} onClick={() => setTarget(null)}>Cancel</button>
            </div>}
            <div className="admin-table-wrap" aria-busy={loading}>
                <table><thead><tr><th>User</th><th>Email</th><th>Joined</th><th>Status</th><th>Action</th></tr></thead>
                    <tbody>{loading ? <tr><td colSpan={5} className="admin-empty">Loading users…</td></tr> : users.length === 0 ? <tr><td colSpan={5} className="admin-empty">No users found.</td></tr> : users.map(user => <tr key={user.id}>
                        <td><strong>{user.display_name || user.username}</strong><small>@{user.username}{user.id === currentUserId ? " · you" : ""}</small></td>
                        <td>{user.email || "—"}{user.email && <small>{user.email_verified_at ? "Verified" : "Not verified"}</small>}</td>
                        <td>{new Date(user.created_at).toLocaleDateString()}</td>
                        <td><span className={`admin-badge ${user.is_blocked ? "blocked" : user.is_admin ? "administrator" : "active"}`}>{user.is_blocked ? "Blocked" : user.is_admin ? "Admin" : "Active"}</span></td>
                        <td>{user.is_admin || user.id === currentUserId ? <span className="admin-protected">Protected</span> : <button type="button" className={user.is_blocked ? "" : "admin-danger-text"} disabled={saving || Boolean(target)} onClick={() => { setTarget(user); setNotice(""); }}>{user.is_blocked ? "Unblock" : "Block"}</button>}</td>
                    </tr>)}</tbody>
                </table>
            </div>
            <footer className="admin-pagination"><span>{total ? `${offset + 1}–${Math.min(offset + PAGE_SIZE, total)} of ${total}` : "0 users"}</span><div><button type="button" disabled={loading || saving || Boolean(target) || offset === 0} onClick={() => setOffset(value => Math.max(0, value - PAGE_SIZE))}>Previous</button><button type="button" disabled={loading || saving || Boolean(target) || offset + PAGE_SIZE >= total} onClick={() => setOffset(value => value + PAGE_SIZE)}>Next</button></div></footer>
        </section>
    </dialog>;
}
