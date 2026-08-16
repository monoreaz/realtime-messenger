import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

export default defineConfig({
    plugins: [react()],
    server: {
        allowedHosts: [
            "messenger-01.tail1a26d2.ts.net",
        ],
    },
});