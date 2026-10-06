export function CallMessage({ content, outgoing }: { content: string; outgoing: boolean }) {
    const completed = /^Audio call · \d+ min \d{2} sec$/.test(content);
    const detail = content.replace(/^Audio call(?: · | )?/, "");
    return (
        <div className={`call-message-card ${completed ? "completed" : "unanswered"}`}>
            <span className="call-message-symbol" aria-hidden="true">
                <svg width="25" height="25" viewBox="0 0 24 24" fill="none" stroke="currentColor" strokeWidth="1.8" strokeLinecap="round" strokeLinejoin="round">
                    <path d="M5 3h4l2 5-3 2a15 15 0 0 0 6 6l2-3 5 2v4a2 2 0 0 1-2 2C10 21 3 14 3 5a2 2 0 0 1 2-2Z" />
                </svg>
            </span>
            <div className="call-message-details">
                <strong>{outgoing ? "Outgoing audio call" : "Incoming audio call"}</strong>
                <span><span aria-hidden="true">{outgoing ? "↗" : "↙"}</span> {completed ? detail : content === "Missed audio call" ? "Missed call" : detail || "Call ended"}</span>
            </div>
        </div>
    );
}
