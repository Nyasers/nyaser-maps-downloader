// 压缩配置选项 - 经过精心调优以获得最佳压缩效果
const minifyOptions = {
  // 基础压缩选项 - 核心功能
  collapseWhitespace: true,
  removeComments: true,
  removeOptionalTags: true,
  removeRedundantAttributes: true,
  removeScriptTypeAttributes: true,
  removeStyleLinkTypeAttributes: true,
  useShortDoctype: true,
  removeEmptyElements: false,
  removeEmptyAttributes: true,
  collapseBooleanAttributes: true,
  minifyURLs: true, // 优化URL
  processConditionalComments: true,

  // CSS压缩优化 - 最大化压缩率
  minifyCSS: {
    level: 2, // 最高压缩级别
    format: {
      comments: false,
      spaces: false
    },
    // Tauri应用不需要IE11兼容性，移除以提高压缩率
    discardComments: { removeAll: true },
    roundingPrecision: -1
  },

  // JavaScript压缩优化 - 经过实战验证的最佳配置
  minifyJS: {
    compress: {
      passes: 6, // 增加压缩遍数以提高压缩率
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
      side_effects: true,
      // 额外的压缩选项
      sequences: true,
      properties: true,
      comparisons: true,
      arrows: true,
      assign: true,
      variables: true,
      unsafe: true,
      unsafe_arrows: true,
      unsafe_methods: true,
      unsafe_proto: true
    },
    mangle: {
      toplevel: true,
      keep_classnames: false,
      keep_fnames: false,
      safari10: true,
      eval: true,
      module: true // 假设项目使用ES模块
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
  decodeEntities: true, // 解码HTML实体

  // 确保与Tauri API的兼容性
  ignoreCustomComments: [/TAURI_API/],
  ignoreCustomFragments: [/\<\?#.*?\?\>/],
  keepClosingSlash: true
};

export default minifyOptions;