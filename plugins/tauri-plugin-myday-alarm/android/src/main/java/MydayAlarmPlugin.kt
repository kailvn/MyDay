package dev.myday.alarm

import android.app.Activity
import android.app.AlarmManager
import android.app.NotificationManager
import android.content.Context
import android.content.Intent
import android.net.Uri
import android.os.Build
import app.tauri.annotation.Command
import app.tauri.annotation.InvokeArg
import app.tauri.annotation.TauriPlugin
import app.tauri.plugin.Invoke
import app.tauri.plugin.JSObject
import app.tauri.plugin.Plugin

/** 一条应调度的提醒（Rust 提醒循环算出，前端转发；字段 camelCase 对齐）。 */
@InvokeArg
class AlarmSpecArg {
  lateinit var key: String
  var rid: Long = 0
  var at: Long = 0
  lateinit var kind: String
  lateinit var itemId: String
  lateinit var title: String
  lateinit var body: String
}

@InvokeArg
class SyncArgs {
  /** true = 全量对账：存储里不在本次集合中的闹钟一律取消（App 冷启动拉取用）。 */
  var replace: Boolean = false
  var alarms: List<AlarmSpecArg> = emptyList()
  var cancelKeys: List<String> = emptyList()
}

/**
 * MyDay 系统闹钟移交插件。
 *
 * Rust 侧不做调度（进程会被冻结），本插件把提醒写进 AlarmManager：
 * - kind = alarm：setAlarmClock，到点全屏闹铃 + 循环铃声；
 * - kind = notify：setExactAndAllowWhileIdle（无精确权限时降级 setWindow），
 *   到点普通系统通知。
 * 到点统一由 [MydayAlarmReceiver] 发通知，与 App 进程死活无关。
 */
@TauriPlugin
class MydayAlarmPlugin(private val activity: Activity) : Plugin(activity) {
  @Command
  fun syncAlarms(invoke: Invoke) {
    try {
      val args = invoke.parseArgs(SyncArgs::class.java)
      AlarmScheduler.sync(activity.applicationContext, args)
      invoke.resolve()
    } catch (e: Throwable) {
      // 异常细节进 logcat（tag MydayAlarm），同时回传 JS 侧
      android.util.Log.e("MydayAlarm", "syncAlarms failed", e)
      invoke.reject(e.message ?: e.toString())
    }
  }

  /** 权限自检：exact = 精确闹钟（12+ 需授权）；fullScreen = 全屏意图（14+ 默认拒绝）。 */
  @Command
  fun alarmPermissions(invoke: Invoke) {
    val ctx = activity.applicationContext
    val am = ctx.getSystemService(Context.ALARM_SERVICE) as AlarmManager
    val exact = Build.VERSION.SDK_INT < Build.VERSION_CODES.S || am.canScheduleExactAlarms()
    val nm = ctx.getSystemService(Context.NOTIFICATION_SERVICE) as NotificationManager
    val fullScreen =
        Build.VERSION.SDK_INT < Build.VERSION_CODES.TIRAMISU || nm.canUseFullScreenIntent()
    val result = JSObject().apply {
      put("exact", exact)
      put("fullScreen", fullScreen)
    }
    invoke.resolve(result)
  }

  /** 跳系统「闹钟和提醒」（精确闹钟）授权页（31+；低版本无需授权直接 resolve）。 */
  @Command
  fun openExactAlarmSettings(invoke: Invoke) {
    if (Build.VERSION.SDK_INT >= Build.VERSION_CODES.S) {
      val i = Intent(android.provider.Settings.ACTION_REQUEST_SCHEDULE_EXACT_ALARM)
          .setData(Uri.parse("package:${activity.packageName}"))
          .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
      activity.startActivity(i)
    }
    invoke.resolve()
  }

  /** 跳系统应用详情页（含「闹钟和提醒」/全屏意图、通知等开关）。 */
  @Command
  fun openAppDetailsSettings(invoke: Invoke) {
    val i = Intent(android.provider.Settings.ACTION_APPLICATION_DETAILS_SETTINGS)
        .setData(Uri.parse("package:${activity.packageName}"))
        .addFlags(Intent.FLAG_ACTIVITY_NEW_TASK)
    activity.startActivity(i)
    invoke.resolve()
  }
}
