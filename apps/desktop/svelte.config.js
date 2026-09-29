// 显式提供配置：缺这个文件时 vite-plugin-svelte 每次构建都发 "no Svelte config
// found" 告警。组件 `lang="ts"` 走 Svelte 5 内置 TS 支持，无需预处理器；
// 若日后用到枚举/装饰器等高级 TS 语法，再引入 vitePreprocess()。
export default {};
