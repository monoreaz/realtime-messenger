package com.monoreaz.messenger;

import android.Manifest;
import android.app.*;
import android.content.*;
import android.content.pm.PackageManager;
import android.media.AudioAttributes;
import android.media.RingtoneManager;
import android.net.Uri;
import android.os.Build;
import android.os.Handler;
import android.os.Looper;
import android.os.PowerManager;
import android.media.Ringtone;
import org.json.JSONObject;
import java.util.Map;
import java.util.UUID;

final class IncomingCalls {
    static final String CHANNEL = "incoming_calls";
    static final String PREF = "incoming_call";
    static final int ID = 4001;
    private static final Handler presentationHandler = new Handler(Looper.getMainLooper());
    private static Runnable presentationTick;
    private static Ringtone foregroundTone;
    private static String toneCallId = "", notificationCallId = "";

    // Serialize FCM and Activity lifecycle updates on the UI thread. Re-read
    // visibility there so a queued push cannot present a stale full-screen UI.
    static void refreshPresentation(Context context) {
        Context app = context.getApplicationContext();
        presentationHandler.post(() -> refreshOnMain(app));
    }

    private static boolean screenLocked(Context context) {
        return !context.getSystemService(PowerManager.class).isInteractive()
            || context.getSystemService(KeyguardManager.class).isKeyguardLocked();
    }

    private static void stopTone() {
        if (foregroundTone != null) {
            try { foregroundTone.stop(); } catch (RuntimeException ignored) { }
        }
        foregroundTone = null;
        toneCallId = "";
    }

    private static synchronized void refreshOnMain(Context context) {
        if (presentationTick != null) presentationHandler.removeCallbacks(presentationTick);
        presentationTick = null;
        JSONObject call = current(context);
        NotificationManager manager = context.getSystemService(NotificationManager.class);
        if (!valid(context, call)) {
            stopTone();
            notificationCallId = "";
            manager.cancel(ID);
            return;
        }
        String id = call.optString("call_id");
        NotificationChannel defaults = new NotificationChannel(CHANNEL, "Incoming calls", NotificationManager.IMPORTANCE_HIGH);
        defaults.setSound(RingtoneManager.getDefaultUri(RingtoneManager.TYPE_RINGTONE),
            new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_NOTIFICATION_RINGTONE).build());
        defaults.enableVibration(true);
        manager.createNotificationChannel(defaults);
        NotificationChannel channel = manager.getNotificationChannel(CHANNEL);
        if (PushNotifications.foreground && !screenLocked(context)) {
            // The WebView already shows Answer/Decline. No CallStyle notification
            // or Activity launch is needed while Messenger is on screen.
            manager.cancel(ID);
            notificationCallId = "";
            if (!id.equals(toneCallId)) {
                stopTone();
                toneCallId = id;
                if (manager.areNotificationsEnabled() && channel.getImportance() >= NotificationManager.IMPORTANCE_DEFAULT
                    && channel.getSound() != null) {
                    try {
                        foregroundTone = RingtoneManager.getRingtone(context, channel.getSound());
                        if (foregroundTone != null) {
                            foregroundTone.setAudioAttributes(new AudioAttributes.Builder().setUsage(AudioAttributes.USAGE_NOTIFICATION_RINGTONE).build());
                            if (Build.VERSION.SDK_INT >= 28) foregroundTone.setLooping(true);
                            foregroundTone.play();
                        }
                    } catch (RuntimeException ignored) { stopTone(); toneCallId = id; }
                }
            }
        } else {
            stopTone();
            if (!id.equals(notificationCallId)) {
                showNotification(context, call);
                notificationCallId = id;
            }
        }
        presentationTick = () -> refreshOnMain(context);
        presentationHandler.postDelayed(presentationTick, 500);
    }


    static JSONObject current(Context context) {
        try { return new JSONObject(PushNotifications.preferences(context).getString(PREF, "{}")); }
        catch (Exception error) { return new JSONObject(); }
    }

    static boolean valid(Context context, JSONObject call) {
        return call.optLong("expires_at") > System.currentTimeMillis()
            && PushNotifications.matches(context, call.optString("user_id"), call.optString("binding"));
    }

    static synchronized void clear(Context context, String id) {
        if (id == null || id.isEmpty()) return;
        SharedPreferences prefs = PushNotifications.preferences(context);
        // Tombstones prevent a late incoming push from resurrecting an ended call.
        JSONObject ended;
        try { ended = new JSONObject(prefs.getString("ended_calls", "{}")); }
        catch (Exception error) { ended = new JSONObject(); }
        java.util.Iterator<String> keys = ended.keys();
        long now = System.currentTimeMillis();
        while (keys.hasNext()) { String key = keys.next(); if (ended.optLong(key) < now) keys.remove(); }
        try { ended.put(id, now + 120_000); } catch (Exception ignored) { }
        prefs.edit().putString("ended_calls", ended.toString()).apply();
        if (id.equals(current(context).optString("call_id"))) {
            prefs.edit().remove(PREF).apply();
            context.getSystemService(NotificationManager.class).cancel(ID);
            refreshPresentation(context);
        }
    }

    static Intent openIntent(Context context, JSONObject call, String action) {
        return new Intent(context, MainActivity.class)
            .setData(Uri.parse("messenger://call/" + call.optString("call_id") + "/" + action))
            .putExtra("push_user", call.optString("user_id"))
            .putExtra("push_binding", call.optString("binding"))
            .putExtra("push_call", call.optString("call_id"))
            .putExtra("push_peer", call.optString("peer_id"))
            .putExtra("push_call_action", action)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TOP | Intent.FLAG_ACTIVITY_SINGLE_TOP);
    }

    static PendingIntent answerIntent(Context context, JSONObject call) {
        return PendingIntent.getActivity(context, 0, openIntent(context, call, "answer"),
            PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
    }

    static PendingIntent declineIntent(Context context, JSONObject call) {
        Intent intent = new Intent(context, DeclineCallReceiver.class)
            .setData(Uri.parse("messenger://decline/" + call.optString("call_id")))
            .putExtra("call_id", call.optString("call_id"));
        return PendingIntent.getBroadcast(context, 0, intent, PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
    }

    static synchronized void receive(Context context, Map<String, String> data) {
        String id = data.getOrDefault("call_id", "");
        try { UUID.fromString(id); } catch (IllegalArgumentException error) { return; }
        if (!PushNotifications.matches(context, data.getOrDefault("user_id", ""), data.getOrDefault("binding", ""))) return;
        if ("call.dismiss".equals(data.get("type"))) { clear(context, id); return; }
        JSONObject call = new JSONObject(data);
        if (!valid(context, call)) return;
        try {
            JSONObject ended = new JSONObject(PushNotifications.preferences(context).getString("ended_calls", "{}"));
            if (ended.optLong(id) > System.currentTimeMillis()) return;
        } catch (Exception ignored) { }
        if (id.equals(current(context).optString("call_id"))) return;
        PushNotifications.preferences(context).edit().putString(PREF, call.toString()).apply();
        refreshPresentation(context);
    }

    private static void showNotification(Context context, JSONObject call) {
        if (Build.VERSION.SDK_INT >= 33 && context.checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED) return;
        NotificationManager manager = context.getSystemService(NotificationManager.class);
        String id = call.optString("call_id");
        Intent screen = new Intent(context, IncomingCallActivity.class)
            .setData(Uri.parse("messenger://incoming/" + id)).putExtra("call_id", id)
            .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK | Intent.FLAG_ACTIVITY_CLEAR_TOP);
        PendingIntent fullScreen = PendingIntent.getActivity(context, 0, screen,
            PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE);
        String name = call.optString("caller_name", "Messenger");
        Notification.Builder builder = new Notification.Builder(context, CHANNEL)
            .setSmallIcon(R.drawable.ic_notification).setContentTitle(name).setContentText("Incoming audio call")
            .setCategory(Notification.CATEGORY_CALL).setVisibility(Notification.VISIBILITY_PRIVATE)
            .setOngoing(true).setAutoCancel(false).setContentIntent(PendingIntent.getActivity(context, 0, openIntent(context, call, "open"), PendingIntent.FLAG_UPDATE_CURRENT | PendingIntent.FLAG_IMMUTABLE))
            .setTimeoutAfter(Math.max(1, call.optLong("expires_at") - System.currentTimeMillis()));
        if (screenLocked(context) && (Build.VERSION.SDK_INT < 34 || manager.canUseFullScreenIntent())) builder.setFullScreenIntent(fullScreen, true);
        if (Build.VERSION.SDK_INT >= 31) {
            builder.setStyle(Notification.CallStyle.forIncomingCall(new Person.Builder().setName(name).setImportant(true).build(),
                declineIntent(context, call), answerIntent(context, call)));
        } else {
            builder.addAction(new Notification.Action.Builder(android.R.drawable.ic_menu_close_clear_cancel, "Decline", declineIntent(context, call)).build());
            builder.addAction(new Notification.Action.Builder(android.R.drawable.sym_action_call, "Answer", answerIntent(context, call)).build());
        }
        Notification notification = builder.build();
        notification.flags |= Notification.FLAG_INSISTENT;
        manager.notify(ID, notification);
    }
}
