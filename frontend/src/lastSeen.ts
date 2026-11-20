export type LastSeenVisibility = "everyone" | "nobody";

const MINUTE = 60_000;
const HOUR = 60 * MINUTE;

function isSameDay(first: Date, second: Date): boolean {
    return (
        first.getFullYear() === second.getFullYear() &&
        first.getMonth() === second.getMonth() &&
        first.getDate() === second.getDate()
    );
}

/** Human-readable "Last seen …" text for an ISO timestamp, relative to `now`. */
export function formatLastSeen(value: string, now: Date = new Date(), locale?: string): string {
    const seen = new Date(value);

    if (Number.isNaN(seen.getTime())) {
        return "Offline";
    }

    const elapsed = now.getTime() - seen.getTime();

    if (elapsed < MINUTE) {
        return "Last seen just now";
    }

    if (elapsed < HOUR) {
        const minutes = Math.floor(elapsed / MINUTE);
        return `Last seen ${minutes} ${minutes === 1 ? "minute" : "minutes"} ago`;
    }

    const time = seen.toLocaleTimeString(locale, { hour: "2-digit", minute: "2-digit" });

    if (isSameDay(seen, now)) {
        return `Last seen today at ${time}`;
    }

    const yesterday = new Date(now);
    yesterday.setDate(now.getDate() - 1);

    if (isSameDay(seen, yesterday)) {
        return `Last seen yesterday at ${time}`;
    }

    const date = seen.toLocaleDateString(locale, {
        day: "numeric",
        month: "short",
        ...(seen.getFullYear() === now.getFullYear() ? {} : { year: "numeric" }),
    });

    return `Last seen ${date}`;
}

/** Applies a presence event's optional last-seen value to the per-user map. */
export function withLastSeen(
    current: ReadonlyMap<string, string>,
    userId: string,
    lastSeenAt: unknown,
): Map<string, string> {
    const updated = new Map(current);

    if (typeof lastSeenAt === "string" && lastSeenAt) {
        updated.set(userId, lastSeenAt);
    } else {
        updated.delete(userId);
    }

    return updated;
}
