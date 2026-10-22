package jp.shut.bridge

import android.app.Activity
import android.content.Context
import android.content.Intent
import android.webkit.WebView
import com.google.android.gms.common.ConnectionResult
import com.google.android.gms.common.GoogleApiAvailability
import com.google.mlkit.vision.barcode.common.Barcode
import com.google.mlkit.vision.codescanner.GmsBarcodeScannerOptions
import com.google.mlkit.vision.codescanner.GmsBarcodeScanning

/** Android SDK/Play services glue. Rust owns connection validation. */
class NativeBridge(private val activity: Activity) {
    private var result: String = ""
    init { activity.runOnUiThread { WebView.setWebContentsDebuggingEnabled(false) } }
    @Synchronized private fun complete(value: String) { result = value }
    @Synchronized fun takeResult(): String {
        val value = result
        result = ""
        return value
    }
    fun startScan() {
        activity.runOnUiThread {
            if (GoogleApiAvailability.getInstance().isGooglePlayServicesAvailable(activity) != ConnectionResult.SUCCESS) {
                complete("error:QRを読むにはGoogle Play 開発者サービスが必要です。更新するか、接続URLを入力してください。")
                return@runOnUiThread
            }
            val options = GmsBarcodeScannerOptions.Builder().setBarcodeFormats(Barcode.FORMAT_QR_CODE).enableAutoZoom().build()
            try {
                GmsBarcodeScanning.getClient(activity, options).startScan()
                    .addOnSuccessListener { barcode ->
                        val value = barcode.rawValue
                        if (value.isNullOrBlank()) complete("error:接続URLを読み取れませんでした。もう一度試すか、URLを入力してください。")
                        else complete("scan:$value")
                    }
                    .addOnCanceledListener { complete("cancelled") }
                    .addOnFailureListener {
                        complete("error:QRを読み取れませんでした。初回はインターネットに接続してから再度試してください。接続URLの入力も利用できます。")
                    }
            } catch (_: Exception) {
                complete("error:この端末ではQR読み取りを開始できません。接続URLを入力してください。")
            }
        }
    }
    fun openConnection(url: String) {
        activity.runOnUiThread {
            try {
                // The URL never travels in an Intent or Bundle: only a static
                // hand-off plus the app-private recents store for reconnects.
                ConnectionSession.pending = url
                activity.startActivity(Intent(activity, RemoteActivity::class.java))
                // 起動できた時点で履歴化する（クラッシュ・強制終了後の再接続用）
                ConnectionSession.saveRecent(activity, url)
            } catch (_: Exception) {
                ConnectionSession.pending = null
                complete("error:接続画面を開けませんでした。アプリを開き直してください。")
            }
        }
    }
    fun saveRecent(url: String) = ConnectionSession.saveRecent(activity, url)
    fun loadRecent(): String = ConnectionSession.recents(activity).joinToString("\n")
    fun clearRecent() = ConnectionSession.clearRecents(activity)
}
internal object ConnectionSession {
    var pending: String? = null
    fun take(): String? { val value = pending; pending = null; return value }

    // クラッシュ・プロセス再生成後のワンタップ再接続用に、直近の接続URLを
    // アプリ専用のSharedPreferencesに保持する（最大3件、最新順）。
    private const val PREFS = "shut_recent"
    private const val KEY = "recent_urls"
    private const val LIMIT = 3

    fun saveRecent(context: Context, url: String) {
        val next = (listOf(url) + recents(context).filter { it != url }).take(LIMIT)
        // commit(): force-stop/クラッシュで apply() の非同期書き込みが失われるのを防ぐ
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .edit().putString(KEY, next.joinToString("\n")).commit()
    }
    fun recents(context: Context): List<String> =
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
            .getString(KEY, "")?.split("\n")?.filter { it.isNotBlank() } ?: emptyList()
    fun clearRecents(context: Context) {
        context.getSharedPreferences(PREFS, Context.MODE_PRIVATE).edit().remove(KEY).commit()
    }
}
