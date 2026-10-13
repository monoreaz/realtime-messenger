package com.monoreaz.messenger;

import android.Manifest;
import android.app.*;
import android.content.*;
import android.content.pm.PackageManager;
import android.net.Uri;
import android.os.Build;
import java.util.Map;
import java.util.UUID;

final class PushNotifications {
    static final String CHANNEL = "messages";
    static volatile boolean foreground;
    static volatile String activeChat = "";

    static SharedPreferences preferences(Context context) {
        return context.getSharedPreferences("push", Context.MODE_PRIVATE);
    }

    static void createChannel(Context context) {
        NotificationChannel channel = new NotificationChannel(CHANNEL, "Messages", NotificationManager.IMPORTANCE_HIGH);
        channel.setDescription("New messages in personal and group chats");
        context.getSystemService(NotificationManager.class).createNotificationChannel(channel);
    }

    static synchronized String bind(Context context, String server, String user) {
        SharedPreferences prefs = preferences(context);
        if (!server.equals(prefs.getString("server", "")) || !user.equals(prefs.getString("user", ""))) {
            context.getSystemService(NotificationManager.class).cancelAll();
            prefs.edit().putString("server", server).putString("user", user)
                .putString("binding", user.isEmpty() ? "" : UUID.randomUUID().toString()).apply();
        }
        return prefs.getString("binding", "");
    }

    static boolean matches(Context context, String user, String binding) {
        SharedPreferences prefs = preferences(context);
        return !user.isEmpty() && !binding.isEmpty() && user.equals(prefs.getString("user", ""))
            && binding.equals(prefs.getString("binding", ""));
    }

    static synchronized void show(Context context, Map<String, String> data) {
        String user = data.getOrDefault("user_id", "");
        String binding = data.getOrDefault("binding", "");
        String chat = data.getOrDefault("chat_id", "");
        if (!matches(context, user, binding) || chat.isEmpty()) return;
        try { UUID.fromString(chat); } catch (IllegalArgumentException error) { return; }
        if (foreground && chat.equals(activeChat)) return;
        if (Build.VERSION.SDK_INT >= 33 && context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) return;
        createChannel(context);
        Intent intent = new Intent(context, MainActivity.class)
            .setData(Uri.parse("messenger://push/" + chat))
            .putExtra("push_chat", chat).putExtra("push_user", user).putExtra("push_binding", binding)
            .addFlags(Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
        PendingIntent pending = PendingIntent.getActivity(context, 0, intent, PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
        Notification notification = new Notification.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification).setContentTitle("Messenger")
            .setContentText("New message").setCategory(Notification.CATEGORY_MESSAGE)
            .setVisibility(Notification.VISIBILITY_PRIVATE).setAutoCancel(true)
            .setContentIntent(pending).build();
        context.getSystemService(NotificationManager.class).notify(chat, 0, notification);
    }
}
