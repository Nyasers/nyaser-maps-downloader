import path from 'path';
import { fileURLToPath } from 'url';
import { minify } from 'html-minifier-terser';
import { readdirSync, readFileSync, writeFileSync } from 'fs';

// 获取当前文件和目录路径
const __filename = fileURLToPath(import.meta.url);
const __dirname = path.dirname(__filename);

// 获取HTML目录路径
const htmlDir = path.join(__dirname, 'src-tauri', 'html');

// 获取HTML目录下所有非min的HTML文件
const htmlFiles = readdirSync(htmlDir)
  .filter(file => file.endsWith('.html') && !file.endsWith('.min.html'))
  .map(file => path.join(htmlDir, file));

// 配置压缩选项，经过精心调优以获得最佳压缩效果
const options = {
  // 基础压缩选项 - 核心功能
  collapseWhitespace: true,
  removeComments: true,
  removeOptionalTags: true,
  removeRedundantAttributes: true,
  removeScriptTypeAttributes: true,
  removeStyleLinkTypeAttributes: true,
  useShortDoctype: true,
  removeEmptyElements: true,
  removeEmptyAttributes: true,
  
  // CSS压缩优化 - 平衡压缩率和性能
  minifyCSS: {
    level: 2,
    format: {
      comments: false,
      spaces: false
    },
    compatibility: 'ie11',
    roundingPrecision: -1
  },
  
  // JavaScript压缩优化 - 经过实战验证的最佳配置
  minifyJS: {
    compress: {
      passes: 4, // 适当的压缩遍数
      drop_console: true,
      drop_debugger: true,
      conditionals: true,
      dead_code: true,
      evaluate: true,
      booleans: true,
      loops: true,
      unused: true,
      warnings: false,
      join_vars: true,
      toplevel: true,
      keep_fargs: false,
      pure_getters: true,
      pure_funcs: ['console.log', 'console.warn', 'console.error', 'debugger'],
      if_return: true,
      join_vars: true,
      side_effects: true,
      global_defs: {
        "DEBUG": false
      }
    },
    mangle: {
      toplevel: true,
      keep_classnames: false,
      keep_fnames: false,
      safari10: true
    },
    output: {
      comments: false,
      beautify: false,
      indent_level: 0,
      quote_style: 1
    }
  },
  
  // Tauri应用特有的优化
  removeAttributeQuotes: true,
  preserveLineBreaks: false,
  sortAttributes: true,
  sortClassName: true,
  html5: true,
  caseSensitive: false,
  
  // 确保与Tauri API的兼容性
  ignoreCustomComments: [/TAURI_API/],
  ignoreCustomFragments: [/\<\?#.*?\?\>/],
  keepClosingSlash: true
};

// 生成压缩后的文件路径
function generateOutputPath(inputPath) {
  const dir = path.dirname(inputPath);
  const baseName = path.basename(inputPath, '.html');
  return path.join(dir, `${baseName}.min.html`);
}

// 格式化文件大小显示
function formatFileSize(bytes) {
  if (bytes === 0) return '0 Bytes';
  const k = 1024;
  const sizes = ['Bytes', 'KB', 'MB', 'GB'];
  const i = Math.floor(Math.log(bytes) / Math.log(k));
  return parseFloat((bytes / Math.pow(k, i)).toFixed(2)) + ' ' + sizes[i];
}

// 获取文件编码后的字节长度
function getFileSize(content) {
  return Buffer.byteLength(content, 'utf8');
}

// 安全的文件读写操作
function readFileSafely(filePath, encoding = 'utf8') {
  try {
    return readFileSync(filePath, encoding);
  } catch (error) {
    console.error(`读取文件失败: ${filePath}`, error.message);
    throw error;
  }
}

function writeFileSafely(filePath, content, encoding = 'utf8') {
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
    console.log(`🚀 发现 ${htmlFiles.length} 个文件需要压缩...`);
    
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
          minifiedContent = await minify(originalContent, options);
        } catch (error) {
          console.error(`⚠️  高级压缩失败，尝试降级压缩: ${path.basename(file)}`);
          // 降级压缩配置
          const fallbackOptions = { ...options };
          fallbackOptions.minifyJS = false;
          fallbackOptions.minifyCSS = false;
          minifiedContent = await minify(originalContent, fallbackOptions);
        }
        
        const minifiedSize = getFileSize(minifiedContent);
        const compressionRatio = ((1 - minifiedSize / originalSize) * 100).toFixed(2);
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
          outputPath
        });
        
        // 打印单个文件的压缩结果
        console.log(`✅ 已压缩: ${path.basename(file)}`);
        console.log(`   📦 原始大小: ${formatFileSize(originalSize)}`);
        console.log(`   📦 压缩大小: ${formatFileSize(minifiedSize)}`);
        console.log(`   💾 节省空间: ${formatFileSize(savedSize)} (${compressionRatio}%)`);
      } catch (error) {
        console.error(`❌ 压缩文件失败: ${path.basename(file)}`, error.message);
        results.push({ file, success: false, error: error.message });
      }
    }
    
    // 打印总体统计信息
    const overallCompressionRatio = totalOriginalSize > 0 
      ? ((1 - totalMinifiedSize / totalOriginalSize) * 100).toFixed(2) 
      : '0.00';
    
    console.log('\n========== 压缩统计摘要 ==========');
    console.log(`📂 总文件数: ${htmlFiles.length}`);
    console.log(`⚡ 压缩文件数: ${results.filter(r => r.success).length}`);
    console.log(`📊 总原始大小: ${formatFileSize(totalOriginalSize)}`);
    console.log(`📊 总压缩大小: ${formatFileSize(totalMinifiedSize)}`);
    console.log(`💰 总共节省: ${formatFileSize(totalSavedSize)}`);
    console.log(`🎯 总体压缩率: ${overallCompressionRatio}%`);
    console.log('=================================');
    
    // 检查是否有失败的文件
    const failedFiles = results.filter(result => !result.success);
    if (failedFiles.length > 0) {
      console.log('\n❌ 以下文件压缩失败:');
      failedFiles.forEach(({ file, error }) => {
        console.log(`  - ${path.basename(file)}: ${error}`);
      });
      process.exit(1);
    }
    
    console.log('\n🎉 所有文件压缩完成！');
    
  } catch (error) {
    console.error('压缩过程发生严重错误:', error);
    process.exit(1);
  }
}

// 启动压缩
console.log('🚀 HTML压缩工具启动...');
minifyFiles().catch(error => {
  console.error('压缩流程执行失败:', error);
  process.exit(1);
});