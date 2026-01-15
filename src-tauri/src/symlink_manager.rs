use serde::{Deserialize, Serialize};
use std::{
    fs,
    path::Path,
};

use crate::{log_error, log_info, log_warn};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SymlinkInfo {
    pub name: String,
    pub path: String,
    pub target_path: String,
    pub target_exists: bool,
}

pub fn get_all_file_symlinks_in_dir(dir_path: &str) -> Result<Vec<SymlinkInfo>, String> {
    log_info!("开始扫描目录中的文件符号链接: {}", dir_path);

    let dir = Path::new(dir_path);

    if !dir.exists() {
        return Err(format!("目录不存在: {}", dir_path));
    }

    if !dir.is_dir() {
        return Err(format!("路径不是目录: {}", dir_path));
    }

    let mut symlinks = Vec::new();

    let entries = match fs::read_dir(dir) {
        Ok(entries) => entries,
        Err(e) => {
            log_error!("无法读取目录: {}, 错误: {:?}", dir_path, e);
            return Err(format!("无法读取目录: {:?}", e));
        }
    };

    for entry in entries {
        let entry = match entry {
            Ok(e) => e,
            Err(e) => {
                log_warn!("读取目录项失败: {:?}", e);
                continue;
            }
        };

        let path = entry.path();

        if !path.is_symlink() {
            continue;
        }

        let metadata = match fs::symlink_metadata(&path) {
            Ok(meta) => meta,
            Err(e) => {
                log_warn!("无法获取符号链接元数据: {:?}, 错误: {:?}", path, e);
                continue;
            }
        };

        if metadata.file_type().is_file() {
            let name = match path.file_name() {
                Some(n) => n.to_string_lossy().to_string(),
                None => {
                    log_warn!("无法获取符号链接文件名: {:?}", path);
                    continue;
                }
            };

            let path_str = path.to_string_lossy().to_string();

            let target_path = match fs::read_link(&path) {
                Ok(target) => {
                    let canonical_target = match fs::canonicalize(&target) {
                        Ok(p) => p.to_string_lossy().to_string(),
                        Err(_) => target.to_string_lossy().to_string(),
                    };
                    canonical_target
                }
                Err(e) => {
                    log_warn!("无法读取符号链接目标: {:?}, 错误: {:?}", path, e);
                    continue;
                }
            };

            let target_exists = Path::new(&target_path).exists();

            log_info!("找到文件符号链接: {}", path_str);

            symlinks.push(SymlinkInfo {
                name,
                path: path_str.clone(),
                target_path,
                target_exists,
            });
        }
    }

    log_info!("扫描完成，共找到 {} 个文件符号链接", symlinks.len());
    Ok(symlinks)
}

pub fn create_file_symlink(
    target_path: &str,
    link_dir: &str,
    link_name: &str,
) -> Result<String, String> {
    log_info!(
        "开始创建文件符号链接: 目标={}, 链接目录={}, 链接名称={}",
        target_path,
        link_dir,
        link_name
    );

    let target = Path::new(target_path);

    if !target.exists() {
        return Err(format!("目标文件不存在: {}", target_path));
    }

    if !target.is_file() {
        return Err(format!("目标路径不是文件: {}", target_path));
    }

    let link_dir_path = Path::new(link_dir);

    if !link_dir_path.exists() {
        return Err(format!("链接目录不存在: {}", link_dir));
    }

    if !link_dir_path.is_dir() {
        return Err(format!("链接路径不是目录: {}", link_dir));
    }

    let link_path = link_dir_path.join(link_name);

    if link_path.exists() {
        return Err(format!("链接路径已存在: {}", link_path.display()));
    }

    #[cfg(windows)]
    {
        use std::os::windows::fs::symlink_file;
        symlink_file(target_path, &link_path).map_err(|e| {
            log_error!("创建符号链接失败: {:?}, 错误: {:?}", link_path, e);
            format!("创建符号链接失败: {:?}", e)
        })?;
    }

    #[cfg(not(windows))]
    {
        use std::os::unix::fs::symlink;
        symlink(target_path, &link_path).map_err(|e| {
            log_error!("创建符号链接失败: {:?}, 错误: {:?}", link_path, e);
            format!("创建符号链接失败: {:?}", e)
        })?;
    }

    log_info!("文件符号链接创建成功: {}", link_path.display());
    Ok(format!("符号链接创建成功: {}", link_path.display()))
}

pub fn delete_file_symlink(link_path: &str) -> Result<String, String> {
    log_info!("开始删除文件符号链接: {}", link_path);

    let path = Path::new(link_path);

    if !path.exists() {
        return Err(format!("符号链接不存在: {}", link_path));
    }

    if !path.is_symlink() {
        return Err(format!("路径不是符号链接: {}", link_path));
    }

    fs::remove_file(path).map_err(|e| {
        log_error!("删除符号链接失败: {:?}, 错误: {:?}", path, e);
        format!("删除符号链接失败: {:?}", e)
    })?;

    log_info!("文件符号链接删除成功: {}", link_path);
    Ok(format!("符号链接删除成功: {}", link_path))
}
