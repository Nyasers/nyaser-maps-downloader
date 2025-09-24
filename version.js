import fs from 'fs';
import path from 'path';

// 定义文件路径
const rootDir = process.cwd();
const packageJsonPath = path.join(rootDir, 'package.json');
const tauriConfPath = path.join(rootDir, 'src-tauri', 'tauri.conf.json');
const cargoTomlPath = path.join(rootDir, 'src-tauri', 'Cargo.toml');

// 获取当前环境模式（development或production）
function getEnvironmentMode() {
    // 从环境变量或命令行参数中获取环境模式
    const mode = process.env.NODE_ENV || 'development';
    return mode.toLowerCase();
}

// 读取package.json获取版本号
function getVersionFromPackageJson() {
    try {
        const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, 'utf-8'));
        return packageJson.version;
    } catch (error) {
        console.error('读取package.json失败:', error);
        process.exit(1);
    }
}

// 更新tauri.conf.json中的版本号和frontendDist路径
function updateTauriConf(version) {
    try {
        const tauriConf = JSON.parse(fs.readFileSync(tauriConfPath, 'utf-8'));
        tauriConf.version = version;
        
        // 根据环境模式设置不同的frontendDist路径
        const mode = getEnvironmentMode();
        if (mode === 'development') {
            tauriConf.build.frontendDist = './html/frontend';
        } else {
            tauriConf.build.frontendDist = './dist/frontend';
        }
        
        fs.writeFileSync(tauriConfPath, JSON.stringify(tauriConf, null, 2) + '\n', 'utf-8');
        console.log(`已更新${tauriConfPath}中的版本号为${version}，环境模式: ${mode}，frontendDist: ${tauriConf.build.frontendDist}`);
    } catch (error) {
        console.error('更新tauri.conf.json失败:', error);
        process.exit(1);
    }
}

// 更新Cargo.toml中的版本号
function updateCargoToml(version) {
    try {
        const cargoToml = fs.readFileSync(cargoTomlPath, 'utf-8');
        const updatedContent = cargoToml.replace(/^version = "[^"]+"/m, `version = "${version}"`);
        fs.writeFileSync(cargoTomlPath, updatedContent, 'utf-8');
        console.log(`已更新${cargoTomlPath}中的版本号为${version}`);
    } catch (error) {
        console.error('更新Cargo.toml失败:', error);
        process.exit(1);
    }
}



// 主函数
function main() {
    const version = getVersionFromPackageJson();
    console.log(`从package.json获取版本号: ${version}`);
    
    updateTauriConf(version);
    updateCargoToml(version);
    
    console.log('所有文件的版本号更新完成!');
}

// 执行主函数
main();