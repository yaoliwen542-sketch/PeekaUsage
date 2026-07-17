# UI 框架重构（阶段 A）实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 引入 Tailwind 4 + shadcn/ui，重构设置页和主界面核心组件，修复拖动卡顿。

**Architecture:** 渐进式重构：先接入 Tailwind 4 + shadcn/ui 基础，再逐个重构组件（保留逻辑换样式），最后统一处理拖动卡顿（拖动时禁用 backdrop-filter）。

**Tech Stack:** React 18、Tailwind CSS 4、shadcn/ui（Radix UI）、lucide-react

## Global Constraints

- 语言规范：所有代码注释、错误提示、新增文档必须中文（AGENTS.md）
- 现有功能保留：拖拽排序、轮询、保存链路、多供应商、透明度、边缘吸附全部保留
- 现有 CSS 变量设计系统不保留，Tailwind 用 @theme 接管
- 组件逻辑不动，只换样式（Tailwind class 替换 CSS 类）
- 跨平台：Windows/Linux/macOS 兼容
- 提交约束：不把代码改了但文档没更新的状态提交

## File Structure

### 新建文件
- `src/index.css` - Tailwind 4 入口（@import "tailwindcss" + @theme 设计 token）
- `components.json` - shadcn/ui 配置
- `src/lib/utils.ts` - cn() 工具函数
- `src/components/ui/` - shadcn/ui 组件模板（button/card/input/select/dialog/switch/slider/badge/tooltip/scroll-area/separator/tabs）

### 改造文件
- `vite.config.ts` - 加 @tailwindcss/vite 插件
- `package.json` - 加 Tailwind/shadcn 依赖
- `src/components/common/TitleBar.tsx` - Tailwind 重写样式
- `src/components/widget/WidgetContainer.tsx` - Tailwind + Card 重写
- `src/components/widget/ProviderCard.tsx` - shadcn/ui Card 重写
- `src/components/common/AppSelect.tsx` - 替换为 shadcn/ui Select
- `src/components/common/ConfirmDialog.tsx` - 替换为 shadcn/ui Dialog
- `src/components/settings/SettingsPanel.tsx` - Tailwind + Tabs 重写
- `src/components/settings/ProviderConfig.tsx` - shadcn/ui Card 重写
- `src/assets/styles/widget.css` - 重构完成的组件样式删除
- `src/assets/styles/settings.css` - 同上
- `src/assets/styles/main.css` - 同上
- `src/App.tsx` - 引入 index.css，去掉旧 CSS 引入

---

## Task 1: 安装 Tailwind 4 + 初始化配置

**Files:**
- Modify: `package.json`
- Modify: `vite.config.ts`
- Create: `src/index.css`

**Interfaces:**
- Produces: Tailwind 4 基础配置，后续任务用 Tailwind class

- [ ] **Step 1: 安装 Tailwind 4 和 Vite 插件**

```bash
cd D:/Project/PeekaUsage
npm install tailwindcss @tailwindcss/vite
```

验证：
```bash
npx tailwindcss --version
```
Expected: 输出 Tailwind CSS 版本号（4.x）

- [ ] **Step 2: 配置 vite.config.ts**

在 `vite.config.ts` 的 plugins 数组中加 `tailwindcss()`：

```typescript
import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";

export default defineConfig({
  plugins: [react(), tailwindcss()],
  // ... 其它配置不变
});
```

- [ ] **Step 3: 创建 src/index.css（Tailwind 入口）**

创建 `src/index.css`：

```css
@import "tailwindcss";

/* 设计 token（Tailwind 4 @theme） */
@theme {
  /* 背景层级 */
  --color-background: oklch(0.11 0.01 285);
  --color-surface: oklch(0.16 0.01 285);
  --color-surface-elevated: oklch(0.21 0.01 285);

  /* 主色 */
  --color-primary: oklch(0.65 0.15 240);
  --color-primary-foreground: oklch(0.95 0.02 240);

  /* 边框 */
  --color-border: oklch(0.25 0.01 285);
  --color-border-strong: oklch(0.35 0.01 285);

  /* 文字 */
  --color-text: oklch(0.95 0.02 285);
  --color-text-secondary: oklch(0.7 0.02 285);
  --color-text-tertiary: oklch(0.5 0.02 285);

  /* 状态色 */
  --color-success: oklch(0.65 0.15 150);
  --color-warning: oklch(0.65 0.15 80);
  --color-error: oklch(0.65 0.15 20);

  /* 圆角 */
  --radius-sm: 0.25rem;
  --radius-md: 0.5rem;
  --radius-lg: 0.75rem;
  --radius-xl: 1rem;

  /* 阴影 */
  --shadow-sm: 0 1px 2px rgba(0, 0, 0, 0.1);
  --shadow-md: 0 4px 12px rgba(0, 0, 0, 0.15);
  --shadow-lg: 0 8px 24px rgba(0, 0, 0, 0.2);
}

/* 暗黑模式（默认深色，现有应用就是深色主题） */
@custom-variant dark (&:where(.dark, .dark *));
```

- [ ] **Step 4: 在 App.tsx 引入 index.css**

在 `src/App.tsx` 顶部（或现有 CSS 引入处）加：

```typescript
import "./index.css";
```

注意：现有的 `main.css` / `widget.css` / `settings.css` 暂时保留，后续任务逐个删除。

- [ ] **Step 5: 验证 Tailwind 生效**

启动 `npm run dev`，在浏览器打开 http://localhost:1420，打开 DevTools Console 输入：
```javascript
document.querySelector('div')?.classList.add('bg-red-500');
```
预期：如果 Tailwind 生效，某个 div 会变红。

- [ ] **Step 6: 提交**

```bash
git add package.json package-lock.json vite.config.ts src/index.css src/App.tsx
git commit -m "feat(ui): 接入 Tailwind 4 + Vite 插件"
```

---

## Task 2: 初始化 shadcn/ui

**Files:**
- Create: `components.json`
- Create: `src/lib/utils.ts`
- Create: `src/components/ui/`（多个组件模板）

**Interfaces:**
- Consumes: Tailwind 4（Task 1）
- Produces: shadcn/ui 组件库，后续任务用

- [ ] **Step 1: 初始化 shadcn/ui**

```bash
cd D:/Project/PeekaUsage
npx shadcn@latest init
```

CLI 会问几个问题，按以下选择：
- TypeScript: Yes
- Style: New York
- Base color: Zinc
- CSS variables: Yes
- Tailwind config: `src/index.css`
- Import alias: `@/components`

初始化后创建：
- `components.json`
- `src/lib/utils.ts`（cn() 函数）

- [ ] **Step 2: 添加需要的组件**

```bash
npx shadcn@latest add button card input select dialog switch slider badge tooltip scroll-area separator tabs
```

这会在 `src/components/ui/` 创建对应组件模板。

- [ ] **Step 3: 验证组件可用**

在任意组件里 import 并使用：
```typescript
import { Button } from "@/components/ui/button";

<Button variant="default">测试</Button>
```

- [ ] **Step 4: 提交**

```bash
git add components.json src/lib/ src/components/ui/
git commit -m "feat(ui): 初始化 shadcn/ui + 基础组件"
```

---

## Task 3: 重构 TitleBar

**Files:**
- Modify: `src/components/common/TitleBar.tsx`

**Interfaces:**
- Consumes: Tailwind 4（Task 1）、shadcn/ui（Task 2）

- [ ] **Step 1: 读现有 TitleBar.tsx**

先读 `src/components/common/TitleBar.tsx`，理解当前结构和样式类名。

- [ ] **Step 2: 用 Tailwind 重写样式**

把现有的 CSS 类名替换成 Tailwind class。参考现有样式变量（颜色、间距、圆角）映射到 Tailwind 设计 token。

示例（具体类名按实际样式调整）：
```tsx
// 旧
<div className="titlebar">
  <div className="titlebar-drag-region">
    <button className="titlebar-btn titlebar-btn-close">×</button>
  </div>
</div>

// 新
<div className="flex h-8 items-center justify-between bg-surface border-b border-border">
  <div className="flex-1 h-full" style={{ WebkitAppRegion: "drag" } as React.CSSProperties} />
  <button className="h-8 w-8 flex items-center justify-center hover:bg-error/20 transition-colors">×</button>
</div>
```

保留所有逻辑（拖拽区域、按钮点击、窗口控制），只改样式。

- [ ] **Step 3: 验证**

运行 `npm run dev`，确认标题栏显示正常、拖动窗口功能正常。

- [ ] **Step 4: 提交**

```bash
git add src/components/common/TitleBar.tsx
git commit -m "refactor(ui): TitleBar 用 Tailwind 重写样式"
```

---

## Task 4: 重构主界面容器 WidgetContainer

**Files:**
- Modify: `src/components/widget/WidgetContainer.tsx`

- [ ] **Step 1: 读现有 WidgetContainer.tsx**

理解当前布局结构（卡片容器、底部按钮、拖拽排序）。

- [ ] **Step 2: 用 Tailwind + shadcn/ui Card 重写**

把卡片容器改为 shadcn/ui Card 组件，样式用 Tailwind class。

示例：
```tsx
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";

// 旧
<div className="provider-card">
  <div className="provider-card-header">...</div>
  <div className="provider-card-body">...</div>
</div>

// 新
<Card className="bg-surface border-border">
  <CardHeader className="pb-2">
    <CardTitle>...</CardTitle>
  </CardHeader>
  <CardContent>...</CardContent>
</Card>
```

保留拖拽排序逻辑（react-dnd 或现有拖拽实现）和底部按钮。

- [ ] **Step 3: 验证**

确认主界面卡片显示正常、拖拽排序正常、底部按钮正常。

- [ ] **Step 4: 提交**

```bash
git add src/components/widget/WidgetContainer.tsx
git commit -m "refactor(ui): WidgetContainer 用 Tailwind + shadcn/ui Card 重写"
```

---

## Task 5: 重构 ProviderCard

**Files:**
- Modify: `src/components/widget/ProviderCard.tsx`

- [ ] **Step 1: 读现有 ProviderCard.tsx**

理解卡片结构（图标、名称、用量显示、进度条、状态）。

- [ ] **Step 2: 用 shadcn/ui Card + Progress + Badge 重写**

```tsx
import { Card, CardContent, CardHeader } from "@/components/ui/card";
import { Progress } from "@/components/ui/progress";
import { Badge } from "@/components/ui/badge";

// 供应商卡片结构用 shadcn/ui 组件替换
```

保留所有数据展示逻辑（balance/subscription/rate limit 显示）。

- [ ] **Step 3: 验证**

确认卡片显示正常、进度条正常、状态徽章正常。

- [ ] **Step 4: 提交**

```bash
git add src/components/widget/ProviderCard.tsx
git commit -m "refactor(ui): ProviderCard 用 shadcn/ui 重写"
```

---

## Task 6: 重构 AppSelect（替换为 shadcn/ui Select）

**Files:**
- Modify: `src/components/common/AppSelect.tsx`

- [ ] **Step 1: 读现有 AppSelect.tsx**

理解自定义下拉实现（portal、分组、选项渲染）。

- [ ] **Step 2: 用 shadcn/ui Select 重写**

```tsx
import {
  Select,
  SelectContent,
  SelectGroup,
  SelectItem,
  SelectLabel,
  SelectTrigger,
  SelectValue,
} from "@/components/ui/select";
```

shadcn/ui Select 基于 Radix UI，支持分组（SelectGroup + SelectLabel），保留 portal 到 body。

保留现有分组逻辑（官方订阅/用量查询/余额查询/中转网关/自定义）。

- [ ] **Step 3: 验证**

确认下拉显示正常、分组正常、选项图标正常。

- [ ] **Step 4: 提交**

```bash
git add src/components/common/AppSelect.tsx
git commit -m "refactor(ui): AppSelect 替换为 shadcn/ui Select"
```

---

## Task 7: 重构 ConfirmDialog 和 ProviderWizardDialog

**Files:**
- Modify: `src/components/common/ConfirmDialog.tsx`
- Modify: `src/components/settings/ProviderWizardDialog.tsx`

- [ ] **Step 1: 用 shadcn/ui Dialog 重写**

```tsx
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
```

ConfirmDialog 改为 Dialog 组件，ProviderWizardDialog 的 3 步向导用 Dialog + 自定义步骤指示器。

- [ ] **Step 2: 验证**

确认弹层显示正常、portal 到 body、动画正常。

- [ ] **Step 3: 提交**

```bash
git add src/components/common/ConfirmDialog.tsx src/components/settings/ProviderWizardDialog.tsx
git commit -m "refactor(ui): Dialog 组件替换为 shadcn/ui Dialog"
```

---

## Task 8: 重构 SettingsPanel 和 ProviderConfig

**Files:**
- Modify: `src/components/settings/SettingsPanel.tsx`
- Modify: `src/components/settings/ProviderConfig.tsx`

- [ ] **Step 1: SettingsPanel 用 Tailwind + Tabs 重写**

```tsx
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
```

设置页子导航改为 Tabs 组件。

- [ ] **Step 2: ProviderConfig 用 shadcn/ui Card + Input + Switch 重写**

```tsx
import { Input } from "@/components/ui/input";
import { Switch } from "@/components/ui/switch";
```

- [ ] **Step 3: 验证**

确认设置页显示正常、子导航切换正常、供应商配置卡片正常。

- [ ] **Step 4: 提交**

```bash
git add src/components/settings/SettingsPanel.tsx src/components/settings/ProviderConfig.tsx
git commit -m "refactor(ui): SettingsPanel 和 ProviderConfig 用 shadcn/ui 重写"
```

---

## Task 9: 修复拖动卡顿（重构时统一解决）

**Files:**
- Modify: `src/components/common/TitleBar.tsx`
- Modify: `src/assets/styles/widget.css`
- Modify: `src/assets/styles/settings.css`
- Modify: `src/App.tsx`

- [ ] **Step 1: 加 CSS 自定义属性控制 backdrop-filter**

在 `src/index.css` 或 `main.css` 加：

```css
:root {
  --backdrop-blur: 18px;
}

/* 受影响元素改为引用变量 */
.provider-card-overlay {
  backdrop-filter: blur(var(--backdrop-blur));
}

.settings-overlay {
  backdrop-filter: blur(var(--backdrop-blur));
}
```

- [ ] **Step 2: TitleBar 拖动时动态改变量**

在 `TitleBar.tsx` 的拖拽逻辑里：

```typescript
function handleDragStart() {
  document.documentElement.style.setProperty("--backdrop-blur", "0px");
}

function handleDragEnd() {
  document.documentElement.style.setProperty("--backdrop-blur", "18px");
}
```

- [ ] **Step 3: 验证**

拖动窗口时观察：拖动过程中 backdrop-filter 应该被禁用（不模糊），停止后恢复。

- [ ] **Step 4: 提交**

```bash
git add src/components/common/TitleBar.tsx src/assets/styles/ src/App.tsx src/index.css
git commit -m "perf(ui): 拖动时动态禁用 backdrop-filter 修复卡顿"
```

---

## Task 10: 清理旧 CSS + 最终验证

**Files:**
- Modify: `src/assets/styles/main.css`
- Modify: `src/assets/styles/widget.css`
- Modify: `src/assets/styles/settings.css`

- [ ] **Step 1: 删除已重构组件的旧样式**

从旧 CSS 里删除已被 Tailwind 替换的组件样式。

- [ ] **Step 2: 全量验证**

```bash
cd D:/Project/PeekaUsage
npx tsc --noEmit
cd src-tauri
cargo fmt --all --check
cargo check
```

- [ ] **Step 3: 手动验证关键路径**

- 主界面卡片显示正常
- 拖拽排序正常
- 设置页保存正常
- 透明度滑杆正常
- 拖动窗口不卡顿

- [ ] **Step 4: 提交**

```bash
git add src/assets/styles/
git commit -m "refactor(ui): 清理旧 CSS，完成阶段 A UI 重构"
```

---

## 完成标准

阶段 A 完成的标志：
1. Tailwind 4 + shadcn/ui 成功接入
2. 设置页和主界面核心组件用 shadcn/ui 重写
3. 拖动窗口不卡顿（backdrop-filter 动态禁用生效）
4. 所有现有功能保留（拖拽排序、轮询、保存、多供应商、透明度）
5. tsc / cargo check 通过
6. 旧 CSS 清理完成

## 后续阶段

- **阶段 B**：悬浮窗灵动岛（新窗口 + shadcn/ui 组件）
- **阶段 C**：窗口层级提升 + 细节优化 + 全面 UI/UX 审核
