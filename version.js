import fs from 'fs';
import path from 'path';
import { execSync } from 'child_process';

// 获取命令行参数
const args = process.argv.slice(2);

if (args.length === 0) {
    console.error('请提供版本号参数，例如: patch, minor, major 或具体版本号');
    process.exit(1);
}

// 定义文件路径
const rootDir = process.cwd();
const packageJsonPath = path.join(rootDir, 'package.json');

// 读取package.json
function readPackageJson() {
    try {
        return JSON.parse(fs.readFileSync(packageJsonPath, 'utf-8'));
    } catch (error) {
        console.error('读取package.json失败:', error);
        process.exit(1);
    }
}

// 写入package.json
function writePackageJson(data) {
    try {
        fs.writeFileSync(packageJsonPath, JSON.stringify(data, null, 2) + '\n', 'utf-8');
    } catch (error) {
        console.error('写入package.json失败:', error);
        process.exit(1);
    }
}

// 计算新版本号
function calculateNewVersion(currentVersion, versionType) {
    if (versionType === 'patch' || versionType === 'minor' || versionType === 'major') {
        // 解析版本号
        const [major, minor, patch] = currentVersion.split('.').map(Number);
        
        // 根据类型增加版本号
        if (versionType === 'patch') {
            return `${major}.${minor}.${patch + 1}`;
        } else if (versionType === 'minor') {
            return `${major}.${minor + 1}.0`;
        } else if (versionType === 'major') {
            return `${major + 1}.0.0`;
        }
    }
    
    // 如果不是patch/minor/major，则直接使用提供的版本号
    return versionType;
}

// 执行命令并处理错误
function runCommand(command, description) {
    console.log(`\n🚀 ${description}...`);
    try {
        execSync(command, { stdio: 'inherit', cwd: rootDir });
        console.log(`✅ ${description} 完成`);
    } catch (error) {
        console.error(`❌ ${description} 失败:`, error.message);
        process.exit(1);
    }
}

// 主函数
function main() {
    const packageJson = readPackageJson();
    const currentVersion = packageJson.version;
    const versionArg = args[0];
    const newVersion = calculateNewVersion(currentVersion, versionArg);
    
    console.log(`当前版本: ${currentVersion}`);
    console.log(`新版本: ${newVersion}`);
    
    // 更新package.json中的版本号
    packageJson.version = newVersion;
    writePackageJson(packageJson);
    console.log(`✅ 已更新package.json中的版本号为 ${newVersion}`);
    
    // 执行构建命令（build过程中会自动运行version.js）
    runCommand('npm run build', '执行构建');
    
    // 提交更改并创建标签
    runCommand(`git add .`, '添加所有更改到暂存区');
    runCommand(`git commit -m "v${newVersion}"`, '提交更改');
    runCommand(`git tag -a v${newVersion} -m "v${newVersion}"`, `创建标签 v${newVersion}`);
    
    console.log(`\n🎉 版本更新完成! 新版本: ${newVersion}`);
    console.log(`提示: 运行 git push && git push --tags 来推送更改和标签`);
}

// 执行主函数
main();