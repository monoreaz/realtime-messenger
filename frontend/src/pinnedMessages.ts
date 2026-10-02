type TimelineMessage = { id: string; created_at: string };

function compareMessages(a: TimelineMessage, b: TimelineMessage): number {
    return Date.parse(a.created_at) - Date.parse(b.created_at) || a.id.localeCompare(b.id);
}

// Follow message position in the conversation, not the order in which pins were added.
export function selectPinnedMessage<T extends TimelineMessage>(pins: T[], anchor?: TimelineMessage): T | undefined {
    const ordered = [...pins].sort(compareMessages);
    if (!anchor) return ordered.at(-1);
    return ordered.filter(pin => compareMessages(pin, anchor) <= 0).at(-1) ?? ordered[0];
}
