package dev.myday.alarm

import android.app.AlarmManager
import android.app.PendingIntent
import android.content.Context
import android.content.Intent
import android.os.Build
import org.json.JSONObject

/**
 * AlarmManager 调度 + SharedPreferences 持久化。
 *
 * 持久化的意义：开机重排（[MydayAlarmBootReceiver]，PendingIntent 不跨重启）。
 * 序列化存 JSON（字段与 [AlarmSpecArg] 一致），upsert 前比对，内容没变不重设，
 * 避免每 30 秒一轮的同步白白唤醒 RTL/驱动。
 */
object AlarmScheduler {
  private const val PREFS = "myday_alarms"
  const val ACTION_FIRE = "dev.myday.alarm.ACTION_FIRE"
  private const val WINDOW_FALLBACK_MS = 10L * 60_000

  fun sync(context: Context, args: SyncArgs) {
    val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    val am = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager

    val newKeys = args.alarms.map { it.key }.toSet()
    val toCancel = args.cancelKeys.toMutableSet()
    if (args.replace) {
      prefs.all.keys.forEach { if (it !in newKeys) toCancel.add(it) }
    }
    val editor = prefs.edit()
    for (key in toCancel) {
      cancel(context, am, key)
      editor.remove(key)
    }

    for (spec in args.alarms) {
      val json = toJson(spec)
      if (!args.replace && prefs.contains(spec.key) && prefs.getString(spec.key, null) == json) {
        continue // 已排定且内容没变，跳过
      }
      schedule(context, am, spec)
      editor.putString(spec.key, json)
    }
    editor.apply()
  }

  /** 开机重排：未触发的闹钟重新注册；已过时刻的直接清掉（App 打开后走补发摘要）。 */
  fun rearm(context: Context) {
    val prefs = context.getSharedPreferences(PREFS, Context.MODE_PRIVATE)
    val am = context.getSystemService(Context.ALARM_SERVICE) as AlarmManager
    val now = System.currentTimeMillis()
    val editor = prefs.edit()
    for ((key, raw) in prefs.all) {
      val json = raw as? String ?: continue
      val spec = fromJson(json) ?: continue
      if (spec.at <= now) {
        editor.remove(key)
        continue
      }
      schedule(context, am, spec)
    }
    editor.apply()
  }

  private fun fireIntent(context: Context, spec: AlarmSpecArg): Intent =
      // 必须显式组件：接收器没有 intent-filter，action+package 的隐式包广播
      // 解析不到它（真机踩过：调度成功、触发无声无息蒸发）
      Intent(context, MydayAlarmReceiver::class.java)
          .setAction(ACTION_FIRE)
          .putExtra("key", spec.key)
          .putExtra("kind", spec.kind)
          .putExtra("at", spec.at)
          .putExtra("itemId", spec.itemId)
          .putExtra("title", spec.title)
          .putExtra("body", spec.body)

  private fun firePendingIntent(context: Context, spec: AlarmSpecArg): PendingIntent =
      PendingIntent.getBroadcast(
          context,
          spec.key.hashCode(),
          fireIntent(context, spec),
          PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)

  private fun schedule(context: Context, am: AlarmManager, spec: AlarmSpecArg) {
    val fire = firePendingIntent(context, spec)
    try {
      if (spec.kind == "alarm") {
        // setAlarmClock：面向「用户设置的闹钟」，Doze 下照常触发；代价是状态栏
        // 出现闹钟图标（对闹钟档来说这正是反馈）。14+ 同样受精确闹钟授权检查
        val launch = context.packageManager.getLaunchIntentForPackage(context.packageName)
        val show = launch?.let {
          PendingIntent.getActivity(
              context,
              spec.key.hashCode(),
              it,
              PendingIntent.FLAG_UPDATE_CURRENT or PendingIntent.FLAG_IMMUTABLE)
        }
        am.setAlarmClock(AlarmManager.AlarmClockInfo(spec.at, show), fire)
      } else {
        val canExact =
            Build.VERSION.SDK_INT < Build.VERSION_CODES.S || am.canScheduleExactAlarms()
        if (canExact) {
          am.setExactAndAllowWhileIdle(AlarmManager.RTC_WAKEUP, spec.at, fire)
        } else {
          // 未授予精确闹钟：±10 分钟窗口内送达（Doze 仍可触发）
          am.setWindow(AlarmManager.RTC_WAKEUP, spec.at, WINDOW_FALLBACK_MS, fire)
        }
      }
    } catch (e: SecurityException) {
      // 精确闹钟授权缺失（12+ 未开「闹钟和提醒」）：单条降级 ±10 分钟窗口，
      // 不拖垮整批——没有授权时宁可时间粗一点，也不能一条失败全部哑火
      android.util.Log.w("MydayAlarm", "exact alarm denied, falling back to window: ${spec.key}")
      am.setWindow(AlarmManager.RTC_WAKEUP, spec.at, WINDOW_FALLBACK_MS, fire)
    }
  }

  private fun cancel(context: Context, am: AlarmManager, key: String) {
    // filterEquals 含 component：须与 schedule 用的显式组件一致才能命中
    val intent = Intent(context, MydayAlarmReceiver::class.java).setAction(ACTION_FIRE)
    val pi =
        PendingIntent.getBroadcast(
            context,
            key.hashCode(),
            intent,
            PendingIntent.FLAG_NO_CREATE or PendingIntent.FLAG_IMMUTABLE)
    if (pi != null) {
      am.cancel(pi)
      pi.cancel()
    }
  }

  private fun toJson(spec: AlarmSpecArg): String =
      JSONObject()
          .put("key", spec.key)
          .put("rid", spec.rid)
          .put("at", spec.at)
          .put("kind", spec.kind)
          .put("itemId", spec.itemId)
          .put("title", spec.title)
          .put("body", spec.body)
          .toString()
  private fun fromJson(json: String): AlarmSpecArg? =
      try {
        val o = JSONObject(json)
        AlarmSpecArg().apply {
          key = o.getString("key")
          rid = o.getLong("rid")
          at = o.getLong("at")
          kind = o.getString("kind")
          itemId = o.getString("itemId")
          title = o.getString("title")
          body = o.getString("body")
        }
      } catch (_: Exception) {
        null
      }
}
