package jp.shut.bridge

import android.annotation.SuppressLint
import android.app.Activity
import android.app.AlertDialog
import android.content.Intent
import android.net.Uri
import android.net.http.SslError
import android.os.Bundle
import android.view.WindowManager
import android.webkit.CookieManager
import android.webkit.SslErrorHandler
import android.webkit.ValueCallback
import android.webkit.WebChromeClient
import android.webkit.WebResourceError
import android.webkit.WebResourceRequest
import android.webkit.WebResourceResponse
import android.webkit.WebSettings
import android.webkit.WebView
import android.webkit.WebViewClient
import java.io.ByteArrayInputStream

/** Remote HTML has no JavascriptInterface, Rust IPC, or persistent DOM storage. */
class RemoteActivity : Activity() {
    private var browser: WebView? = null
    private var entry: Uri? = null
    private var failed = false
    private var fileCallback: ValueCallback<Array<Uri>>? = null
    /** 認証ページへ自動入力するパスワード（注入後は即破棄）。 */
    private var pendingPassword: String? = null

    @SuppressLint("SetJavaScriptEnabled")
    override fun onCreate(savedInstanceState: Bundle?) {
        super.onCreate(null)
        // プロセス再生成・システムによる復帰では pending が空になるため、
        // アプリ専用領域の直近接続へ自動で再接続する（クラッシュ復帰用）。
        val session = ConnectionSession.take()
            ?: ConnectionSession.recents(this).firstOrNull()?.let(ConnectionSession::parseEntry)
        if (session == null) { finish(); return }
        val url = session.url
        pendingPassword = session.password.ifEmpty { null }
        entry = Uri.parse(url)
        WebView.setWebContentsDebuggingEnabled(false)
        val view = WebView(this)
        browser = view
        with(view.settings) {
            javaScriptEnabled = true
            // Contract v2: ?ephemeral=1 tells the PC HTML not to access storage.
            domStorageEnabled = false
            databaseEnabled = false
            allowFileAccess = false
            allowContentAccess = false
            javaScriptCanOpenWindowsAutomatically = false
            setSupportMultipleWindows(false)
            setGeolocationEnabled(false)
            cacheMode = WebSettings.LOAD_NO_CACHE
            mixedContentMode = WebSettings.MIXED_CONTENT_NEVER_ALLOW
            @Suppress("DEPRECATION") setSaveFormData(false)
        }
        CookieManager.getInstance().setAcceptCookie(false)
        CookieManager.getInstance().setAcceptThirdPartyCookies(view, false)
        // Deliberately no addJavascriptInterface / custom IPC handler.
        view.webChromeClient = object : WebChromeClient() {
            // <input type=file>（PCへのファイル送信）を端末のピッカーにつなぐ
            override fun onShowFileChooser(
                view: WebView,
                callback: ValueCallback<Array<Uri>>,
                params: FileChooserParams
            ): Boolean {
                fileCallback?.onReceiveValue(null)
                fileCallback = callback
                return try {
                    @Suppress("DEPRECATION")
                    startActivityForResult(params.createIntent(), REQ_FILE)
                    true
                } catch (_: Exception) {
                    fileCallback = null
                    false
                }
            }
        }
        view.webViewClient = object : WebViewClient() {
            override fun shouldOverrideUrlLoading(view: WebView, request: WebResourceRequest): Boolean = !allowedNavigation(request.url)
            @Deprecated("Used on older WebView implementations")
            override fun shouldOverrideUrlLoading(view: WebView, url: String): Boolean = !allowedNavigation(Uri.parse(url))
            override fun shouldInterceptRequest(view: WebView, request: WebResourceRequest): WebResourceResponse? =
                if (sameOrigin(request.url)) null else blocked()
            override fun onReceivedSslError(view: WebView, handler: SslErrorHandler, error: SslError) {
                handler.cancel()
                fail("接続先の証明書を確認できませんでした。PCの接続URLを確認してください。")
            }
            override fun onReceivedError(view: WebView, request: WebResourceRequest, error: WebResourceError) {
                if (request.isForMainFrame) fail("PCに接続できませんでした。PC側のShut、接続URL、ネットワークを確認してください。")
            }
            override fun onPageFinished(view: WebView, url: String) {
                view.clearHistory()
                val pw = pendingPassword ?: return
                pendingPassword = null
                // 認証ページの shutLogin() へパスワードを注入。
                // URLやhistoryには乗せず、ページ内fetchで /api/auth へ送られる。
                view.evaluateJavascript(
                    "window.shutLogin && window.shutLogin(" + org.json.JSONObject.quote(pw) + ")", null
                )
            }
        }
        setContentView(view)
        // 操作中に画面が消えないようにする（接続画面だけ消灯抑止）
        window.addFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        view.loadUrl(url)
    }
    override fun onResume() {
        super.onResume()
        // 前面復帰でWebViewの処理を再開。ページ側はvisibilitychangeで配信を張り直す。
        browser?.onResume()
    }
    override fun onPause() {
        // 裏に回った間の余計な処理を止める（配信自体は復帰時に再接続される）
        browser?.onPause()
        super.onPause()
    }
    private fun sameOrigin(url: Uri): Boolean {
        val approved = entry ?: return false
        return url.scheme in listOf("http", "https") && url.scheme == approved.scheme &&
            url.host == approved.host && effectivePort(url) == effectivePort(approved) && url.userInfo == null
    }
    // shutLogin 成功後の ?t= への遷移を許可するため、クエリ一致は要求しない。
    // 同一オリジンのトップページだけが遷移先。
    private fun allowedNavigation(url: Uri): Boolean = sameOrigin(url) &&
        url.path in listOf("", "/") && url.fragment == null
    private fun effectivePort(url: Uri): Int = if (url.port != -1) url.port else if (url.scheme == "https") 443 else 80
    private fun blocked() = WebResourceResponse("text/plain", "UTF-8", 403, "Blocked", emptyMap(), ByteArrayInputStream(ByteArray(0)))
    private fun fail(message: String) {
        runOnUiThread {
            if (failed || isFinishing) return@runOnUiThread
            failed = true
            browser?.stopLoading()
            // Suppress default WebView error pages, which can show the token URL.
            browser?.visibility = android.view.View.INVISIBLE
            AlertDialog.Builder(this).setTitle("接続できませんでした").setMessage(message)
                .setPositiveButton("戻る") { _, _ -> finish() }.setOnCancelListener { finish() }.show()
        }
    }
    @Deprecated("Activity result API")
    override fun onActivityResult(requestCode: Int, resultCode: Int, data: Intent?) {
        if (requestCode == REQ_FILE) {
            fileCallback?.onReceiveValue(WebChromeClient.FileChooserParams.parseResult(resultCode, data))
            fileCallback = null
            return
        }
        @Suppress("DEPRECATION")
        super.onActivityResult(requestCode, resultCode, data)
    }
    override fun onSaveInstanceState(outState: Bundle) {
        // No WebView state or token URL is saved into a Bundle.
    }
    override fun onDestroy() {
        browser?.apply {
            stopLoading(); clearHistory(); clearCache(true)
            (parent as? android.view.ViewGroup)?.removeView(this)
            removeAllViews(); destroy()
        }
        browser = null; entry = null; pendingPassword = null; ConnectionSession.pending = null
        fileCallback?.onReceiveValue(null); fileCallback = null
        window.clearFlags(WindowManager.LayoutParams.FLAG_KEEP_SCREEN_ON)
        super.onDestroy()
    }
    private companion object { const val REQ_FILE = 41 }
}
