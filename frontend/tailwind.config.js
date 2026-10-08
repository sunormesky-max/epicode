/** @type {import('tailwindcss').Config} */
// 颜色一律走 src/index.css 的 CSS 变量(--bg-* / --text-* / --accent-* …),不经 Tailwind 颜色类。
// 原 shadcn 模板的 hsl(var(--primary)) 等颜色、accordion/caret 动画与 tailwindcss-animate 插件
// 在代码中零引用(构建产物逐字节对比无差异),已移除。
module.exports = {
  content: ['./index.html', './src/**/*.{js,ts,jsx,tsx}'],
  theme: {
    extend: {
      // 注意:--radius 从未定义,rounded-md/lg/xl 当前实际渲染为直角。保留现状(视觉不变),见 PR 说明。
      borderRadius: {
        xl: "calc(var(--radius) + 4px)",
        lg: "var(--radius)",
        md: "calc(var(--radius) - 2px)",
        sm: "calc(var(--radius) - 4px)",
        xs: "calc(var(--radius) - 6px)",
      },
    },
  },
}
