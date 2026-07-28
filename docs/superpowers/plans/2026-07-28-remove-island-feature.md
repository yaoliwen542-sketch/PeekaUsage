# 删除灵动岛功能实施计划

> **给执行代理：**按任务顺序执行，每个任务完成后运行对应验证命令。

**目标：**彻底移除灵动岛窗口、组件、位置持久化、显隐设置和托盘入口，只保留并验证主窗口及其边缘隐藏/展开功能。

**架构：**Tauri 只保留 `main` 窗口；React 入口只渲染主界面；设置、托盘和配置不再维护灵动岛状态。主窗口现有 `useEdgeDock`、窗口尺寸/位置持久化和设置页继续保留，不改其行为。

**技术栈：**Tauri v2、Rust、React、TypeScript、Node 内置测试。

## 约束

- 所有新增或修改的对话、注释、错误提示和文档使用中文。
- 不新增替代浮窗，不保留无调用方的灵动岛兼容代码。
- 普通设置保存不再包含灵动岛字段；旧配置中的未知字段由 serde 读取时忽略并在下一次保存时清理。
- 主窗口边缘隐藏/展开逻辑必须继续通过 `useEdgeDock.ts` 工作。

## 任务

### 任务 1：建立删除后的回归边界

**文件：**

- 新增：`test/no-island-feature.test.ts`

**验证：**先运行 `npm.cmd test -- test/no-island-feature.test.ts`，确认现有仓库因仍存在 island 窗口、入口和配置字段而失败；删除完成后该测试应通过。

### 任务 2：删除 Tauri 窗口和 Rust 状态链路

**文件：**

- 修改：`src-tauri/tauri.conf.json`
- 删除：`src-tauri/capabilities/island.json`
- 修改：`src-tauri/src/lib.rs`
- 修改：`src-tauri/src/config/app_config.rs`
- 修改：`src-tauri/src/commands/settings_commands.rs`
- 修改：`src-tauri/src/tray/mod.rs`

移除 island 窗口定义、启动恢复、`islandVisible`、位置/吸附边字段、独立保存命令和托盘显隐菜单；保留托盘显示主窗口、刷新、设置和退出菜单。

### 任务 3：删除 React 入口、组件、IPC 和设置项

**文件：**

- 修改：`src/main.tsx`
- 删除：`src/components/island/IslandWidget.tsx`
- 删除：`src/assets/styles/island.css`
- 删除：`src/utils/islandBounds.ts`
- 删除：`test/islandBounds.test.ts`
- 修改：`src/App.tsx`
- 修改：`src/stores/providerStore.ts`
- 修改：`src/stores/settingsStore.ts`
- 修改：`src/types/settings.ts`
- 修改：`src/utils/ipc.ts`
- 修改：`src/components/settings/SettingsPanel.tsx`

入口只加载主窗口样式；删除 island 事件同步、位置保存 IPC、显隐设置和相关类型；保留主窗口用量刷新、窗口控制和边缘隐藏。

### 任务 4：清理文案和文档引用

**文件：**

- 修改：`src/i18n/messages.ts`
- 修改：`src/i18n/windowLabels.ts`
- 按实际需要更新：`README.md`、`CLAUDE.md`、`AGENTS.md`
- 删除：`docs/superpowers/plans/2026-07-28-island-stability-and-docking.md`

移除灵动岛设置和专用文案；不改历史设计文档中的历史记录，避免无关大范围重写。

### 任务 5：验证主窗口交付物

运行：

```bash
npm.cmd test
npm.cmd run typecheck
cargo check
npm.cmd run build
git diff --check
```

确认 Tauri 配置只创建主窗口，主窗口边缘隐藏代码仍被 `App.tsx` 使用，且不再有 `island` 运行时引用。
