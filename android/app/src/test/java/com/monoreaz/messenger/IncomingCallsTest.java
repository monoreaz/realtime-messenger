package com.monoreaz.messenger;

import android.app.Application;
import android.app.KeyguardManager;
import android.app.NotificationManager;
import android.os.Looper;
import android.os.PowerManager;
import org.junit.*;
import org.junit.runner.RunWith;
import org.robolectric.RobolectricTestRunner;
import org.robolectric.RuntimeEnvironment;
import org.robolectric.annotation.Config;
import org.robolectric.Shadows;
import java.time.Duration;
import java.util.*;
import static org.junit.Assert.*;

@RunWith(RobolectricTestRunner.class)
@Config(sdk = 32)
public class IncomingCallsTest {
    private Application app;
    private String id;
    private Map<String,String> payload;
    private NotificationManager manager;

    @Before public void setup() {
        app = RuntimeEnvironment.getApplication();
        id = UUID.randomUUID().toString();
        String user = UUID.randomUUID().toString(), binding = UUID.randomUUID().toString();
        PushNotifications.preferences(app).edit().clear().putString("user",user).putString("binding",binding).apply();
        manager = app.getSystemService(NotificationManager.class);
        Shadows.shadowOf(app.getSystemService(PowerManager.class)).setIsInteractive(true);
        Shadows.shadowOf(app.getSystemService(KeyguardManager.class)).setKeyguardLocked(false);
        payload = new HashMap<>();
        payload.put("type","call.incoming"); payload.put("call_id",id);
        payload.put("user_id",user); payload.put("binding",binding);
        payload.put("expires_at",Long.toString(System.currentTimeMillis()+60000));
        payload.put("caller_name","Caller");
    }
    private void flush() { Shadows.shadowOf(Looper.getMainLooper()).idle(); }
    private void incoming(boolean visible) {
        PushNotifications.foreground = visible;
        IncomingCalls.receive(app,payload); flush();
    }
    @After public void cleanup() {
        IncomingCalls.clear(app,id); flush();
        PushNotifications.foreground = false;
    }
    @Test public void foregroundCallDoesNotPostNotificationOrLaunchActivity() {
        incoming(true);
        assertEquals(0, manager.getActiveNotifications().length);
        assertNull(Shadows.shadowOf(app).getNextStartedActivity());
        assertEquals(id,IncomingCalls.current(app).optString("call_id"));
    }
    @Test @Config(sdk = 35) public void android15ForegroundCallDoesNotPostNotificationOrLaunchActivity() {
        foregroundCallDoesNotPostNotificationOrLaunchActivity();
    }
    @Test public void queuedPushUsesLatestVisibility() {
        PushNotifications.foreground=false;
        IncomingCalls.receive(app,payload);
        PushNotifications.foreground=true;
        flush();
        assertEquals(0,manager.getActiveNotifications().length);
        assertNull(Shadows.shadowOf(app).getNextStartedActivity());
    }
    @Test public void switchingToBackgroundAndBackPreservesPendingCall() {
        incoming(true);
        PushNotifications.foreground=false;
        IncomingCalls.refreshPresentation(app); flush();
        assertEquals(1,manager.getActiveNotifications().length);
        assertNull(manager.getActiveNotifications()[0].getNotification().fullScreenIntent);
        PushNotifications.foreground=true;
        IncomingCalls.refreshPresentation(app); flush();
        assertEquals(0,manager.getActiveNotifications().length);
        assertEquals(id,IncomingCalls.current(app).optString("call_id"));
    }
    @Test public void lockedScreenStillHasFullScreenCall() {
        Shadows.shadowOf(app.getSystemService(KeyguardManager.class)).setKeyguardLocked(true);
        incoming(false);
        assertEquals(1,manager.getActiveNotifications().length);
        assertNotNull(manager.getActiveNotifications()[0].getNotification().fullScreenIntent);
    }
    @Test public void endedCallCannotReappearAfterQueuedPresentationOrLatePush() {
        PushNotifications.foreground=false;
        IncomingCalls.receive(app,payload);
        IncomingCalls.clear(app,id); flush();
        IncomingCalls.receive(app,payload); flush();
        assertEquals(0,manager.getActiveNotifications().length);
        assertFalse(IncomingCalls.valid(app,IncomingCalls.current(app)));
    }
    @Test public void expiredForegroundCallDoesNotReappearWhenAppCloses() throws Exception {
        incoming(true);
        // Robolectric advances the looper clock separately from wall-clock expiry.
        org.json.JSONObject expired = IncomingCalls.current(app);
        expired.put("expires_at",System.currentTimeMillis()-1);
        PushNotifications.preferences(app).edit().putString(IncomingCalls.PREF,expired.toString()).apply();
        Shadows.shadowOf(Looper.getMainLooper()).idleFor(Duration.ofSeconds(1));
        PushNotifications.foreground=false;
        IncomingCalls.refreshPresentation(app); flush();
        assertEquals(0,manager.getActiveNotifications().length);
    }
}
