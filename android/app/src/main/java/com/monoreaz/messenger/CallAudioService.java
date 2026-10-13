package com.monoreaz.messenger;

import android.app.*;
import android.content.*;
import android.content.pm.ServiceInfo;
import android.os.*;

/** Started only after the user opens/answers a call and grants microphone access. */
public class CallAudioService extends Service {
    @Override public IBinder onBind(Intent intent) { return null; }
    @Override public int onStartCommand(Intent intent, int flags, int startId) {
        if (intent == null) { stopSelf(); return START_NOT_STICKY; }
        NotificationManager manager = getSystemService(NotificationManager.class);
        manager.createNotificationChannel(new NotificationChannel("active_calls", "Active calls", NotificationManager.IMPORTANCE_LOW));
        PendingIntent open = PendingIntent.getActivity(this, 0, new Intent(this, MainActivity.class)
            .addFlags(Intent.FLAG_ACTIVITY_SINGLE_TOP | Intent.FLAG_ACTIVITY_CLEAR_TOP), PendingIntent.FLAG_IMMUTABLE | PendingIntent.FLAG_UPDATE_CURRENT);
        Notification notification = new Notification.Builder(this, "active_calls")
            .setSmallIcon(R.drawable.ic_notification).setContentTitle("Messenger audio call")
            .setContentText("Tap to return to the call").setCategory(Notification.CATEGORY_CALL)
            .setContentIntent(open).setOngoing(true).build();
        try {
            if (Build.VERSION.SDK_INT >= 30) startForeground(4002, notification, ServiceInfo.FOREGROUND_SERVICE_TYPE_MICROPHONE);
            else startForeground(4002, notification);
        } catch (RuntimeException error) { stopSelf(); }
        return START_NOT_STICKY;
    }
    @Override public void onTaskRemoved(Intent rootIntent) { stopSelf(); }
}
