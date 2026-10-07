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
    private static final String DEFAULT_SERVER = "https://messenger-01.tail1a26d2.ts.net";
    private WebView web;
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
        Button settings = new Button(this);
        settings.setText("Messenger · Сервер");
        settings.setOnClickListener(v -> showServerSettings());
        root.addView(settings);
        progress = new ProgressBar(this, null, android.R.attr.progressBarStyleHorizontal);
        root.addView(progress, new LinearLayout.LayoutParams(-1, 6));
        errorPanel = new LinearLayout(this);
        errorPanel.setOrientation(LinearLayout.VERTICAL);
        errorPanel.setPadding(24,24,24,24);
        TextView message = new TextView(this);
        message.setText("Не удалось подключиться. Проверьте интернет и адрес сервера. Для адреса ts.net включите Tailscale и доступ к сети сервера.");
        message.setTextColor(Color.WHITE);
        errorPanel.addView(message);
        Button retry = new Button(this);
        retry.setText("Повторить"); retry.setOnClickListener(v -> loadServer());
        errorPanel.addView(retry);
        root.addView(errorPanel);
        web = new WebView(this);
        root.addView(web, new LinearLayout.LayoutParams(-1, 0, 1));
        setContentView(root);
        WebSettings config = web.getSettings();
        config.setJavaScriptEnabled(true);
        config.setDomStorageEnabled(true);
        config.setAllowFileAccess(false);
        config.setAllowContentAccess(true);
        config.setMixedContentMode(WebSettings.MIXED_CONTENT_NEVER_ALLOW);
        CookieManager.getInstance().setAcceptCookie(true);
        CookieManager.getInstance().setAcceptThirdPartyCookies(web, false);
        web.setWebViewClient(new WebViewClient() {
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
                if (checkSelfPermission(Manifest.permission.RECORD_AUDIO) == PackageManager.PERMISSION_GRANTED) {
                    request.grant(new String[]{PermissionRequest.RESOURCE_AUDIO_CAPTURE});
                } else {
                    audioRequest = request;
                    requestPermissions(new String[]{Manifest.permission.RECORD_AUDIO}, AUDIO_REQUEST);
                }
            }
            @Override public void onPermissionRequestCanceled(PermissionRequest request) {
                if (audioRequest == request) audioRequest = null;
            }
        });
        loadServer();
    }

    private boolean trusted(Uri uri) {
        Uri origin = Uri.parse(server);
        return "https".equals(uri.getScheme()) && origin.getHost() != null && origin.getHost().equalsIgnoreCase(uri.getHost())
            && effectivePort(origin) == effectivePort(uri);
    }
    private int effectivePort(Uri uri) { return uri.getPort() == -1 ? 443 : uri.getPort(); }
    private void loadServer() {
        errorPanel.setVisibility(View.GONE); web.setVisibility(View.VISIBLE); web.loadUrl(server);
    }
    private void showError() { errorPanel.setVisibility(View.VISIBLE); web.setVisibility(View.GONE); }
    private void showServerSettings() {
        EditText input = new EditText(this);
        input.setSingleLine(true); input.setInputType(17); input.setText(server);
        AlertDialog dialog = new AlertDialog.Builder(this).setTitle("HTTPS-адрес сервера")
            .setMessage("Укажите адрес вашего мессенджера. При смене сервера потребуется войти заново.")
            .setView(input).setNegativeButton("Отмена", null).setPositiveButton("Подключиться", null).create();
        dialog.setOnShowListener(v -> dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener(button -> {
            String value = input.getText().toString().trim();
            Uri uri = Uri.parse(value);
            if (!"https".equals(uri.getScheme()) || uri.getHost() == null || uri.getUserInfo() != null
                || uri.getQuery() != null || uri.getFragment() != null || !(uri.getPath() == null || uri.getPath().isEmpty() || "/".equals(uri.getPath()))) {
                input.setError("Введите HTTPS-адрес без пути, например https://example.com"); return;
            }
            String next = "https://" + uri.getEncodedAuthority();
            if (!next.equals(server)) {
                web.stopLoading();
                if (audioRequest != null) { audioRequest.deny(); audioRequest = null; }
                finishFiles(null);
                CookieManager.getInstance().removeAllCookies(done -> {
                    CookieManager.getInstance().flush();
                    WebStorage.getInstance().deleteAllData(); web.clearCache(true); web.clearHistory();
                    server = next; getPreferences(MODE_PRIVATE).edit().putString("server", server).apply(); loadServer();
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
        if (request == AUDIO_REQUEST && audioRequest != null) {
            if (results.length > 0 && results[0] == PackageManager.PERMISSION_GRANTED && trusted(audioRequest.getOrigin())) audioRequest.grant(new String[]{PermissionRequest.RESOURCE_AUDIO_CAPTURE});
            else audioRequest.deny();
            audioRequest = null;
        }
    }
    @Override public void onBackPressed() { if (web.canGoBack()) web.goBack(); else super.onBackPressed(); }
    @Override protected void onPause() { CookieManager.getInstance().flush(); super.onPause(); }
    @Override protected void onDestroy() {
        finishFiles(null); if (audioRequest != null) audioRequest.deny();
        web.destroy(); super.onDestroy();
    }
}
