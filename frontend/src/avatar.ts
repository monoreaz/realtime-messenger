import type { CSSProperties } from "react";

/** Up to two initials from a display name ("Emily Carter" → "EC", "alex" → "A"). */
export function avatarInitials(name: string): string {
    const words = name.trim().split(/[\s._-]+/).filter(Boolean);
    if (words.length === 0) return "?";
    const letters = words.length > 1 ? words[0][0] + words[1][0] : words[0][0];
    return letters.toUpperCase();
}

/** A stable, muted hue per user so letter avatars are distinguishable without being loud. */
export function avatarStyle(seed: string): CSSProperties {
    let hash = 0;
    for (let index = 0; index < seed.length; index++) {
        hash = (hash * 31 + seed.charCodeAt(index)) | 0;
    }
    return { "--avatar-hue": String(Math.abs(hash) % 360) } as CSSProperties;
}
