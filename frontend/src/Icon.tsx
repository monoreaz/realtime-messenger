import brandMarkUrl from "./assets/brand-mark.webp";

// Stroke icons on a 24×24 grid. Keep them visually consistent: 1.7px round strokes, no fills.
const paths = {
    settings: "M9.5 3h5l.6 2.4 2.1 1.2 2.4-.7 2.5 4.2-1.8 1.7v2.4l1.8 1.7-2.5 4.2-2.4-.7-2.1 1.2-.6 2.4h-5l-.6-2.4-2.1-1.2-2.4.7L1.9 16l1.8-1.7v-2.4L1.9 10.2l2.5-4.3 2.4.7 2.1-1.2L9.5 3Z M15.5 12a3.5 3.5 0 1 1-7 0 3.5 3.5 0 0 1 7 0",
    plus: "M12 5v14M5 12h14",
    back: "m14 5-7 7 7 7M7 12h14",
    shield: "M12 3 4 6v6c0 5 8 9 8 9s8-4 8-9V6l-8-3Z M9 12l2 2 4-4",
    user: "M16 7a4 4 0 1 1-8 0 4 4 0 0 1 8 0 M4 21v-2a8 8 0 0 1 16 0v2",
    lock: "M7 10V7a5 5 0 0 1 10 0v3 M5 10h14v11H5z M12 14v3",
    device: "M3 4h18v13H3z M8 21h8M12 17v4",
    pin: "m8 3 8 0-1 7 4 4v2H5v-2l4-4-1-7ZM12 16v6",
    unpin: "m8 3 8 0-1 7 4 4v2H5v-2l4-4-1-7ZM12 16v6 M3 3l18 18",
    reply: "m9 5-6 6 6 6M3 11h10a8 8 0 0 1 8 8",
    edit: "m15 4 5 5M4 20l5-1L21 7l-5-5L4 14v6Z",
    close: "m6 6 12 12M6 18 18 6",
    group: "M15 7a3 3 0 1 1-6 0 3 3 0 0 1 6 0 M5 21v-2a7 7 0 0 1 14 0v2 M18 4a3 3 0 0 1 0 6M21 20v-2a6 6 0 0 0-3-5",
    search: "M18 11a7 7 0 1 1-14 0 7 7 0 0 1 14 0 M20 20l-4.2-4.2",
    paperclip: "m20.5 11.5-8.4 8.4a5 5 0 0 1-7.1-7.1l8.4-8.4a3.3 3.3 0 0 1 4.7 4.7l-8.4 8.4a1.7 1.7 0 0 1-2.4-2.4l7.8-7.8",
    send: "M12 19V5 M6 11l6-6 6 6",
    phone: "M21 16.4v2.8a1.9 1.9 0 0 1-2 1.9A18.8 18.8 0 0 1 2.9 5a1.9 1.9 0 0 1 1.9-2h2.8a1.9 1.9 0 0 1 1.9 1.6c.1.9.4 1.8.7 2.7a1.9 1.9 0 0 1-.4 2L8.6 10.5a15 15 0 0 0 4.9 4.9l1.2-1.2a1.9 1.9 0 0 1 2-.4c.9.3 1.8.6 2.7.7a1.9 1.9 0 0 1 1.6 1.9Z",
    logout: "M9 4H6a2 2 0 0 0-2 2v12a2 2 0 0 0 2 2h3 M16 17l5-5-5-5 M21 12H9",
    eye: "M2 12s3.6-7 10-7 10 7 10 7-3.6 7-10 7S2 12 2 12Z M15 12a3 3 0 1 1-6 0 3 3 0 0 1 6 0",
    eyeOff: "M3 3l18 18 M10.6 5.1Q11.3 5 12 5c6.4 0 10 7 10 7a17 17 0 0 1-3 3.9 M6.6 6.6A17 17 0 0 0 2 12s3.6 7 10 7a9.7 9.7 0 0 0 5.4-1.6 M9.9 9.9a3 3 0 0 0 4.2 4.2",
    chevronRight: "m9 6 6 6-6 6",
    chevronLeft: "m15 6-6 6 6 6",
    arrowDown: "M12 5v14 M6 13l6 6 6-6",
    list: "M9 6h12M9 12h12M9 18h12M4 6h.01M4 12h.01M4 18h.01",
    play: "M7 4.8v14.4a.8.8 0 0 0 1.2.7l11.3-7.2a.8.8 0 0 0 0-1.4L8.2 4.1a.8.8 0 0 0-1.2.7Z",
    mic: "M9 5.5a3 3 0 0 1 6 0V11a3 3 0 0 1-6 0Z M5 10.5v.5a7 7 0 0 0 14 0v-.5 M12 18v3 M8.5 21h7",
    micOff: "M9 5.5a3 3 0 0 1 6 0V11a3 3 0 0 1-6 0Z M5 10.5v.5a7 7 0 0 0 14 0v-.5 M12 18v3 M8.5 21h7 M3 3l18 18",
    check: "m5 12.5 4.5 4.5L19 7.5",
    checkDouble: "m2.5 12.5 4.5 4.5 9.5-9.5 M11.5 16l1 1 9.5-9.5",
    trash: "M4 7h16 M10 11v6 M14 11v6 M6 7l1 13h10l1-13 M9 7V4h6v3",
    chat: "M21 12a8.5 8.5 0 0 1-12.4 7.6L3.5 21l1.5-4.7A8.5 8.5 0 1 1 21 12Z",
    refresh: "M20 11a8 8 0 0 0-14.3-4.9L4 8 M4 4v4h4 M4 13a8 8 0 0 0 14.3 4.9L20 16 M20 20v-4h-4",
    image: "M4 5h16v14H4z M4 16l4.5-4.5 4 4 2.5-2.5L20 18 M15.5 9.5h.01",
    video: "M3 6h12v12H3z M15 10l6-3.5v11L15 14",
} as const;

export type IconName = keyof typeof paths;

export function Icon({ name, className }: { name: IconName; className?: string }) {
    return <svg className={className ? `ui-icon ${className}` : "ui-icon"} viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.7" strokeLinecap="round" strokeLinejoin="round" aria-hidden="true" focusable="false"><path d={paths[name]} /></svg>;
}

/** The product mark: the glowing glass "S" tile (256px WebP cropped from the master sileoicon.png). */
export function BrandMark({ className = "" }: { className?: string }) {
    return (
        <span className={`brand-mark ${className}`} aria-hidden="true">
            <img src={brandMarkUrl} alt="" draggable={false} />
        </span>
    );
}
