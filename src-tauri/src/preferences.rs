//! 应用级偏好设置（对应「设置 → 高级」与「设置 → 通用」里的几个开关）。
//!
//! 修复前的状况：日志等级、上游请求超时、「关闭窗口最小化到托盘」这三个控件
//! 只往 localStorage 写值，后端一行都不读——超时恒定 180 秒、日志恒定 info、
//! 关闭窗口永远隐藏到托盘。界面看起来能配，实际全是摆设。
//!
//! 这里把它们统一落到 settings 表的一个 JSON 里（和 proxy_settings 同一套机制），
//! 再在真正用到的位置读出来：
//! - `request_timeout_secs` → `proxy::ProxyState::new` 构建 reqwest 客户端时；
//! - `log_level` → `run()` 初始化 tracing 订阅器时（进程级，改完要重启）；
//! - `minimize_to_tray` → `on_window_event` 决定关闭窗口是隐藏还是真退出。

use crate::database::Database;
use serde::{Deserialize, Serialize};
use std::time::Duration;
use tauri::State;

const APP_PREFERENCES_KEY: &str = "app_preferences";

/// 请求超时允许的范围。下限 15 秒避免用户把自己卡死在重试里，
/// 上限 600 秒是因为长上下文 + 高推理强度的请求确实可能跑几分钟。
const MIN_TIMEOUT_SECS: u64 = 15;
const MAX_TIMEOUT_SECS: u64 = 600;

/// tracing 能接受的等级，保存时做白名单校验，
/// 免得把非法字符串写进去导致下次启动 EnvFilter 解析失败。
const LOG_LEVELS: [&str; 4] = ["debug", "info", "warn", "error"];

fn default_minimize_to_tray() -> bool {
    true
}

fn default_request_timeout_secs() -> u64 {
    180
}

fn default_log_level() -> String {
    "info".to_string()
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AppPreferences {
    // 三个字段都带 serde 默认值：旧版本写下的 JSON 少字段时要能继续读，
    // 不能因为加了一个开关就把用户已有的偏好整块判为"损坏"。
    #[serde(default = "default_minimize_to_tray")]
    pub minimize_to_tray: bool,
    #[serde(default = "default_request_timeout_secs")]
    pub request_timeout_secs: u64,
    #[serde(default = "default_log_level")]
    pub log_level: String,
}

impl Default for AppPreferences {
    fn default() -> Self {
        Self {
            minimize_to_tray: default_minimize_to_tray(),
            request_timeout_secs: default_request_timeout_secs(),
            log_level: default_log_level(),
        }
    }
}

impl AppPreferences {
    fn validate(&self) -> Result<(), String> {
        if !(MIN_TIMEOUT_SECS..=MAX_TIMEOUT_SECS).contains(&self.request_timeout_secs) {
            return Err(format!(
                "请求超时必须在 {MIN_TIMEOUT_SECS} 到 {MAX_TIMEOUT_SECS} 秒之间"
            ));
        }
        if !LOG_LEVELS.contains(&self.log_level.as_str()) {
            return Err(format!("未知的日志等级：{}", self.log_level));
        }
        Ok(())
    }

    /// 上游整体请求超时。连接超时保持固定 10 秒——那属于网络可达性，
    /// 调大它只会让"连不上"这件事晚几秒才报出来。
    pub fn request_timeout(&self) -> Duration {
        Duration::from_secs(self.request_timeout_secs)
    }
}

/// 读取偏好。缺失、损坏、甚至数据库读失败都回退默认值：
/// 一个设置项不该有能力让代理起不来。
pub fn load(database: &Database) -> AppPreferences {
    match database.get_setting(APP_PREFERENCES_KEY) {
        Ok(Some(value)) => serde_json::from_str(&value).unwrap_or_else(|error| {
            tracing::warn!(%error, "app preferences corrupted; falling back to defaults");
            AppPreferences::default()
        }),
        Ok(None) => AppPreferences::default(),
        Err(error) => {
            tracing::warn!(%error, "failed to read app preferences; falling back to defaults");
            AppPreferences::default()
        }
    }
}

pub fn save(database: &Database, preferences: &AppPreferences) -> Result<(), String> {
    preferences.validate()?;
    let value = serde_json::to_string(preferences).map_err(|error| error.to_string())?;
    database.set_setting(APP_PREFERENCES_KEY, &value)
}

#[tauri::command]
pub fn get_app_preferences(db: State<'_, Database>) -> Result<AppPreferences, String> {
    Ok(load(&db))
}

#[tauri::command]
pub fn save_app_preferences(
    preferences: AppPreferences,
    db: State<'_, Database>,
) -> Result<AppPreferences, String> {
    save(&db, &preferences)?;
    Ok(load(&db))
}

/// 开机自启。以前「通用」页那个开关只写 localStorage，一点用都没有。
///
/// 这里刻意不引入 tauri-plugin-autostart：注册表 Run 项用系统自带的 reg.exe
/// 就能读写，不增加依赖，用户在任务管理器「启动」页里也能自己看到和禁用。
/// 真实状态以注册表为准，不写进 [`AppPreferences`]——否则用户在系统里手动
/// 关掉之后，界面还会一直显示"已开启"。
#[cfg(windows)]
mod autostart {
    use std::os::windows::process::CommandExt;
    use std::process::{Command, Output};

    const RUN_KEY: &str = r"HKCU\Software\Microsoft\Windows\CurrentVersion\Run";
    const VALUE_NAME: &str = "XY Switch";
    /// CREATE_NO_WINDOW。不加这个标志，每次开关都会闪一下黑色控制台窗口。
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    fn reg(args: &[&str]) -> Result<Output, String> {
        Command::new("reg")
            .args(args)
            .creation_flags(CREATE_NO_WINDOW)
            .output()
            .map_err(|error| format!("无法调用 reg.exe：{error}"))
    }

    /// reg.exe 的输出在中文系统上是 OEM 代码页，直接转成 UTF-8 会变乱码，
    /// 所以只报退出码，不把它的原文塞给用户看。
    fn failed(action: &str, output: &Output) -> String {
        format!(
            "{action}失败（reg.exe 退出码 {}）",
            output.status.code().unwrap_or(-1)
        )
    }

    pub fn is_enabled() -> bool {
        reg(&["query", RUN_KEY, "/v", VALUE_NAME])
            .map(|output| output.status.success())
            .unwrap_or(false)
    }

    pub fn set_enabled(enabled: bool) -> Result<(), String> {
        if !enabled {
            let output = reg(&["delete", RUN_KEY, "/v", VALUE_NAME, "/f"])?;
            // 本来就没这条值时 reg delete 也会返回非零，
            // 只要最终状态是"已关闭"就不算失败。
            if !output.status.success() && is_enabled() {
                return Err(failed("删除开机自启项", &output));
            }
            return Ok(());
        }

        let exe = std::env::current_exe().map_err(|error| format!("无法定位程序路径：{error}"))?;
        let exe = exe.to_string_lossy().into_owned();
        if exe.contains('"') {
            return Err("程序路径包含引号，无法写入开机自启项".to_string());
        }
        // 路径里大概率有空格，必须自己补引号；--minimized 让开机时只驻留托盘。
        let command = format!("\"{exe}\" --minimized");
        let output = reg(&[
            "add",
            RUN_KEY,
            "/v",
            VALUE_NAME,
            "/t",
            "REG_SZ",
            "/d",
            &command,
            "/f",
        ])?;
        if !output.status.success() {
            return Err(failed("写入开机自启项", &output));
        }
        Ok(())
    }
}

#[cfg(not(windows))]
mod autostart {
    pub fn is_enabled() -> bool {
        false
    }
    pub fn set_enabled(_: bool) -> Result<(), String> {
        Err("开机自启目前只支持 Windows".to_string())
    }
}

/// 开机自启启动参数。命中时主窗口不显示，只在托盘驻留。
pub const START_MINIMIZED_FLAG: &str = "--minimized";

/// 本次进程是否由开机自启拉起（或用户手动带了 `--minimized`）。
pub fn started_minimized() -> bool {
    std::env::args().any(|arg| arg == START_MINIMIZED_FLAG)
}

#[tauri::command]
pub fn get_autostart_enabled() -> bool {
    autostart::is_enabled()
}

#[tauri::command]
pub fn set_autostart_enabled(enabled: bool) -> Result<bool, String> {
    autostart::set_enabled(enabled)?;
    // 回读一次再返回：写成功但被安全软件拦掉的情况下界面不该显示成已开启。
    Ok(autostart::is_enabled())
}

#[cfg(test)]
mod tests {
    use super::{AppPreferences, load, save};
    use crate::database::Database;

    fn test_database() -> (Database, std::path::PathBuf) {
        let path = std::env::temp_dir().join(format!("xy-switch-prefs-{}.db", uuid::Uuid::new_v4()));
        (Database::open(&path).unwrap(), path)
    }

    #[test]
    fn defaults_match_the_previously_hardcoded_behaviour() {
        let preferences = AppPreferences::default();
        // 默认值必须和修复前写死的行为一致，升级后的用户不该感觉到变化。
        assert!(preferences.minimize_to_tray);
        assert_eq!(preferences.request_timeout_secs, 180);
        assert_eq!(preferences.log_level, "info");
        assert_eq!(preferences.request_timeout().as_secs(), 180);
    }

    #[test]
    fn round_trips_through_the_settings_table() {
        let (database, path) = test_database();
        // 没写过任何值时读到的就是默认值，不能报错。
        assert_eq!(load(&database), AppPreferences::default());

        let wanted = AppPreferences {
            minimize_to_tray: false,
            request_timeout_secs: 60,
            log_level: "debug".to_string(),
        };
        save(&database, &wanted).unwrap();
        assert_eq!(load(&database), wanted);

        drop(database);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn rejects_out_of_range_timeout_and_unknown_log_level() {
        let (database, path) = test_database();
        let too_short = AppPreferences {
            request_timeout_secs: 1,
            ..AppPreferences::default()
        };
        assert!(save(&database, &too_short).unwrap_err().contains("超时"));

        let too_long = AppPreferences {
            request_timeout_secs: 9_999,
            ..AppPreferences::default()
        };
        assert!(save(&database, &too_long).unwrap_err().contains("超时"));

        let bogus_level = AppPreferences {
            log_level: "trace; DROP TABLE settings".to_string(),
            ..AppPreferences::default()
        };
        assert!(save(&database, &bogus_level).unwrap_err().contains("日志"));

        // 校验失败不能留下半截数据。
        assert_eq!(load(&database), AppPreferences::default());

        drop(database);
        std::fs::remove_file(path).unwrap();
    }

    #[test]
    fn corrupted_json_falls_back_to_defaults() {
        let (database, path) = test_database();
        database
            .set_setting("app_preferences", "{ not json at all")
            .unwrap();
        assert_eq!(load(&database), AppPreferences::default());

        // 缺字段的旧版本数据也要能读：serde 默认值补齐。
        database
            .set_setting("app_preferences", r#"{"logLevel":"warn"}"#)
            .unwrap();
        let loaded = load(&database);
        assert_eq!(loaded.log_level, "warn");
        assert_eq!(loaded.request_timeout_secs, 180);
        assert!(loaded.minimize_to_tray);

        drop(database);
        std::fs::remove_file(path).unwrap();
    }
}
