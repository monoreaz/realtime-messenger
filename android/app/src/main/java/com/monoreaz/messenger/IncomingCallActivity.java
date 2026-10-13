package com.monoreaz.messenger;

import android.app.*;
import android.os.*;
import android.graphics.Color;
import android.view.Gravity;
import android.view.WindowManager;
import android.widget.*;
import org.json.JSONObject;

public class IncomingCallActivity extends Activity {
    private final Handler handler = new Handler(Looper.getMainLooper());
    private String callId;
    private final Runnable check = new Runnable() {
        public void run() {
            JSONObject call = IncomingCalls.current(IncomingCallActivity.this);
            if (!callId.equals(call.optString("call_id")) || !IncomingCalls.valid(IncomingCallActivity.this, call)) {
                IncomingCalls.clear(IncomingCallActivity.this, callId);
                finish(); return;
            }
            handler.postDelayed(this, 500);
        }
    };

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        callId = getIntent().getStringExtra("call_id");
        JSONObject call = IncomingCalls.current(this);
        if (callId == null || !callId.equals(call.optString("call_id")) || !IncomingCalls.valid(this, call)) { finish(); return; }
        if (Build.VERSION.SDK_INT >= 27) { setShowWhenLocked(true); setTurnScreenOn(true); }
        else getWindow().addFlags(WindowManager.LayoutParams.FLAG_SHOW_WHEN_LOCKED | WindowManager.LayoutParams.FLAG_TURN_SCREEN_ON);
        getWindow().addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON);
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL); root.setGravity(Gravity.CENTER);
        root.setPadding(40, 80, 40, 80); root.setBackgroundColor(Color.rgb(23,33,43));
        TextView title = new TextView(this); title.setText("Incoming audio call"); title.setTextColor(Color.LTGRAY); title.setTextSize(20); root.addView(title);
        TextView name = new TextView(this); name.setText(call.optString("caller_name", "Messenger")); name.setTextColor(Color.WHITE); name.setTextSize(32); name.setGravity(Gravity.CENTER); name.setPadding(0, 40, 0, 80); root.addView(name);
        Button answer = new Button(this); answer.setText("Answer"); root.addView(answer);
        Button decline = new Button(this); decline.setText("Decline"); root.addView(decline);
        answer.setOnClickListener(v -> {
            Runnable open = () -> {
                if (!IncomingCalls.valid(this, call)) { finish(); return; }
                IncomingCalls.clear(this, callId);
                startActivity(IncomingCalls.openIntent(this, call, "answer"));
                finish();
            };
            KeyguardManager keyguard = getSystemService(KeyguardManager.class);
            if (keyguard.isKeyguardLocked()) keyguard.requestDismissKeyguard(this, new KeyguardManager.KeyguardDismissCallback() {
                @Override public void onDismissSucceeded() { open.run(); }
            });
            else open.run();
        });
        decline.setOnClickListener(v -> {
            try { IncomingCalls.declineIntent(this, call).send(); } catch (PendingIntent.CanceledException ignored) { }
            finish();
        });
        setContentView(root);
        handler.post(check);
    }
    @Override protected void onDestroy() { handler.removeCallbacks(check); super.onDestroy(); }
}
