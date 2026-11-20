import { useEffect, useState } from "react";
import { formatLastSeen } from "./lastSeen";

/** Last-seen label that keeps its relative wording ("5 minutes ago") current. */
export function LastSeenText({ at }: { at: string }) {
    const [now, setNow] = useState(() => new Date());

    useEffect(() => {
        const timer = window.setInterval(() => setNow(new Date()), 30_000);
        return () => window.clearInterval(timer);
    }, []);

    return <span title={new Date(at).toLocaleString()}>{formatLastSeen(at, now)}</span>;
}
