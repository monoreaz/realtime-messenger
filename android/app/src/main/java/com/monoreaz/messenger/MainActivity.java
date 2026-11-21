package com.monoreaz.messenger;

import android.Manifest;
import android.app.Activity;
import android.app.AlertDialog;
import android.content.ActivityNotFoundException;
import android.content.Intent;
import android.content.pm.PackageManager;
import android.graphics.Color;
import android.net.Uri;
import android.os.Bundle;
import android.webkit.*;
import android.view.View;
import android.view.WindowInsets;
import android.widget.*;
import java.util.Arrays;

public class MainActivity extends Activity {
    private static final int FILE_REQUEST = 10, AUDIO_REQUEST = 11;
    // New installs; a server saved in Server settings is kept (the tailnet address still works).
    private static final String DEFAULT_SERVER = "https://chat.slaxz.com";
    private WebView web;
    private PushBridge pushBridge;
    private LinearLayout toolbar;
    private LinearLayout errorPanel;
    private ProgressBar progress;
    private String server;
    private ValueCallback<Uri[]> fileCallback;
    private PermissionRequest audioRequest;

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        server = getPreferences(MODE_PRIVATE).getString("server", DEFAULT_SERVER);
        LinearLayout root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(Color.rgb(23,33,43));
        root.setOnApplyWindowInsetsListener((view, insets) -> {
            if (android.os.Build.VERSION.SDK_INT >= 30) {
                android.graphics.Insets bars = insets.getInsets(WindowInsets.Type.systemBars() | WindowInsets.Type.ime());
                view.setPadding(bars.left, bars.top, bars.right, bars.bottom);
            } else {
                view.setPadding(insets.getSystemWindowInsetLeft(), insets.getSystemWindowInsetTop(), insets.getSystemWindowInsetRight(), insets.getSystemWindowInsetBottom());
            }
            return insets;
        });
        toolbar = new LinearLayout(this);
        toolbar.setGravity(android.view.Gravity.CENTER_VERTICAL);
        toolbar.setPadding(dp(20), 0, dp(8), 0);
        TextView brand = new TextView(this);
        brand.setText("Messenger");
        brand.setTextSize(24);
        brand.setTextColor(Color.rgb(92, 190, 255));
        brand.setTypeface(android.graphics.Typeface.create("sans-serif-medium", android.graphics.Typeface.BOLD));
        brand.setLetterSpacing(0.025f);
        toolbar.addView(brand, new LinearLayout.LayoutParams(0, dp(56), 1));
        brand.setGravity(android.view.Gravity.CENTER_VERTICAL);
        ImageButton settings = new ImageButton(this);
        settings.setImageResource(com.monoreaz.messenger.R.drawable.ic_settings);
        settings.setBackgroundColor(Color.TRANSPARENT);
        settings.setPadding(dp(12), dp(12), dp(12), dp(12));
        settings.setContentDescription("Server settings");
        settings.setOnClickListener(v -> showServerSettings());
        toolbar.addView(settings, new LinearLayout.LayoutParams(dp(48), dp(48)));
        root.addView(toolbar);
        progress = new ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal);
        root.addView(progress, new LinearLayout.LayoutParams(-1, 6));
        errorPanel = new LinearLayout(this);
        errorPanel.setOrientation(LinearLayout.VERTICAL);
        errorPanel.setPadding(24,24,24,24);
        TextView message = new TextView(this);
        message.setText("Unable to connect. Check your internet connection and server address. For ts.net addresses, enable Tailscale and access to the server network.");
        message.setTextColor(Color.WHITE);
        errorPanel.addView(message);
        Button retry = new Button(this);
        retry.setText("Try again"); retry.setOnClickListener(v -> loadServer());
        errorPanel.addView(retry);
        root.addView(errorPanel);
        web = new WebView(this);
        root.addView(web, new LinearLayout.LayoutParams(-1, 0, 1));
        setContentView(root);
        WebSettings config = web.getSettings();
        config.setJavaScriptEnabled(true);
        config.setDomStorageEnabled(true);
        config.setMediaPlaybackRequiresUserGesture(false);
        config.setAllowFileAccess(false);
        config.setAllowContentAccess(true);
        config.setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);
        CookieManager.getInstance().setAcceptCookie(true);
        CookieManager.getInstance().setAcceptThirdPartyCookies(web, false);
        web.addJavascriptInterface(new ChatChrome(), "MessengerChrome");
        pushBridge = new PushBridge(this, web);
        pushBridge.install(server);
        web.setWebViewClient(new WebViewClient() {
            @Override public boolean onRenderProcessGone(WebView view, RenderProcessGoneDetail detail) {
                android.util.Log.e("Messenger", "WebView renderer exited; crashed=" + detail.didCrash());
                audioRequest = null;
                finishFiles(null);
                pushBridge = null;
                stopService(new Intent(MainActivity.this, CallAudioService.class));
                if (view.getParent() instanceof android.view.ViewGroup) ((android.view.ViewGroup) view.getParent()).removeView(view);
                web = null;
                view.destroy();
                toolbar.setVisibility(View.GONE);
                ((TextView) errorPanel.getChildAt(0)).setText("The app window stopped unexpectedly. Tap Try again to reopen Messenger. Your account and chats are kept.");
                errorPanel.setVisibility(View.VISIBLE);
                progress.setVisibility(View.GONE);
                return true;
            }
            @Override public void onPageFinished(WebView view, String url) {
                if (!trusted(Uri.parse(url))) return;
                view.evaluateJavascript("(function(){if(window.messengerChromeObserver)window.messengerChromeObserver.disconnect();"
                    + "function sync(){MessengerChrome.setChatOpen(!!document.querySelector('.messenger.chat-open'));}"
                    + "window.messengerChromeObserver=new MutationObserver(sync);"
                    + "window.messengerChromeObserver.observe(document.documentElement,{subtree:true,childList:true,attributes:true,attributeFilter:['class']});sync();})()", null);
            }

            @Override public boolean shouldOverrideUrlLoading(WebView view, WebResourceRequest request) {
                if (trusted(request.getUrl())) return false;
                if (request.isForMainFrame() && "https".equals(request.getUrl().getScheme())) {
                    try { startActivity(new Intent(Intent.ACTION_VIEW, request.getUrl())); }
                    catch (ActivityNotFoundException ignored) { }
                }
                return true;
            }
            @Override public void onReceivedError(WebView view, WebResourceRequest request, WebResourceError error) {
                if (request.isForMainFrame()) showError();
            }
            @Override public void onReceivedHttpError(WebView view, WebResourceRequest request, WebResourceResponse response) {
                if (request.isForMainFrame() && response.getStatusCode() >= 400) showError();
            }
            @Override public void onReceivedSslError(WebView view, android.webkit.SslErrorHandler handler, android.net.http.SslError error) {
                handler.cancel(); showError();
            }
        });
        web.setWebChromeClient(new WebChromeClient() {
            @Override public void onProgressChanged(WebView view, int value) {
                progress.setProgress(value); progress.setVisibility(value == 100 ? View.GONE : View.VISIBLE);
            }
            @Override public boolean onShowFileChooser(WebView view, ValueCallback<Uri[]> callback, FileChooserParams params) {
                if (fileCallback != null) fileCallback.onReceiveValue(null);
                fileCallback = callback;
                if (!trusted(Uri.parse(view.getUrl() == null ? "" : view.getUrl()))) { finishFiles(null); return true; }
                Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
                intent.addCategory(Intent.CATEGORY_OPENABLE);
                intent.setType("*/*");
                String[] types = Arrays.stream(params.getAcceptTypes()).filter(t -> t.contains("/")).toArray(String[]::new);
                intent.putExtra(Intent.EXTRA_MIME_TYPES, types.length > 0 ? types : new String[]{"image/*", "video/*"});
                intent.putExtra(Intent.EXTRA_ALLOW_MULTIPLE, params.getMode() == FileChooserParams.MODE_OPEN_MULTIPLE);
                try { startActivityForResult(intent, FILE_REQUEST); }
                catch (ActivityNotFoundException error) { finishFiles(null); }
                return true;
            }
            @Override public void onPermissionRequest(PermissionRequest request) {
                if (!trusted(request.getOrigin()) || !Arrays.asList(request.getResources()).contains(PermissionRequest.RESOURCE_AUDIO_CAPTURE)) { request.deny(); return; }
                if (audioRequest != null) { request.deny(); return; }
                audioRequest = request;
                if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
                    grantAudioWhenReady();
                } else {
                    requestPermissions(new String[]{Manifest.permission.RECORD_AUDIO}, AUDIO_REQUEST);
                }
            }
            @Override public void onPermissionRequestCanceled(PermissionRequest request) {
                if (audioRequest == request) audioRequest = null;
            }
        });
        loadServer();
    }

    public class ChatChrome {
        @JavascriptInterface public void setChatOpen(boolean open) {
            runOnUiThread(() -> {
                if (web != null && web.getUrl() != null && trusted(Uri.parse(web.getUrl()))) {
                    int visibility = open ? View.GONE : View.VISIBLE;
                    if (toolbar.getVisibility() != visibility) toolbar.setVisibility(visibility);
                }
            });
        }
    }

    void offerCallScreenPermission() {
        if (android.os.Build.VERSION.SDK_INT < 34 || !PushNotifications.foreground
            || checkSelfPermission(Manifest.permission.POST_NOTIFICATIONS) != PackageManager.PERMISSION_GRANTED
            || getSystemService(android.app.NotificationManager.class).canUseFullScreenIntent()
            || PushNotifications.preferences(this).getBoolean("callScreenAsked", false)) return;
        PushNotifications.preferences(this).edit().putBoolean("callScreenAsked", true).apply();
        new AlertDialog.Builder(this).setTitle("Incoming call screen")
            .setMessage("Allow Messenger to show incoming calls over the lock screen? You can change this later in Server settings → Notifications.")
            .setNegativeButton("Not now", null).setPositiveButton("Open settings", (dialog, which) -> openCallScreenSettings()).show();
    }
    private void openCallScreenSettings() {
        if (android.os.Build.VERSION.SDK_INT >= 34) startActivity(new Intent(android.provider.Settings.ACTION_MANAGE_APP_USE_FULL_SCREEN_INTENT, Uri.parse("package:" + getPackageName())));
    }
    private void showNotificationSettings() {
        if (android.os.Build.VERSION.SDK_INT < 34) {
            startActivity(new Intent(android.provider.Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(android.provider.Settings.EXTRA_APP_PACKAGE, getPackageName()));
            return;
        }
        new AlertDialog.Builder(this).setTitle("Notifications")
            .setItems(new String[]{"Messages and call sounds", "Incoming call screen"}, (dialog, which) -> {
                if (which == 1) openCallScreenSettings();
                else startActivity(new Intent(android.provider.Settings.ACTION_APP_NOTIFICATION_SETTINGS).putExtra(android.provider.Settings.EXTRA_APP_PACKAGE, getPackageName()));
            }).show();
    }

    private int dp(int value) { return Math.round(value * getResources().getDisplayMetrics().density); }

    private boolean trusted(Uri uri) {
        Uri origin = Uri.parse(server);
        return "https".equals(uri.getScheme()) && origin.getHost() != null && origin.getHost().equalsIgnoreCase(uri.getHost())
            && effectivePort(origin) == effectivePort(uri);
    }
    private int effectivePort(Uri uri) { return uri.getPort() == -1 ? 443 : uri.getPort(); }
    private void loadServer() {
        if (web == null) { recreate(); return; }
        toolbar.setVisibility(View.VISIBLE); errorPanel.setVisibility(View.GONE); web.setVisibility(View.VISIBLE); web.loadUrl(server);
    }
    private void showError() { if (web == null) return; toolbar.setVisibility(View.VISIBLE); errorPanel.setVisibility(View.VISIBLE); web.setVisibility(View.GONE); }
    private void showServerSettings() {
        EditText input = new EditText(this);
        input.setSingleLine(true); input.setInputType(17); input.setText(server);
        AlertDialog dialog = new AlertDialog.Builder(this).setTitle("Server address")
            .setMessage("Enter your Messenger HTTPS address. Changing servers requires signing in again.")
            .setNeutralButton("Notifications", (d, which) -> showNotificationSettings())
            .setView(input).setNegativeButton("Cancel", null).setPositiveButton("Connect", null).create();
        dialog.setOnShowListener(v -> dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener(button -> {
            String value = input.getText().toString().trim();
            Uri uri = Uri.parse(value);
            if (!"https".equals(uri.getScheme()) || uri.getHost() == null || uri.getUserInfo() != null
                || uri.getQuery() != null || uri.getFragment() != null || !(uri.getPath() == null || uri.getPath().isEmpty() || "/".equals(uri.getPath()))) {
                input.setError("Enter an HTTPS address without a path, e.g. https://example.com"); return;
            }
            String next = "https://" + uri.getEncodedAuthority();
            if (!next.equals(server)) {
                PushNotifications.bind(this, next, "");
                PushNotifications.activeChat = "";
                if (web == null) { dialog.dismiss(); return; }
                web.stopLoading();
                if (audioRequest != null) { audioRequest.deny(); audioRequest = null; }
                finishFiles(null);
                CookieManager.getInstance().removeAllCookies(done -> {
                    if (web == null) return;
                    CookieManager.getInstance().flush();
                    WebStorage.getInstance().deleteAllData(); web.clearCache(true); web.clearHistory();
                    server = next; pushBridge.install(server); getPreferences(MODE_PRIVATE).edit().putString("server", server).apply(); loadServer();
                });
            }
            dialog.dismiss();
        }));
        dialog.show();
    }
    private void finishFiles(Uri[] uris) { if (fileCallback != null) { fileCallback.onReceiveValue(uris); fileCallback = null; } }
    @Override protected void onActivityResult(int request, int result, Intent data) {
        super.onActivityResult(request, result, data);
        if (request == FILE_REQUEST) finishFiles(WebChromeClient.FileChooserParams.parseResult(result, data));
    }
    @Override public void onRequestPermissionsResult(int request, String[] permissions, int[] results) {
        super.onRequestPermissionsResult(request, permissions, results);
        if (request == 12 && web != null) web.evaluateJavascript("window.dispatchEvent(new Event('messenger-push-sync'))", null);
        if (request == AUDIO_REQUEST && audioRequest != null) {
            if (results.length > 0 && results[0] == PackageManager.PERMISSION_GRANTED) grantAudioWhenReady();
            else { audioRequest.deny(); audioRequest = null; }
        }
    }
    private void grantAudioWhenReady() {
        if (audioRequest == null || !PushNotifications.foreground || !hasWindowFocus()) return;
        if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) != PackageManager.PERMISSION_GRANTED) return;
        PermissionRequest request = audioRequest;
        audioRequest = null;
        if (trusted(request.getOrigin())) request.grant(new String[]{PermissionRequest.RESOURCE_AUDIO_CAPTURE});
        else request.deny();
    }
    @Override public void onWindowFocusChanged(boolean focused) {
        super.onWindowFocusChanged(focused);
        if (focused) grantAudioWhenReady();
    }
    @Override public void onBackPressed() { if (web != null && web.canGoBack()) web.goBack(); else super.onBackPressed(); }
    @Override protected void onNewIntent(Intent intent) {
        super.onNewIntent(intent);
        setIntent(intent);
        if (web != null) web.evaluateJavascript("window.dispatchEvent(new Event('messenger-push-sync'))", null);
    }
    @Override protected void onStart() {
        super.onStart();
        PushNotifications.foreground = true;
        IncomingCalls.refreshPresentation(this);
    }
    @Override protected void onStop() {
        PushNotifications.foreground = false;
        IncomingCalls.refreshPresentation(this);
        super.onStop();
    }
    @Override protected void onResume() {
        super.onResume();
        PushNotifications.foreground = true;
        IncomingCalls.refreshPresentation(this);
        grantAudioWhenReady();
        if (pushBridge != null) pushBridge.resumeCallAudio();
        if (web != null) web.evaluateJavascript("window.dispatchEvent(new Event('messenger-push-sync'))", null);
    }
    @Override protected void onPause() {
        CookieManager.getInstance().flush(); super.onPause();
    }
    @Override protected void onDestroy() {
        finishFiles(null); if (audioRequest != null) audioRequest.deny();
        stopService(new Intent(this, CallAudioService.class));
        if (web != null) web.destroy(); super.onDestroy();
    }
}
