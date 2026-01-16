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
pub fn write_config(
    app_handle: AppHandle,
    config_name: &str,
    config: Value,
) -> Result<String, String> {
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
            Ok(_) => {}
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
            // 如果是config.json，且包含nmd_data，更新窗口标题并初始化资源
            if config_name == "config.json" {
                if let Some(nmd_data) = config.get("nmd_data") {
                    if let Some(data_dir) = nmd_data.as_str() {
                        // 更新窗口标题
                        update_window_title(&app_handle, data_dir);

                        // 获取当前的 addons_dir（如果已设置）
                        let current_addons_dir = {
                            let guard = crate::dir_manager::DIR_MANAGER.lock().unwrap();
                            guard.as_ref().and_then(|dm| dm.addons_dir().cloned())
                        };

                        // 重新初始化目录管理器
                        let mut dir_manager = match crate::dir_manager::DirManager::with_nmd_data_dir(
                            std::path::PathBuf::from(data_dir),
                        ) {
                            Ok(dm) => dm,
                            Err(e) => {
                                crate::log_error!("重新初始化目录管理器失败: {}", e);
                                return Err(format!("重新初始化目录管理器失败: {}", e));
                            }
                        };

                        // 如果之前有设置 addons_dir，重新设置回去
                        if let Some(addons_dir) = current_addons_dir {
                            dir_manager.set_addons_dir(addons_dir);
                        }

                        // 更新全局目录管理器
                        {
                            let mut guard = crate::dir_manager::DIR_MANAGER.lock().unwrap();
                            *guard = Some(dir_manager);
                        }

                        // 初始化aria2c后端
                        if let Err(e) = crate::aria2c::initialize_aria2c_backend() {
                            crate::log_error!("初始化aria2c后端失败: {}", e);
                            return Err(format!("初始化aria2c后端失败: {}", e));
                        }

                        // 初始化7z资源
                        crate::log_info!("开始初始化 7z 资源...");
                        crate::extract_manager::initialize_7z_resources();
                        crate::log_info!("7z 资源初始化完成");
                    }
                }
            }
            Ok(format!("配置已成功写入: {:?}", config_path))
        }
        Err(e) => Err(format!("无法写入配置文件: {:?}", e)),
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
        Ok(_) => Ok(format!("配置已成功删除: {:?}", config_path)),
        Err(e) => Err(format!("无法删除配置文件: {:?}", e)),
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
