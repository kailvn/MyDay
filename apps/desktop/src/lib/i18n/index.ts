/**
 * 桶导出：组件统一 `from ".../i18n"` 导入。真正的响应式实现在
 * `index.svelte.ts`（runes），这里只转发 —— 纯 node 环境（golden 自检）
 * 不会经过本文件，仍按 misc 词典的懒加载 shim 回退中文。
 */
export * from "./index.svelte";
