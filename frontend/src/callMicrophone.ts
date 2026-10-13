// Serialize acquisition across calls, including a cancelled permission prompt.
let pending: Promise<unknown> = Promise.resolve();
export function acquireCallMicrophone(isCurrent: () => boolean): Promise<MediaStream | null> {
    const request = pending.then(async () => {
        for (let attempt = 0; attempt < 2; attempt++) {
            if (!isCurrent()) return null;
            try {
                const media = await navigator.mediaDevices.getUserMedia({ audio: true, video: false });
                if (!isCurrent()) { media.getTracks().forEach(track => track.stop()); return null; }
                return media;
            } catch (error) {
                if (!isCurrent()) return null;
                const name = error instanceof Error ? error.name : "";
                if (attempt === 0 && (name === "NotReadableError" || name === "AbortError")) {
                    await new Promise(resolve => setTimeout(resolve, 500));
                    continue;
                }
                if (name === "NotReadableError" || name === "AbortError") {
                    throw new Error("Microphone could not start. Close other calls or recording apps, check the phone's Microphone access switch, then try again.");
                }
                if (name === "NotAllowedError") throw new Error("Allow microphone access in Android settings and try again.");
                throw error;
            }
        }
        return null;
    });
    pending = request.catch(() => null);
    return request;
}
