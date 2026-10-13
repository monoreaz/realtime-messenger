package com.monoreaz.messenger;

import android.content.*;
import org.json.JSONObject;
import javax.net.ssl.HttpsURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;

public class DeclineCallReceiver extends BroadcastReceiver {
    @Override public void onReceive(Context context, Intent intent) {
        JSONObject call = IncomingCalls.current(context);
        if (!call.optString("call_id").equals(intent.getStringExtra("call_id")) || !IncomingCalls.valid(context, call)) return;
        String server = PushNotifications.preferences(context).getString("server", "");
        IncomingCalls.clear(context, call.optString("call_id"));
        PendingResult result = goAsync();
        new Thread(() -> {
            HttpsURLConnection connection = null;
            try {
                // Capability authorizes only declining this call for this device, for 60 seconds.
                URL url = new URL(server + "/api/calls/" + call.getString("call_id") + "/decline");
                connection = (HttpsURLConnection) url.openConnection();
                connection.setInstanceFollowRedirects(false);
                connection.setConnectTimeout(3000); connection.setReadTimeout(4000);
                connection.setRequestMethod("POST"); connection.setDoOutput(true);
                connection.setRequestProperty("Content-Type", "application/json");
                byte[] body = new JSONObject().put("token", call.getString("decline_token")).toString().getBytes(StandardCharsets.UTF_8);
                try (java.io.OutputStream output = connection.getOutputStream()) { output.write(body); }
                connection.getResponseCode();
            } catch (Exception ignored) {
                // Offline decline silences this device; server-side ringing still expires.
            } finally { if (connection != null) connection.disconnect(); result.finish(); }
        }, "decline-call").start();
    }
}
