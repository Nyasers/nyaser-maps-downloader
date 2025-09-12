// 日志工具模块
// 此模块包含所有日志相关的辅助函数和宏定义
#[cfg(debug_assertions)]
use chrono::DateTime;
#[cfg(debug_assertions)]
use std::time::{Duration, SystemTime};

/// 辅助函数：获取当前时间的格式化字符串
#[cfg(debug_assertions)]
pub fn get_current_time() -> String {
    let now = SystemTime::now();
    let since_epoch = now
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or(Duration::from_secs(0));
    let time = DateTime::from_timestamp(since_epoch.as_secs() as i64, 0)
        .unwrap_or_else(|| DateTime::from_timestamp(0, 0).unwrap());
    time.format("%Y-%m-%d %H:%M:%S.%3f").to_string()
}

/// 记录日志的辅助函数
pub fn log_message(level: &str, message: &str) {
    #[cfg(debug_assertions)]
    {
        let timestamp = get_current_time();
        println!("[{}] [{}] {}", timestamp, level, message);
    }
    #[cfg(not(debug_assertions))]
    {
        _ = (level, message);
    }
}

/// 日志宏定义 - 信息级别
#[macro_export]
macro_rules! log_info {
    ($($arg:tt)*) => { $crate::log_utils::log_message("INFO", &format!($($arg)*)); };
}

/// 日志宏定义 - 警告级别
#[macro_export]
macro_rules! log_warn {
    ($($arg:tt)*) => { $crate::log_utils::log_message("WARN", &format!($($arg)*)) };
}

/// 日志宏定义 - 错误级别
#[macro_export]
macro_rules! log_error {
    ($($arg:tt)*) => { $crate::log_utils::log_message("ERROR", &format!($($arg)*)) };
}

/// 日志宏定义 - 调试级别
#[macro_export]
macro_rules! log_debug {
    ($($arg:tt)*) => { $crate::log_utils::log_message("DEBUG", &format!($($arg)*)) };
}
