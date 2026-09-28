const COMMANDS: &[&str] = &[
    "sync_alarms",
    "alarm_permissions",
    "open_exact_alarm_settings",
    "open_app_details_settings",
];

fn main() {
    let result = tauri_plugin::Builder::new(COMMANDS)
        .android_path("android")
        .try_build();

    // when building documentation for Android the plugin build result is always Err() and is irrelevant to the crate documentation build
    if !(cfg!(docsrs) && std::env::var("TARGET").unwrap().contains("android")) {
        result.unwrap();
    }
}
