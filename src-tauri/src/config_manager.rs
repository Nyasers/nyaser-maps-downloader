// config_manager.rs 模块 - 处理用户配置的读写操作

use serde_json::{json, Value};
use std::fs;
use tauri::{AppHandle, Manager};

// 导入update_window_title函数
use crate::init::update_window_title;

/// 读取用户配置
/// 
/// # 参数
/// - `app_handle`: Tauri应用句柄，用于获取应用数据目录
/// - `config_name`: 配置文件名（不包含路径）
/// 
/// # 返回值
/// - 成功时返回包含配置内容的Ok(Value)
/// - 失败时返回包含错误信息的Err(String)
#[tauri::command]
pub fn read_config(app_handle: AppHandle, config_name: &str) -> Result<Value, String> {
    // 获取应用配置目录
    let config_dir = match app_handle.path().app_config_dir() {
        Ok(path) => path,
        Err(e) => {
            return Err(format!("无法获取应用配置目录: {:?}", e));
        }
    };

    // 构建完整的配置文件路径
    let config_path = config_dir.join(config_name);

    // 检查文件是否存在
    if !config_path.exists() {
        // 如果文件不存在，返回空的JSON对象
        return Ok(json!({}));
    }

    // 读取文件内容
    let content = match fs::read_to_string(&config_path) {
        Ok(content) => content,
        Err(e) => {
            return Err(format!("无法读取配置文件: {:?}", e));
        }
    };

    // 解析JSON内容
    let config: Value = match serde_json::from_str(&content) {
        Ok(config) => config,
        Err(e) => {
            return Err(format!("无法解析配置文件: {:?}", e));
        }
    };

    Ok(config)
}

/// 写入用户配置
/// 
/// # 参数
/// - `app_handle`: Tauri应用句柄，用于获取应用数据目录
/// - `config_name`: 配置文件名（不包含路径）
/// - `config`: 要写入的配置内容
/// 
/// # 返回值
/// - 成功时返回包含成功信息的Ok(String)
/// - 失败时返回包含错误信息的Err(String)
#[tauri::command]
pub fn write_config(app_handle: AppHandle, config_name: &str, config: Value) -> Result<String, String> {
    // 获取应用配置目录
    let config_dir = match app_handle.path().app_config_dir() {
        Ok(path) => path,
        Err(e) => {
            return Err(format!("无法获取应用配置目录: {:?}", e));
        }
    };

    // 确保配置目录存在
    if !config_dir.exists() {
        match fs::create_dir_all(&config_dir) {
            Ok(_) => {},
            Err(e) => {
                return Err(format!("无法创建配置目录: {:?}", e));
            }
        }
    }

    // 构建完整的配置文件路径
    let config_path = config_dir.join(config_name);

    // 将配置转换为格式化的JSON字符串
    let content = match serde_json::to_string_pretty(&config) {
        Ok(content) => content,
        Err(e) => {
            return Err(format!("无法序列化配置: {:?}", e));
        }
    };

    // 写入文件
    match fs::write(&config_path, content) {
        Ok(_) => {
            // 如果是config.json，且包含nmd_data，更新窗口标题
            if config_name == "config.json" {
                if let Some(nmd_data) = config.get("nmd_data") {
                    if let Some(data_dir) = nmd_data.as_str() {
                        // 更新窗口标题
                        update_window_title(&app_handle, data_dir);
                    }
                }
            }
            Ok(format!("配置已成功写入: {:?}", config_path))
        },
        Err(e) => {
            Err(format!("无法写入配置文件: {:?}", e))
        }
    }
}

/// 删除用户配置
/// 
/// # 参数
/// - `app_handle`: Tauri应用句柄，用于获取应用数据目录
/// - `config_name`: 配置文件名（不包含路径）
/// 
/// # 返回值
/// - 成功时返回包含成功信息的Ok(String)
/// - 失败时返回包含错误信息的Err(String)
#[tauri::command]
pub fn delete_config(app_handle: AppHandle, config_name: &str) -> Result<String, String> {
    // 获取应用配置目录
    let config_dir = match app_handle.path().app_config_dir() {
        Ok(path) => path,
        Err(e) => {
            return Err(format!("无法获取应用配置目录: {:?}", e));
        }
    };

    // 构建完整的配置文件路径
    let config_path = config_dir.join(config_name);

    // 检查文件是否存在
    if !config_path.exists() {
        return Ok(format!("配置文件不存在: {:?}", config_path));
    }

    // 删除文件
    match fs::remove_file(&config_path) {
        Ok(_) => {
            Ok(format!("配置已成功删除: {:?}", config_path))
        },
        Err(e) => {
            Err(format!("无法删除配置文件: {:?}", e))
        }
    }
}

/// 获取数据存储目录
/// 
/// # 参数
/// - `app_handle`: Tauri应用句柄，用于获取应用数据目录
/// 
/// # 返回值
/// - 成功时返回包含数据存储目录的Ok(Option<String>)
/// - 失败时返回包含错误信息的Err(String)
#[tauri::command]
pub fn get_data_dir(app_handle: AppHandle) -> Result<Option<String>, String> {
    // 读取配置文件
    let config = read_config(app_handle, "config.json")?;
    
    // 检查是否存在nmd_data配置
    if let Some(nmd_data) = config.get("nmd_data") {
        if let Some(dir_str) = nmd_data.as_str() {
            return Ok(Some(dir_str.to_string()));
        }
    }
    
    Ok(None)
}
