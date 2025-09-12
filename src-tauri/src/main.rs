// Prevents additional console window on Windows in release, DO NOT REMOVE!!
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

// 明确指定该应用程序仅支持Windows平台
#[cfg(not(target_os = "windows"))]
compile_error!("This application only supports Windows systems");

fn main() {
    nyaser_maps_downloader_lib::run()
}
