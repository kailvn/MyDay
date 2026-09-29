/**
 * 编译期常量（vite define 注入，见 vite.config.ts）：非生产构建为 true。
 * 用于把浏览器 e2e mock（?e2e=1）限制在 dev 构建，生产构建整段消除。
 */
declare const __MYDAY_E2E__: boolean;
