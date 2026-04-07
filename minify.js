import path from "path";
import { fileURLToPath } from "url";
import { minify as minifyHTML } from "html-minifier-terser";
import { minify as minifyJS } from "terser";
import { default as cssnanoPlugin } from "cssnano";
import {
  readdirSync,
  readFileSync,
  writeFileSync,
  existsSync,
  mkdirSync,
  statSync,
  rmSync,
} from "fs";

// 导入压缩配置选项
import options from "./minify-options.js";
const cssnano = cssnanoPlugin();

// 获取当前文件和目录路径
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// 定义依赖复制配置
const copyConfig = [
  {
    src: "fuse.js/dist/fuse.min.js",
    dst: "fuse.min.js",
  },
];

// 源目录和目标目录
const nodeModulesDir = path.join(__dirname, "node_modules");
const assetsLibDir = path.join(__dirname, "src-tauri", "assets", "lib");

// 获取src和assets目录路径
const srcDir = path.join(__dirname, "src");
const assetsPath = path.join(__dirname, "src-tauri", "assets");
if (existsSync(assetsPath)) {
  rmSync(assetsPath, { recursive: true });
}

// 获取当前环境模式（development或production）
function getEnvironmentMode() {
  // 从环境变量或命令行参数中获取环境模式
  const mode = process.env.NODE_ENV || "production";
  return mode.toLowerCase();
}

// 递归获取目录下所有文件，排除指定扩展名
function getAllFilesExcept(dir, excludeExtensions = []) {
  let results = [];
  const list = readdirSync(dir);

  list.forEach((file) => {
    const filePath = path.join(dir, file);
    const stat = statSync(filePath);

    if (stat.isDirectory()) {
      // 递归处理子目录
      results = results.concat(getAllFilesExcept(filePath, excludeExtensions));
    } else if (!excludeExtensions.some((ext) => file.endsWith(ext))) {
      results.push(filePath);
    }
  });

  return results;
}

// 获取所有文件（默认不排除任何文件）
const allFiles = getAllFilesExcept(srcDir);

// 定义文件类型映射
const fileTypes = {
  html: ".html",
  js: ".js",
  css: ".css",
  json: ".json",
};

// 生成需要排除的文本文件扩展名数组
const textFileExtensions = Object.values(fileTypes);

// 按文件类型分类
const htmlFiles = allFiles.filter((file) => file.endsWith(fileTypes.html));
const jsFiles = allFiles.filter((file) => file.endsWith(fileTypes.js));
const cssFiles = allFiles.filter((file) => file.endsWith(fileTypes.css));
const jsonFiles = allFiles.filter((file) => file.endsWith(fileTypes.json));

// 使用完全的exclude模式处理资产文件：排除所有不需要的文件类型
const assetFiles = allFiles.filter((file) => {
  // 排除所有文本文件
  const isTextFile = textFileExtensions.some((ext) => file.endsWith(ext));
  if (isTextFile) return false;

  // 排除其他不需要的文件类型（如果有）
  const otherExcludedExtensions = []; // 可以在这里添加其他需要排除的文件类型
  const isOtherExcludedFile = otherExcludedExtensions.some((ext) =>
    file.endsWith(ext),
  );
  if (isOtherExcludedFile) return false;

  // 保留所有剩余文件（作为资产文件）
  return true;
});

// 生成压缩后的文件路径
function generateOutputPath(inputPath) {
  // 计算相对于src目录的路径
  const relativePath = path.relative(srcDir, inputPath);

  // 构建assets目录中的目标路径，保持相同的目录结构
  const filePath = path.join(assetsPath, relativePath);

  // 确保目标目录存在
  const fileDir = path.dirname(filePath);
  if (!existsSync(fileDir)) {
    mkdirSync(fileDir, { recursive: true });
  }

  return filePath;
}

// 格式化文件大小显示
function formatFileSize(bytes) {
  if (bytes === 0) return "0 Bytes";
  const k = 1024;
  const sizes = ["Bytes", "KB", "MB", "GB"];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + " " + sizes[i];
}

// 获取文件编码后的字节长度
function getFileSize(content) {
  return Buffer.byteLength(content, "utf8");
}

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

// 主压缩函数
async function minifyFiles() {
  try {
    console.log(`🚀 资源压缩工具启动...`);
    console.log(
      `🚀 发现 ${htmlFiles.length} 个HTML文件、${jsFiles.length} 个JavaScript文件、${cssFiles.length} 个CSS文件和 ${jsonFiles.length} 个JSON文件需要压缩...`,
    );

    // 总统计信息
    let totalOriginalSize = 0;
    let totalMinifiedSize = 0;
    let totalSavedSize = 0;
    const results = [];

    // 压缩HTML文件
    for (const file of htmlFiles) {
      try {
        const originalContent = readFileSafely(file);
        const originalSize = getFileSize(originalContent);

        // 先尝试基础压缩
        let minifiedContent;
        try {
          minifiedContent = await minifyHTML(originalContent, options);
        } catch (error) {
          console.error(
            `⚠️  高级压缩失败，尝试降级压缩: ${path.basename(file)}`,
          );
          // 降级压缩配置
          const fallbackOptions = { ...options };
          fallbackOptions.minifyJS = false;
          fallbackOptions.minifyCSS = false;
          minifiedContent = await minifyHTML(originalContent, fallbackOptions);
        }

        const minifiedSize = getFileSize(minifiedContent);
        const compressionRatio = (
          (1 - minifiedSize / originalSize) *
          100
        ).toFixed(2);
        const savedSize = originalSize - minifiedSize;

        // 更新总统计
        totalOriginalSize += originalSize;
        totalMinifiedSize += minifiedSize;
        totalSavedSize += savedSize;

        // 保存压缩文件
        const outputPath = generateOutputPath(file);
        writeFileSafely(outputPath, minifiedContent);

        results.push({
          file,
          success: true,
          originalSize,
          minifiedSize,
          savedSize,
          compressionRatio,
          outputPath,
        });

        // 打印单个文件的压缩结果，显示相对路径
        const relativeFilePath = path.relative(srcDir, file);
        const relativeOutputPath = path.relative(assetsPath, outputPath);
        console.log(`✅ 已压缩: ${relativeFilePath}`);
        console.log(`   📦 原始大小: ${formatFileSize(originalSize)}`);
        console.log(`   📦 压缩大小: ${formatFileSize(minifiedSize)}`);
        console.log(
          `   💾 节省空间: ${formatFileSize(savedSize)} (${compressionRatio}%)`,
        );
        console.log(`   🎯 输出到: ${relativeOutputPath}`);
      } catch (error) {
        const relativeFilePath = path.relative(srcDir, file);
        console.error(`❌ 压缩文件失败: ${relativeFilePath}`, error.message);
        results.push({ file, success: false, error: error.message });
      }
    }

    // 压缩JavaScript文件
    for (const file of jsFiles) {
      try {
        const originalContent = readFileSafely(file);
        const originalSize = getFileSize(originalContent);

        // 压缩JS文件
        let minifiedContent;
        try {
          const result = await minifyJS(originalContent, options.minifyJS);
          minifiedContent = result.code || originalContent;
        } catch (error) {
          console.error(`⚠️  JavaScript压缩失败: ${error.stack}`);
          minifiedContent = originalContent;
        }

        const minifiedSize = getFileSize(minifiedContent);
        const compressionRatio = (
          (1 - minifiedSize / originalSize) *
          100
        ).toFixed(2);
        const savedSize = originalSize - minifiedSize;

        // 更新总统计
        totalOriginalSize += originalSize;
        totalMinifiedSize += minifiedSize;
        totalSavedSize += savedSize;

        // 保存压缩文件
        const outputPath = generateOutputPath(file);
        writeFileSafely(outputPath, minifiedContent);

        results.push({
          file,
          success: true,
          originalSize,
          minifiedSize,
          savedSize,
          compressionRatio,
          outputPath,
        });

        // 打印单个文件的压缩结果，显示相对路径
        const relativeFilePath = path.relative(srcDir, file);
        const relativeOutputPath = path.relative(assetsPath, outputPath);
        console.log(`✅ 已压缩: ${relativeFilePath}`);
        console.log(`   📦 原始大小: ${formatFileSize(originalSize)}`);
        console.log(`   📦 压缩大小: ${formatFileSize(minifiedSize)}`);
        console.log(
          `   💾 节省空间: ${formatFileSize(savedSize)} (${compressionRatio}%)`,
        );
        console.log(`   🎯 输出到: ${relativeOutputPath}`);
      } catch (error) {
        const relativeFilePath = path.relative(srcDir, file);
        console.error(`❌ 压缩文件失败: ${relativeFilePath}`, error.message);
        results.push({ file, success: false, error: error.message });
      }
    }

    // 压缩CSS文件
    for (const file of cssFiles) {
      try {
        const originalContent = readFileSafely(file);
        const originalSize = getFileSize(originalContent);

        // 压缩CSS文件
        let minifiedContent;
        try {
          const result = await cssnano.process(originalContent, {
            from: undefined,
          });
          minifiedContent = result.css || originalContent;
        } catch (error) {
          console.error(`⚠️  CSS压缩失败: ${error.stack}`);
          minifiedContent = originalContent;
        }

        const minifiedSize = getFileSize(minifiedContent);
        const compressionRatio = (
          (1 - minifiedSize / originalSize) *
          100
        ).toFixed(2);
        const savedSize = originalSize - minifiedSize;

        // 更新总统计
        totalOriginalSize += originalSize;
        totalMinifiedSize += minifiedSize;
        totalSavedSize += savedSize;

        // 保存压缩文件
        const outputPath = generateOutputPath(file);
        writeFileSafely(outputPath, minifiedContent);

        results.push({
          file,
          success: true,
          originalSize,
          minifiedSize,
          savedSize,
          compressionRatio,
          outputPath,
        });

        // 打印单个文件的压缩结果，显示相对路径
        const relativeFilePath = path.relative(srcDir, file);
        const relativeOutputPath = path.relative(assetsPath, outputPath);
        console.log(`✅ 已压缩: ${relativeFilePath}`);
        console.log(`   📦 原始大小: ${formatFileSize(originalSize)}`);
        console.log(`   📦 压缩大小: ${formatFileSize(minifiedSize)}`);
        console.log(
          `   💾 节省空间: ${formatFileSize(savedSize)} (${compressionRatio}%)`,
        );
        console.log(`   🎯 输出到: ${relativeOutputPath}`);
      } catch (error) {
        const relativeFilePath = path.relative(srcDir, file);
        console.error(`❌ 压缩文件失败: ${relativeFilePath}`, error.message);
        results.push({ file, success: false, error: error.message });
      }
    }

    // 压缩JSON文件
    for (const file of jsonFiles) {
      try {
        const originalContent = readFileSafely(file);
        const originalSize = getFileSize(originalContent);

        // 压缩JSON文件
        let minifiedContent;
        try {
          const parsedJson = JSON.parse(originalContent);
          minifiedContent = JSON.stringify(parsedJson);
        } catch (error) {
          console.error(`⚠️  JSON压缩失败: ${error.stack}`);
          minifiedContent = originalContent;
        }

        const minifiedSize = getFileSize(minifiedContent);
        const compressionRatio = (
          (1 - minifiedSize / originalSize) *
          100
        ).toFixed(2);
        const savedSize = originalSize - minifiedSize;

        // 更新总统计
        totalOriginalSize += originalSize;
        totalMinifiedSize += minifiedSize;
        totalSavedSize += savedSize;

        // 保存压缩文件
        const outputPath = generateOutputPath(file);
        writeFileSafely(outputPath, minifiedContent);

        results.push({
          file,
          success: true,
          originalSize,
          minifiedSize,
          savedSize,
          compressionRatio,
          outputPath,
        });

        // 打印单个文件的压缩结果，显示相对路径
        const relativeFilePath = path.relative(srcDir, file);
        const relativeOutputPath = path.relative(assetsPath, outputPath);
        console.log(`✅ 已压缩: ${relativeFilePath}`);
        console.log(`   📦 原始大小: ${formatFileSize(originalSize)}`);
        console.log(`   📦 压缩大小: ${formatFileSize(minifiedSize)}`);
        console.log(
          `   💾 节省空间: ${formatFileSize(savedSize)} (${compressionRatio}%)`,
        );
        console.log(`   🎯 输出到: ${relativeOutputPath}`);
      } catch (error) {
        const relativeFilePath = path.relative(srcDir, file);
        console.error(`❌ 压缩文件失败: ${relativeFilePath}`, error.message);
        results.push({ file, success: false, error: error.message });
      }
    }

    // 打印总体统计信息
    const overallCompressionRatio =
      totalOriginalSize > 0
        ? ((1 - totalMinifiedSize / totalOriginalSize) * 100).toFixed(2)
        : "0.00";

    // 处理图标和其他资源文件
    for (const file of assetFiles) {
      try {
        // 复制资源文件（不需要压缩）
        const outputPath = generateOutputPath(file);
        const content = readFileSafely(file, null); // 使用null编码以二进制模式读取
        writeFileSafely(outputPath, content, null); // 以二进制模式写入

        // 打印复制结果
        const relativeFilePath = path.relative(srcDir, file);
        const relativeOutputPath = path.relative(assetsPath, outputPath);
        console.log(`✅ 已复制: ${relativeFilePath}`);
        console.log(`   🎯 输出到: ${relativeOutputPath}`);

        results.push({ file, success: true, outputPath });
      } catch (error) {
        const relativeFilePath = path.relative(srcDir, file);
        console.error(`❌ 复制文件失败: ${relativeFilePath}`, error.message);
        results.push({ file, success: false, error: error.message });
      }
    }

    console.log("\n========== 压缩统计摘要 ==========");
    console.log(`📂 总文件数: ${results.length}`);
    console.log(`⚡ 处理文件数: ${results.filter((r) => r.success).length}`);
    console.log(`📊 总原始大小: ${formatFileSize(totalOriginalSize)}`);
    console.log(`📊 总压缩大小: ${formatFileSize(totalMinifiedSize)}`);
    console.log(`💰 总共节省: ${formatFileSize(totalSavedSize)}`);
    console.log(`🎯 总体压缩率: ${overallCompressionRatio}%`);
    console.log("=================================");

    // 检查是否有失败的文件
    const failedFiles = results.filter((result) => !result.success);
    if (failedFiles.length > 0) {
      console.log("\n❌ 以下文件压缩失败:");
      failedFiles.forEach(({ file, error }) => {
        console.log(`  - ${path.basename(file)}: ${error}`);
      });
      process.exit(1);
    }

    console.log("\n🎉 所有文件压缩完成！");

    // 复制依赖文件
    await copyLibs();
  } catch (error) {
    console.error("压缩过程发生严重错误:", error);
    process.exit(1);
  }
}

// 复制依赖文件函数
async function copyLibs() {
  try {
    console.log(`\n🚀 依赖复制工具启动...`);

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

// 启动压缩
console.log("🚀 HTML压缩工具启动...");
minifyFiles().catch((error) => {
  console.error("压缩流程执行失败:", error);
  process.exit(1);
});
