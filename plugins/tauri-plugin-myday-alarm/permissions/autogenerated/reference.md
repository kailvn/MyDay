## Default Permission

MyDay 系统闹钟移交：允许前端把提醒集合同步进系统 AlarmManager
（含取消）、查询闹钟相关授权状态并跳转系统授权页。
默认权限开放全部四条命令。

#### This default permission set includes the following:

- `allow-sync-alarms`
- `allow-alarm-permissions`
- `allow-open-exact-alarm-settings`
- `allow-open-app-details-settings`

## Permission Table

<table>
<tr>
<th>Identifier</th>
<th>Description</th>
</tr>


<tr>
<td>

`myday-alarm:allow-alarm-permissions`

</td>
<td>

Enables the alarm_permissions command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`myday-alarm:deny-alarm-permissions`

</td>
<td>

Denies the alarm_permissions command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`myday-alarm:allow-open-app-details-settings`

</td>
<td>

Enables the open_app_details_settings command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`myday-alarm:deny-open-app-details-settings`

</td>
<td>

Denies the open_app_details_settings command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`myday-alarm:allow-open-exact-alarm-settings`

</td>
<td>

Enables the open_exact_alarm_settings command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`myday-alarm:deny-open-exact-alarm-settings`

</td>
<td>

Denies the open_exact_alarm_settings command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`myday-alarm:allow-sync-alarms`

</td>
<td>

Enables the sync_alarms command without any pre-configured scope.

</td>
</tr>

<tr>
<td>

`myday-alarm:deny-sync-alarms`

</td>
<td>

Denies the sync_alarms command without any pre-configured scope.

</td>
</tr>
</table>
