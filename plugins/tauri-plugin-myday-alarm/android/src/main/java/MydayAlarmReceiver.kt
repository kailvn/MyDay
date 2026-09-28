package dev.myday.alarm

import android.app.Notification
import android.app.NotificationChannel
import android.app.NotificationManager
import android.app.PendingIntent
import android.content.BroadcastReceiver
import android.content.Context
import android.content.Intent
import android.media.AudioAttributes
import android.media.MediaPlayer
import android.media.RingtoneManager
import android.os.Build
import android.os.Handler
import android.os.Looper
import android.os.VibrationEffect
import android.os.Vibrator
import androidx.core.app.NotificationCompat
import androidx.core.app.NotificationManagerCompat

const val CHANNEL_NOTIFY = "myday_notify"
/** v2：闹钟档渠道不带声音/震动——响铃与震动由接收器自己在 ALARM 流上执行，
 *  不受响铃模式 / 静音开关影响（渠道铃声会被部分 ROM 跟随通知静音，真机踩过）。
 *  渠道设置一经创建不可改，改语义只能换 id。 */
const val CHANNEL_ALARM = "myday_alarm_v2"
private const val CHANNEL_ALARM_LEGACY = "myday_alarm"

/** 通知渠道（首次使用时创建；alarm 档 IMPORTANCE_HIGH 保证横幅置顶展示）。 */
fun ensureChannels(context: Context, nm: NotificationManager) {
    if (Build.VERSION.SDK_INT < Build.VERSION_CODES.O) return
    val notify =
        NotificationChannel(CHANNEL_NOTIFY, "MyDay 提醒", NotificationManager.IMPORTANCE_DEFAULT)
    val alarm =
        NotificationChannel(CHANNEL_ALARM, "MyDay 闹钟", NotificationManager.IMPORTANCE_HIGH)
            .apply {
                setSound(null, null)
                enableVibration(false)
            }
    nm.createNotificationChannel(notify)
    nm.createNotificationChannel(alarm)
    // 清理旧版带渠道铃声的闹钟渠道（避免设置里残留死项）
    nm.deleteNotificationChannel(CHANNEL_ALARM_LEGACY)
}

/**
 * 闹钟到点接收器：发系统通知 + alarm 档自响铃，不经 App 进程常驻
 * （进程冻结 / 被杀也能触发）。
 *
 * - kind = alarm：IMPORTANCE_HIGH 横幅 + 全屏意图（授权时亮屏）+
 *   MediaPlayer 循环播放默认闹钟铃声（USAGE_ALARM，走闹钟音量流，
 *   静音/振动模式下照响）+ 震动，自响 15 秒后停止；
 * - kind = notify：普通通知，点击进 App。
 * 未授予通知权限（Android 13+）时 notify 被系统吞掉，这里静默降级。
 */
class MydayAlarmReceiver : BroadcastReceiver() {
    private var player: MediaPlayer? = null
    private var vibrator: Vibrator? = null

    override fun onReceive(context: Context, intent: Intent) {
        val key = intent.getStringExtra("key") ?: return
        val kind = intent.getStringExtra("kind") ?: "notify"
        val title = intent.getStringExtra("title") ?: "MyDay"
        val body = intent.getStringExtra("body") ?: ""
        // 送达延迟 = 实际触发 - 计划时刻。各 ROM 对后台触发有不同程度的拦截/合并，
        // 这个差值是判断「被系统拖慢了多少」的唯一通用信号（不做任何厂商假设）
        val scheduledAt = intent.getLongExtra("at", 0L)
        val delaySec = if (scheduledAt > 0) (System.currentTimeMillis() - scheduledAt) / 1000 else -1
        android.util.Log.i("MydayAlarm", "receiver fired: key=$key kind=$kind delay=${delaySec}s")

        val nm = context.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
        ensureChannels(context, nm)

        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)
        val contentPi =
            launch?.let {
                PendingIntent.getActivity(
                    context,
                    key.hashCode(),
                    it,
                    PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
            }

        val builder =
            NotificationCompat.Builder(
                context, if (kind == "alarm") CHANNEL_ALARM else CHANNEL_NOTIFY)
                .setSmallIcon(context.applicationInfo.icon)
                .setContentTitle(title)
                .setContentText(body)
                .setStyle(NotificationCompat.BigTextStyle().bigText(body))
                .setAutoCancel(true)
                .setContentIntent(contentPi)
        if (kind == "alarm") {
            builder.setPriority(NotificationCompat.PRIORITY_MAX)
                .setCategory(NotificationCompat.CATEGORY_ALARM)
                .setFullScreenIntent(contentPi, true)
        }

        val notification = builder.build()
        try {
            NotificationManagerCompat.from(context).notify(key.hashCode(), notification)
            android.util.Log.i(
                "MydayAlarm",
                "notification posted: key=$key icon=0x${context.applicationInfo.icon.toString(16)}")
        } catch (e: Exception) {
            android.util.Log.e("MydayAlarm", "notify failed: key=$key", e)
        }

        if (kind == "alarm") {
            // goAsync 让广播结果悬挂，接收器进程保活到自响铃结束（上限 15 秒）
            val pending = goAsync()
            ring(context, pending)
        }
    }

    /** alarm 档自响铃：默认闹钟铃声循环 + 震动，[RING_MS] 后自停。 */
    private fun ring(context: Context, pending: PendingResult) {
        try {
            val uri =
                RingtoneManager.getDefaultUri(RingtoneManager.TYPE_ALARM)
                    ?: RingtoneManager.getDefaultUri(RingtoneManager.TYPE_NOTIFICATION)
            if (uri != null) {
                player =
                    MediaPlayer().apply {
                        setDataSource(context, uri)
                        setAudioAttributes(
                            AudioAttributes.Builder()
                                .setUsage(AudioAttributes.USAGE_ALARM)
                                .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
                                .build())
                        isLooping = true
                        prepare()
                        start()
                    }
            }
            vibrator =
                context.getSystemService(Vibrator::class.java)?.apply {
                    vibrate(VibrationEffect.createWaveform(longArrayOf(0, 600, 400), 0))
                }
            android.util.Log.i("MydayAlarm", "ringing started (${RING_MS / 1000}s)")
        } catch (e: Exception) {
            android.util.Log.e("MydayAlarm", "ring failed", e)
        }
        Handler(Looper.getMainLooper()).postDelayed({
            stopRing()
            pending.finish()
        }, RING_MS)
    }

    private fun stopRing() {
        try {
            player?.stop()
            player?.release()
        } catch (_: Exception) {
        }
        player = null
        vibrator?.cancel()
        vibrator = null
    }

    companion object {
        private const val RING_MS = 15_000L
    }
}

/** 开机完成：PendingIntent 不跨重启，从持久化存储重排尚未触发的闹钟。 */
class MydayAlarmBootReceiver : BroadcastReceiver() {
    override fun onReceive(context: Context, intent: Intent) {
        when (intent.action) {
            Intent.ACTION_BOOT_COMPLETED,
            Intent.ACTION_LOCKED_BOOT_COMPLETED,
            "android.intent.action.QUICKBOOT_POWERON" -> AlarmScheduler.rearm(context)
        }
    }
}
