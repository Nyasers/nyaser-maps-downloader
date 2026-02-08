import path from "path";
import { fileURLToPath } from "url";
import {
  readFileSync,
  writeFileSync,
  existsSync,
  mkdirSync,
} from "fs";

// 获取当前文件和目录路径
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// 定义复制配置
const copyConfig = [
  {
    src: "fuse.js/dist/fuse.min.js",
    dst: "fuse.min.js",
  },
];

// 源目录和目标目录
const nodeModulesDir = path.join(__dirname, "node_modules");
const assetsLibDir = path.join(__dirname, "src-tauri", "assets", "lib");

// 安全的文件读写操作
function readFileSafely(filePath, encoding = "utf8") {
  try {
    return readFileSync(filePath, encoding);
  } catch (error) {
    console.error(`读取文件失败: ${filePath}`, error.message);
    throw error;
  }
}

function writeFileSafely(filePath, content, encoding = "utf8") {
  try {
    writeFileSync(filePath, content, encoding);
    return true;
  } catch (error) {
    console.error(`写入文件失败: ${filePath}`, error.message);
    throw error;
  }
}

// 主复制函数
async function copyLibs() {
  try {
    console.log(`🚀 依赖复制工具启动...`);

    // 确保目标目录存在
    if (!existsSync(assetsLibDir)) {
      console.log(`📁 创建目标目录: ${assetsLibDir}`);
      mkdirSync(assetsLibDir, { recursive: true });
    }

    let successCount = 0;
    let totalCount = copyConfig.length;

    // 复制每个依赖
    for (const config of copyConfig) {
      try {
        // 构建源文件路径
        const srcPath = path.join(nodeModulesDir, config.src);
        
        // 构建目标文件路径
        const dstPath = path.join(assetsLibDir, config.dst);

        // 检查源文件是否存在
        if (!existsSync(srcPath)) {
          console.error(`❌ 源文件不存在: ${srcPath}`);
          continue;
        }

        // 读取源文件
        const content = readFileSafely(srcPath);

        // 写入目标文件
        writeFileSafely(dstPath, content);

        // 计算相对路径用于显示
        const relativeSrc = path.relative(__dirname, srcPath);
        const relativeDst = path.relative(__dirname, dstPath);

        console.log(`✅ 已复制: ${relativeSrc}`);
        console.log(`   🎯 输出到: ${relativeDst}`);

        successCount++;
      } catch (error) {
        console.error(`❌ 复制文件失败: ${config.src}`, error.message);
      }
    }

    // 打印统计信息
    console.log(`\n📊 复制统计摘要:`);
    console.log(`📂 总文件数: ${totalCount}`);
    console.log(`⚡ 成功复制: ${successCount}`);
    console.log(`❌ 失败复制: ${totalCount - successCount}`);

    if (successCount === 0) {
      console.error("\n❌ 所有文件复制失败！");
      process.exit(1);
    }

    console.log(`\n🎉 依赖复制完成！`);
  } catch (error) {
    console.error("复制过程发生严重错误:", error);
    process.exit(1);
  }
}

// 启动复制
console.log("🚀 依赖复制工具启动...");
copyLibs().catch((error) => {
  console.error("复制流程执行失败:", error);
  process.exit(1);
});