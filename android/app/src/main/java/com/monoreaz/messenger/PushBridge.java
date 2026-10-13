package com.monoreaz.messenger;

import android.Manifest;
import android.app.NotificationManager;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.os.Build;
import android.webkit.WebView;
import androidx.webkit.WebViewCompat;
import androidx.webkit.WebViewFeature;
import com.google.firebase.FirebaseApp;
import com.google.firebase.messaging.FirebaseMessaging;
import java.util.Collections;
import org.json.JSONObject;

final class PushBridge {
    private final MainActivity activity;
    private final WebView web;
    private boolean installed;

    PushBridge(MainActivity activity, WebView web) { this.activity = activity; this.web = web; }

    void install(String server) {
        if (!WebViewFeature.isFeatureSupported(WebViewFeature.WEB_MESSAGE_LISTENER)) return;
        if (installed) WebViewCompat.removeWebMessageListener(web, "MessengerPush");
        WebViewCompat.addWebMessageListener(web, "MessengerPush", Collections.singleton(server),
            (view, message, origin, mainFrame, reply) -> {
                if (!mainFrame) return;
                try {
                    JSONObject request = new JSONObject(message.getData());
                    String user = request.optString("userId", "");
                    if (!user.isEmpty()) java.util.UUID.fromString(user);
                    String binding = PushNotifications.bind(activity, server, user);
                    PushNotifications.activeChat = request.optString("chatId", "");
                    if (PushNotifications.foreground && !PushNotifications.activeChat.isEmpty()) {
                        activity.getSystemService(NotificationManager.class).cancel(PushNotifications.activeChat, 0);
                    }
                    if (user.isEmpty() || FirebaseApp.getApps(activity).isEmpty()) return;
                    PushNotifications.createChannel(activity);
                    if (Build.VERSION.SDK_INT >= 33
                        && activity.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
                        && !PushNotifications.preferences(activity).getBoolean("permissionAsked", false)) {
                        PushNotifications.preferences(activity).edit().putBoolean("permissionAsked", true).apply();
                        activity.requestPermissions(new String[]{Manifest.permission.POST_NOTIFICATIONS}, 12);
                    }
                    Intent intent = activity.getIntent();
                    if (user.equals(intent.getStringExtra("push_user")) && binding.equals(intent.getStringExtra("push_binding")) && intent.hasExtra("push_chat")) {
                        reply.postMessage(new JSONObject().put("userId", user).put("openChatId", intent.getStringExtra("push_chat")).toString());
                        intent.removeExtra("push_chat");
                    }
                    FirebaseMessaging.getInstance().getToken().addOnSuccessListener(activity, token -> {
                        if (!PushNotifications.matches(activity, user, binding)) return;
                        try {
                            JSONObject response = new JSONObject().put("token", token).put("binding", binding).put("userId", user);
                            reply.postMessage(response.toString());
                        } catch (org.json.JSONException ignored) { }
                    });
                } catch (org.json.JSONException | IllegalArgumentException ignored) { }
            });
        installed = true;
    }
}
