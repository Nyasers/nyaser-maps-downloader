// 压缩配置选项 - 经过精心调优以获得最佳压缩效果
const minifyOptions = {
  // HTML 基础压缩选项 - 核心功能，旨在移除不必要的字符和结构
  collapseWhitespace: true, // 移除所有空格，包括换行符
  removeComments: true, // 移除所有 HTML 注释
  removeOptionalTags: true, // 移除可选的 HTML 标签（如 </body>, </html>）
  removeRedundantAttributes: true, // 移除多余的属性（如 <script type="text/javascript">）
  removeScriptTypeAttributes: true, // 移除 <script> 标签中的 type 属性
  removeStyleLinkTypeAttributes: true, // 移除 <style> 和 <link> 标签中的 type 属性
  useShortDoctype: true, // 使用简短的 HTML5 doctype
  removeEmptyElements: false, // 保留空元素，避免可能破坏模板。若需极限压缩，可设置为 true，但有风险。
  removeEmptyAttributes: true, // 移除所有空属性
  collapseBooleanAttributes: true, // 移除布尔属性的值（如 <input disabled="disabled"> 变为 <input disabled>）
  minifyURLs: true, // 压缩 URL
  processConditionalComments: true, // 处理条件注释

  // CSS 压缩优化 - 使用最高级别压缩，移除注释和多余的精度
  minifyCSS: {
    level: 2, // 确保使用最高压缩级别
    format: {
      comments: false,
      spaces: false
    },
    // 移除所有注释以提高压缩率
    discardComments: { removeAll: true },
    roundingPrecision: -1 // 移除所有浮点数的精度限制
  },

  // JavaScript 压缩优化 - 经过实战验证的最佳配置
  minifyJS: {
    compress: {
      passes: 6, // 增加压缩遍数以提高压缩率
      drop_console: true, // 移除所有 console.* 调用
      drop_debugger: true, // 移除所有 debugger 调用
      conditionals: true, // 优化 if 和三元表达式
      dead_code: true, // 移除不可达代码
      evaluate: true, // 尽可能地预计算常量表达式
      booleans: true, // 将布尔表达式转换为更紧凑的形式
      loops: true, // 优化循环
      unused: true, // 移除未使用的变量、函数和类
      warnings: false,
      join_vars: true, // 合并多个变量声明
      toplevel: true, // 压缩顶级作用域中的变量名
      keep_fargs: false, // 不保留函数参数名
      pure_getters: true, // 启用 getter 的纯函数检查
      pure_funcs: ['console.log', 'console.warn', 'console.error', 'debugger'],
      if_return: true, // 优化 if 语句和 return 语句
      side_effects: true, // 移除没有副作用的语句
      // 以下为额外且激进的压缩选项，可进一步减少文件大小
      sequences: true,
      properties: true,
      comparisons: true,
      arrows: true,
      unsafe: true, // 启用不安全的转换
      unsafe_arrows: true,
      unsafe_methods: true,
      unsafe_proto: true
    },
    mangle: {
      toplevel: true, // 对顶级作用域的变量名进行混淆
      keep_classnames: false, // 混淆类名
      keep_fnames: false, // 混淆函数名
      safari10: true,
      eval: true,
      module: true
    },
    output: {
      comments: false, // 移除所有注释
      beautify: false, // 不美化代码
      indent_level: 0,
      quote_style: 1
    }
  },

  // Tauri 应用特有的优化
  removeAttributeQuotes: true, // 移除不必要的属性引号
  preserveLineBreaks: false, // 移除所有换行符
  sortAttributes: true, // 按字母顺序排序属性
  sortClassName: true, // 按字母顺序排序类名
  html5: true, // 启用 HTML5 优化
  caseSensitive: false, // 启用不区分大小写的匹配
  decodeEntities: true, // 解码 HTML 实体

  // 确保与 Tauri API 的兼容性
  ignoreCustomComments: [/TAURI_API/],
  ignoreCustomFragments: [/\<\?#.*?\?\>/],
  keepClosingSlash: true
};

export default minifyOptions;
