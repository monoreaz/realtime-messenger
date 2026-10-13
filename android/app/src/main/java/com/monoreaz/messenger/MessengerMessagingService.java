package com.monoreaz.messenger;

import com.google.firebase.messaging.FirebaseMessagingService;
import com.google.firebase.messaging.RemoteMessage;

public class MessengerMessagingService extends FirebaseMessagingService {
    @Override public void onMessageReceived(RemoteMessage message) {
        PushNotifications.show(this, message.getData());
    }

    @Override public void onNewToken(String token) {
        // The foreground bridge re-registers getToken() on resume and periodically.
        // Never retain an access token or a refresh cookie in the native service.
        PushNotifications.preferences(this).edit().putString("token", token).apply();
    }
}
