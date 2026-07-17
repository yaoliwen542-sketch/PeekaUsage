# UI 框架重构 + 灵动岛设计

- 日期：2026-07-17
- 作者：brainstorming 会话
- 状态：待实现（阶段 A）
- 范围：阶段 A（框架重构 + 拖动卡顿修复）为主，阶段 B/C 给方向

## 背景与动机

PeekaUsage 当前 UI 是 React + 手写 CSS（main.css/widget.css/settings.css），存在以下问题：
- 样式不统一，组件间复用差
- 拖动窗口卡顿严重（透明窗口 + backdrop-filter 综合）
- 窗口层级不够高，容易被遮挡
- 缺少现代化的桌面交互形态（灵动岛）

用户决策：
- 优先级：先重构 UI 框架
- UI 框架：React + Tailwind 4 + shadcn/ui
- 组件库：shadcn/ui（Radix + Tailwind）
- 窗口层级：最高层级（总是最前）
- 新功能：只做悬浮窗灵动岛，不做桌面吉祥物
- 落地策略：渐进式（阶段 A → B → C）

## 设计决策汇总

| 维度 | 决策 |
|---|---|
| 优先级 | 先重构 UI 框架 |
| UI 框架 | React + Tailwind 4 + shadcn/ui |
| 组件库 | shadcn/ui（Radix + Tailwind） |
| 窗口层级 | 最高层级（总是最前） |
| 新功能 | 只做悬浮窗灵动岛，不做桌面吉祥物 |
| 落地策略 | 渐进式：阶段 A 框架重构 + 卡顿修复 → 阶段 B 灵动岛 → 阶段 C 窗口层级 + 细节 |
| 现有 CSS 变量 | 不保留，Tailwind 全部接管 |
| Tailwind 版本 | 4.x（最新，用 Vite 插件接入） |
| 拖动卡顿 | 重构时统一解决（拖动时禁用 backdrop-filter） |

---

## 第 1 节：总体架构与分阶段

### 目标

把 PeekaUsage 的 UI 从"React + 手写 CSS"重构为"React + Tailwind 4 + shadcn/ui"，提升 UI 一致性和可维护性；实现悬浮窗灵动岛；窗口层级提升到最高级；顺带解决拖动卡顿。

### 分阶段策略

| 阶段 | 内容 | 可发版 |
|---|---|---|
| 阶段 A | 引入 Tailwind 4 + shadcn/ui，重构设置页 + 主界面核心组件，修复拖动卡顿 | ✅ |
| 阶段 B | 实现悬浮窗灵动岛（新窗口/组件，用 shadcn/ui 写） | ✅ |
| 阶段 C | 窗口层级提升到最高级 + 细节优化 + 全面 UI/UX 审核 | ✅ |

### 阶段 A 范围（本次设计重点）

**引入 Tailwind 4 + shadcn/ui：**
- 安装 Tailwind 4 + @tailwindcss/vite 插件
- 初始化 shadcn/ui（components.json + 组件模板）
- 保留现有 CSS 变量设计系统（用 Tailwind 的 @theme 覆盖）

**重构组件：**
- 主界面：WidgetContainer、ProviderCard、TitleBar、底部按钮
- 设置页：SettingsPanel、ProviderConfig、子导航、下拉
- 通用组件：AppSelect、ConfirmDialog、ProviderIcon（保留逻辑，换样式）

**拖动卡顿修复（重构时统一解决）：**
- 拖动时动态禁用 backdrop-filter（拖动开始设 blur 为 0，松手恢复）
- onMoved/onResized 优化（已有的 scaleFactor 缓存保留）

### 技术选型

| 技术 | 版本 | 说明 |
|---|---|---|
| Tailwind CSS | 4.x | 4.0 起用 @tailwindcss/vite 插件，不用 tailwind.config.js |
| shadcn/ui | 最新 | 组件模板，复制到项目 |
| Radix UI | 最新 | shadcn/ui 底层，无障碍 |
| lucide-react | 最新 | 图标库 |

### 兼容性

- 现有组件逻辑（拖拽排序、轮询、保存链路）不动，只换样式
- 现有功能（多供应商、拖拽排序、透明度、边缘吸附）全部保留
- Tailwind 4 兼容现有 React 18

---

## 第 2 节：Tailwind 4 + shadcn/ui 接入

### 2.1 Tailwind 4 接入

**安装：**
```bash
npm install tailwindcss @tailwindcss/vite
```

**`vite.config.ts` 配置：**
```typescript
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
});
```

**`src/index.css`：**
```css
@import "tailwindcss";
```

Tailwind 4 自动检测文件，不需要手动配置 content。

**暗黑模式：**
```css
@custom-variant dark (&:where(.dark, .dark *));
```

### 2.2 shadcn/ui 接入

**初始化：**
```bash
npx shadcn@latest init
```

创建 `components.json`、`src/components/ui/`、`src/lib/utils.ts`。

**需要的组件（阶段 A）：**
button / card / input / select / dialog / switch / slider / badge / tooltip / scroll-area / separator / tabs

```bash
npx shadcn@latest add button card input select dialog switch slider badge tooltip scroll-area separator tabs
```

### 2.3 设计 token（Tailwind 4 @theme）

在 `src/index.css` 定义：

```css
@import "tailwindcss";

@theme {
  --color-background: oklch(0.11 0.01 285);
  --color-surface: oklch(0.16 0.01 285);
  --color-surface-elevated: oklch(0.21 0.01 285);
  --color-primary: oklch(0.65 0.15 240);
  --color-primary-foreground: oklch(0.95 0.02 240);
  --color-border: oklch(0.25 0.01 285);
  --color-text: oklch(0.95 0.02 285);
  --color-text-secondary: oklch(0.7 0.02 285);
  --color-success: oklch(0.65 0.15 150);
  --color-warning: oklch(0.65 0.15 80);
  --color-error: oklch(0.65 0.15 20);
  --radius-sm: 0.25rem;
  --radius-md: 0.5rem;
  --radius-lg: 0.75rem;
}
```

暗黑模式用 `.dark` 类覆盖。

### 2.4 拖动卡顿修复（重构时统一解决）

**方案：** 拖动时动态禁用 backdrop-filter。

**实现：**
1. 用 CSS 自定义属性 `--backdrop-blur` 控制 blur 值
2. 监听标题栏 `mousedown` 时设 `--backdrop-blur: 0`，`mouseup` 恢复
3. 受影响元素（widget.css / settings.css 里的 backdrop-filter 元素）改为引用该变量

**代码位置：**
- `src/components/common/TitleBar.tsx` - mousedown/mouseup 触发
- `src/assets/styles/widget.css` - backdrop-filter 改 `blur(var(--backdrop-blur, 18px))`
- `src/assets/styles/settings.css` - 同上

### 2.5 重构组件清单（阶段 A）

| 组件 | 当前状态 | 重构方式 |
|---|---|---|
| TitleBar.tsx | 自定义标题栏 | Tailwind 重写样式，保留拖拽逻辑 |
| WidgetContainer.tsx | 主界面容器 | Tailwind + Card 组件重写 |
| ProviderCard.tsx | 供应商卡片 | shadcn/ui Card + Progress + Badge |
| AppSelect.tsx | 自定义下拉 | shadcn/ui Select（保留 portal） |
| ConfirmDialog.tsx | 确认弹层 | shadcn/ui Dialog |
| ProviderWizardDialog.tsx | 自定义向导 | shadcn/ui Dialog + Steps |
| SettingsPanel.tsx | 设置页 | Tailwind + Tabs 重写 |
| ProviderConfig.tsx | 供应商配置卡 | shadcn/ui Card + Input + Switch |
| ProviderIcon.tsx | 图标组件 | 保留逻辑，样式用 Tailwind |

### 2.6 旧 CSS 处理

- 保留旧 CSS 里还没重构的组件样式
- 重构完成的组件从旧 CSS 删除
- 全部重构完后，旧 CSS 文件删除

---

## 第 3 节：阶段 B 灵动岛 + 阶段 C 窗口层级

### 3.1 阶段 B：悬浮窗灵动岛

**形态：**
- 默认收起：屏幕顶部中间小胶囊，显示最高用量供应商 + 百分比
- 悬停展开：显示所有供应商 compact 摘要（图标 + 名称 + 百分比 + 进度条）
- 可拖动：可拖到屏幕任意位置，位置持久化
- 可交互：点供应商跳转主界面

**技术实现：**
- 新建独立 Tauri 窗口（`src-tauri/src/windows/island.rs`）
- 窗口配置：transparent + decorations false + alwaysOnTop + skipTaskbar
- 前端组件：`src/components/island/IslandWidget.tsx`（shadcn/ui + Tailwind）
- 数据同步：providerStore -> Tauri event -> island window

**数据流：**
```
providerStore (主窗口) -> Tauri event -> island window -> IslandWidget 显示
```

**性能：**
- 灵动岛窗口的 backdrop-filter 拖动时禁用
- 数据更新防抖（用量变化延迟 500ms 同步）

### 3.2 阶段 C：窗口层级提升

**目标：** 窗口总是在最前面。

**实现：**
- Tauri `alwaysOnTop: true`（已实现）
- 如不够，用 Windows 原生 API 提升到 HWND_TOPMOST
- 设置页提供"窗口置顶"开关，默认开
- 灵动岛也置顶，可单独开关

**跨平台：**
- Linux：X11/Wayland 行为不同，需测试
- macOS：正常

### 3.3 阶段 C 细节优化

- 全面 UI/UX 审核（间距、对齐、颜色、动画）
- 微交互优化（hover 效果、过渡动画）
- 无障碍（键盘导航、aria 标签）

---

## 风险与对策

| 风险 | 对策 |
|---|---|
| Tailwind 4 生态不成熟 | Tailwind 4 已正式发布，Vite 插件稳定；如有问题回退到 Tailwind 3 |
| shadcn/ui 组件定制冲突 | shadcn/ui 组件是模板，可完全定制，不会有库版本冲突 |
| 重构期间功能回归 | 每阶段单独验证，保留旧组件逻辑，只换样式 |
| 拖动卡顿修复不彻底 | 先用 backdrop-filter 方案，如不够再考虑窗口配置调整 |
| 灵动岛窗口性能 | 数据同步防抖，灵动岛窗口小，渲染开销低 |

---

## 参考

- Tailwind 4 文档：https://tailwindcss.com/docs
- shadcn/ui 文档：https://ui.shadcn.com/
- macOS Dynamic Island 设计参考
