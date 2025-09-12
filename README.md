# Nyaser Maps Downloader

Nyaser Maps Downloader 是一个专为从 [maps.nyase.ru](https://maps.nyase.ru) 网站下载和管理地图文件而设计的桌面应用程序。该应用基于 Tauri 框架构建，提供高效、可靠的地图下载体验。

## 项目简介

Nyaser Maps Downloader 简化了从 maps.nyase.ru 网站获取地图资源的过程，通过智能拦截下载请求和高效的下载队列管理，为用户提供流畅的地图获取体验。

## 主要功能

- **一键下载**：通过 HTML 注入技术，自动拦截并处理地图下载请求
- **高效下载管理**：使用 aria2c 作为下载后端，支持队列管理和并行下载
- **自动解压**：下载完成后自动解压地图文件到指定目录
- **用户友好界面**：基于 Web 技术的现代化界面，与 maps.nyase.ru 网站无缝集成
- **系统托盘支持**：应用可最小化到系统托盘，随时访问

## 系统要求

- **操作系统**：Windows 7 或更高版本
- **内存**：至少 2GB RAM
- **存储空间**：至少 100MB 可用空间（不包括下载的地图文件）

## 安装说明

1. 从 [发布页面](https://github.com/Nyasers/nyaser-maps-downloader/releases) 下载最新版本的安装程序
2. 运行安装程序并按照提示完成安装
3. 启动应用程序，开始使用地图下载功能

## 使用方法

1. 打开 Nyaser Maps Downloader 应用程序
2. 应用将自动加载 [maps.nyase.ru](https://maps.nyase.ru) 网站
3. 在网站上浏览并选择您想要下载的地图
4. 点击下载按钮，应用将自动处理下载和解压过程
5. 下载完成后，地图文件将被保存在指定目录中

## 开发指南

如果您想从源代码构建此应用，请按照以下步骤操作：

### 前置要求

- [Node.js](https://nodejs.org/) v16 或更高版本
- [Rust](https://www.rust-lang.org/) 1.60 或更高版本
- [npm](https://www.npmjs.com/) 包管理器
- [Tauri CLI](https://tauri.app/v1/guides/getting-started/prerequisites/#setting-up-windows)

### 构建步骤

1. 克隆仓库：
   ```bash
   git clone https://github.com/Nyasers/nyaser-maps-downloader.git
   cd nyaser-maps-downloader
   ```

2. 安装依赖：
   ```bash
   npm install
   ```

3. 开发模式运行：
   ```bash
   npm run tauri dev
   ```

4. 构建发布版本：
   ```bash
   npm run tauri build
   ```

## 项目结构

```
nyaser-maps-downloader/
├── src-tauri/           # Rust 后端代码
│   ├── src/             # 主源代码目录
│   │   ├── aria2c.rs          # aria2c 下载后端集成
│   │   ├── commands.rs        # Tauri 命令定义
│   │   ├── download_manager.rs # 下载队列管理
│   │   └── ...                # 其他功能模块
│   ├── tauri.conf.json  # Tauri 应用配置
│   └── Cargo.toml       # Rust 依赖配置
├── minify.js            # HTML 压缩脚本
├── package.json         # 前端依赖配置
└── README.md            # 项目文档（当前文件）
```

## 技术栈

- **前端**：HTML, CSS, JavaScript
- **后端**：Rust
- **框架**：[Tauri](https://tauri.app/)
- **下载引擎**：aria2c
- **构建工具**：npm, Cargo

## 许可证

[MIT License](LICENSE)

## 注意事项

- 该应用仅支持 Windows 操作系统
- 请确保您有足够的存储空间用于下载和存储地图文件
- 应用会在临时目录中存储下载的文件，请定期清理以释放空间

## 更新日志

### v1.2.0
- 优化下载性能
- 改进用户界面
- 修复已知问题
