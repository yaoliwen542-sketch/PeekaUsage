# AGENTS.md

## 语言规范

以下内容必须使用中文：

- 所有对话回复
- 代码注释
- 错误提示
- 新增或更新的文档

## 项目定位

这是一个 Tauri v2 桌面浮窗，用来监控 OpenAI、Anthropic、OpenRouter 的 API 用量和订阅计划消耗。

- Rust 后端负责 provider、配置、托盘、窗口命令、密钥存储
- React 前端负责主界面、设置页、轮询、拖拽排序和交互反馈

## 你接手前先知道的当前状态

最近几次改动已经落地，不要按旧逻辑继续开发。

### 1. OpenAI OAuth 检测已修复

文件：`src-tauri/src/commands/window_commands.rs`、`src-tauri/src/providers/oauth_detect.rs`

`~/.codex/auth.json` 的 `tokens.access_token` 现在可能是：

- 直接字符串
- 索引对象

必须同时兼容，不能再只按对象格式解析。

当前要求：

- `~/.codex/auth.json` 仅在 `auth_mode == "chatgpt"` 时才视作可用 OAuth 凭据（其它 mode 直接返回 None）
- `tokens.account_id` 也要一并解析，作为 `ChatGPT-Account-Id` header 发送给 ChatGPT 后端（多账号场景必须）
- 两条解析链路保持一致：
  - `commands::window_commands::detect_oauth_tokens`（Tauri 命令，面向前端，返回带 environment/source 元信息的 `DetectedTokens`，支持 Windows 原生 + WSL）
  - `providers::oauth_detect::detect_openai`（纯函数，供后端订阅查询时同步调用，仅读当前系统原生凭据文件）
- `DetectedToken` 结构新增 `accountId` 字段（camelCase 序列化），Anthropic 恒为 null，OpenAI 从 `tokens.account_id` 读取

### 2. 设置页保存链路已修复

文件：

- `src/components/settings/ProviderConfig.tsx`
- `src/components/settings/SettingsPanel.tsx`
- `src-tauri/src/commands/provider_commands.rs`

当前要求：

- 禁用供应商并保存后，主界面卡片必须同步消失
- 清空 key/token 保存后，旧凭据必须被真正清除
- 保存必须有明确反馈

### 3. 托盘逻辑已修复

文件：

- `src-tauri/src/tray/mod.rs`
- `src-tauri/tauri.conf.json`

当前要求：

- 只能有一个托盘图标
- 托盘由 Rust 手动创建
- 左键单击只处理一次
- 显示窗口前先 `unminimize()`
- 不要把自动托盘配置重新加回 `tauri.conf.json`

### 4. 主界面支持拖拽排序

文件：

- `src/components/widget/WidgetContainer.tsx`
- `src-tauri/src/config/app_config.rs`
- `src-tauri/src/commands/provider_commands.rs`
- `src/utils/ipc.ts`

当前要求：

- 卡片拖动时要有实时碰撞推挤效果
- 松手后保存布局
- 排序写入 `provider_order`
- 刷新和重启后顺序保持一致

### 5. 供应商官方图标已接入

文件：

- `src/components/common/ProviderIcon.tsx`
- `src/components/widget/ProviderCard.tsx`
- `src/components/settings/ProviderConfig.tsx`
- `src/assets/provider-icons/`

当前要求：

- 主界面和设置界面的供应商名字前都显示图标
- 图标路径统一走 `ProviderIcon.tsx`
- 图标文件命名统一为 `openai.*`、`anthropic.*`、`openrouter.*`
- 后续替换图标优先只替换 `src/assets/provider-icons/` 中的资源文件

### 6. 设置页移除供应商已改为应用内确认弹层

文件：

- `src/components/common/ConfirmDialog.tsx`
- `src/components/settings/ProviderConfig.tsx`

当前要求：

- 不再使用 `window.confirm()` 原生确认框
- 弹层必须走应用内样式
- 弹层不显示标题，只显示说明和操作按钮
- 弹层要能在小窗口中自适应，不能被设置卡片裁切
- 弹层通过 `React portal` 挂到 `body`

### 7. 设置页下拉框已改为跨平台自定义组件

文件：

- `src/components/common/AppSelect.tsx`
- `src/components/settings/ProviderConfig.tsx`
- `src/components/settings/SettingsPanel.tsx`

当前要求：

- 不要继续依赖原生 `<select>` 做核心设置交互
- 暗黑模式下背景、边框、浮层风格必须与应用统一
- “新增供应商”下拉项前必须显示供应商图标
- 图标仍然统一通过 `ProviderIcon.tsx` 渲染
- 下拉浮层通过 `React portal` 挂到 `body`
- 要考虑 Windows、Linux、macOS 的一致性

### 8. 刷新间隔已支持秒 / 分钟 / 仅手动

文件：

- `src/components/settings/SettingsPanel.tsx`
- `src/components/widget/ProviderCard.tsx`
- `src/composables/usePolling.ts`
- `src/composables/useProviders.ts`
- `src/types/settings.ts`
- `src-tauri/src/config/app_config.rs`

当前要求：

- 刷新设置由 `pollingMode`、`pollingInterval`、`pollingUnit` 共同决定
- 自动刷新时允许自定义数值，并可切换按秒或按分钟
- 选择“仅手动”后不能继续启动定时轮询
- 设置页里的刷新配置要保持紧凑，优先用小尺寸分段按钮和窄输入框，避免在小窗口中过度占位
- 设置页高级区域可开启“按供应商独立刷新”，开启后仅对已配置供应商显示单独策略
- 分供应商策略支持自动 / 手动、秒 / 分、自定义数值，并在未单独修改时沿用全局策略
- 每张供应商卡片右上角都有单独刷新按钮，只刷新当前供应商
- 主界面底部手动刷新按钮和托盘刷新仍然可用
- 旧配置缺少新字段时，要继续按“5 分钟自动刷新”兼容

### 9. 设置页已支持透明度调节条

文件：

- `src/components/settings/SettingsPanel.tsx`
- `src/composables/useWindowControls.ts`
- `src/App.tsx`

当前要求：

- 设置页提供透明度滑杆
- 拖动时即时预览，松手后持久化到 `windowOpacity`
- 主界面右侧透明度拖拽把手与设置页滑杆共用同一套状态
- 应用启动后要按保存的透明度恢复
- 当前数值语义是“不透明度/可见度”：`100%` 表示完全不透明

### 10. 设置页已支持开机自动启动

文件：

- `src/components/settings/SettingsPanel.tsx`
- `src/utils/autostart.ts`
- `src/i18n/messages.ts`
- `src-tauri/src/lib.rs`
- `src-tauri/capabilities/default.json`
- `src-tauri/Cargo.toml`
- `package.json`

当前要求：

- 设置页“通用”里提供“开机自动启动”开关
- 位置固定在“透明度”后，“返回时刷新主界面”前
- 持久化字段是 `launchAtStartup`
- 点击开关时不能只改本地配置，必须同步调用 Tauri autostart 插件更新系统开机自启状态
- Rust 侧必须注册 `tauri-plugin-autostart`
- `src-tauri/capabilities/default.json` 必须保留 `autostart:default` 权限
- 失败时不能把系统状态和已保存配置长期留在相反状态

### 11. 设置页顶部返回入口已改为图标按钮

文件：

- `src/components/settings/SettingsPanel.tsx`
- `src/App.tsx`
- `src/types/settings.ts`

当前要求：

- 不再显示紫色“返回”文字按钮
- 返回入口使用左箭头图标按钮
- 按钮尺寸、hover 和 focus 态要与应用整体风格一致
- “从设置返回时刷新主界面” 现在是设置页“通用”里的可勾选项
- 持久化字段是 `refreshOnSettingsClose`
- 默认不勾选；只有勾选后，从设置返回主界面才会触发一次全部供应商刷新
- 设置页“通用”里还提供“自动调整窗口高度以适应内容”开关
- 持久化字段是 `autoExpandWindowToFitContent`
- 开启后会按主界面内容变化自动调整窗口高度；内容变多时增高，内容变少时缩小；用户后续仍可手动调整

### 11.1 设置页已改为固定子导航分屏显示

文件：

- `src/components/settings/SettingsPanel.tsx`
- `src/assets/styles/settings.css`
- `src/i18n/messages.ts`

当前要求：

- 进入设置页时默认打开“通用”
- 左上角图标按钮恢复为直接返回主界面
- 标题下方提供固定可见的子导航，当前使用“通用 / 供应商 / 高级 / 更新”四个子项
- 点击子选项后只显示对应子页内容，不再把“通用 / 供应商 / 高级 / 更新”一次性全部渲染
- 子导航要保持紧凑、稳定，不要再回退成悬浮弹出菜单
- 子选项结构要可扩展，优先使用配置驱动的导航项和子页渲染映射，不要把切页逻辑写成到处散落的条件判断

### 12. 设置页已支持应用内更新

文件：

- `src/components/settings/SettingsPanel.tsx`
- `src/components/settings/UpdateSettings.tsx`
- `src/stores/updateStore.ts`
- `src/types/settings.ts`
- `src-tauri/src/commands/update_commands.rs`
- `src-tauri/src/lib.rs`
- `src-tauri/Cargo.toml`

当前要求：

- 设置页子导航固定包含“更新”分区
- 更新分区支持手动检查更新、查看当前版本、查看 Release 说明
- 检测到新版本后支持直接触发应用内更新安装
- 自动更新检查配置通过 `updateAutoCheckEnabled`、`updateCheckOnLaunch`、`updateCheckIntervalHours` 持久化
- 启动时仅在开启自动检查且允许启动检查时触发自动检测
- 不要重复注册 `tauri-plugin-updater` 或 `tauri-plugin-process`

### 13. GitHub 已接入 Windows Release 自动发布

文件：

- `.github/workflows/release.yml`
- `package.json`
- `src-tauri/tauri.conf.json`
- `src-tauri/Cargo.toml`

当前要求：

- 推送 `v*` 标签后自动构建并发布 Windows NSIS 安装包到 GitHub Release
- 发布前必须校验 `package.json`、`tauri.conf.json`、`Cargo.toml` 三处版本号一致
- 标签名必须与应用版本匹配，例如 `v0.1.0`

### 14. Linux 已接入 x86_64 构建与发布

文件：

- `.github/workflows/ci.yml`
- `.github/workflows/release.yml`
- `src-tauri/tauri.linux.conf.json`
- `package.json`

当前要求：

- Linux 打包目标统一放在 `src-tauri/tauri.linux.conf.json`
- 本地 Linux 打包使用 `npm run tauri:build:linux`
- GitHub Release 当前会上传 Linux `x86_64` 的 `deb` 和 `AppImage`
- Linux `arm64` 发布当前暂时关闭，不要在 release workflow 里默认恢复
- Linux CI / Release 的依赖安装要按 Tauri 官方 ARM 打包要求补齐，至少包含 `build-essential`、`curl`、`file`、`libfuse2`、`libgtk-3-dev`、`libssl-dev`、`libwebkit2gtk-4.1-dev`、`libayatana-appindicator3-dev`、`librsvg2-dev`、`patchelf`
- 不要把 Linux 的 `deb` / `appimage` 目标混回主 `tauri.conf.json`

### 15. macOS 已接入 x86_64 / arm64 构建与发布

文件：

- `.github/workflows/ci.yml`
- `.github/workflows/release.yml`
- `src-tauri/tauri.macos.conf.json`
- `package.json`

当前要求：

- macOS 打包目标统一放在 `src-tauri/tauri.macos.conf.json`
- 本地 macOS 打包使用 `npm run tauri:build:macos`
- GitHub Release 会同时上传 macOS `x86_64` / `arm64` 的 `app` 和 `dmg`
- 当前 macOS 产物未签名、未 notarize
- 如果安装后被提示“文件已损坏，无法打开”，文档里要明确提供 `xattr -dr com.apple.quarantine /Applications/PeekaUsage.app` 作为手动放行方案
- 不要把 macOS 的 `app` / `dmg` 目标混回主 `tauri.conf.json`

### 16. 应用标识已改为 PeekaUsage 并保留旧数据迁移

文件：

- `src-tauri/tauri.conf.json`
- `src-tauri/src/config/migration.rs`
- `src-tauri/src/lib.rs`

当前要求：

- 当前 Tauri `identifier` 是 `com.peekausage.desktop`
- 启动时会从旧标识 `com.ai-usage-peek.desktop` 的应用数据目录迁移 `config.json` 和 `keys.dat`
- 迁移只在新目录缺少对应文件时执行，不能覆盖新标识下已经存在的数据
- 如果后续继续改 `identifier`，必须同步更新迁移逻辑，不能只改配置不处理老用户数据

### 17. 多语言支持已接入

文件：

- `src/i18n/index.ts`
- `src/i18n/messages.ts`
- `src/components/settings/SettingsPanel.tsx`
- `src-tauri/src/config/app_config.rs`
- `src/types/settings.ts`

当前要求：

- 设置页“通用”里提供语言选择，顺序固定为“简体中文”“繁体中文”“English”
- 当前持久化字段是 `language`
- 当前仅支持 `zh-Hans`、`zh-Hant`、`en`
- 主界面、设置页和通用交互文案切换要即时生效
- 新增文案不要再直接散落在组件里，优先收敛到 `src/i18n/messages.ts`
- 旧配置缺少 `language` 时要继续兼容，默认按简体中文处理

### 18. 设置页 OAuth Token 区域已新增官方获取入口

文件：

- `src/components/settings/ProviderConfig.tsx`
- `src/i18n/messages.ts`

当前要求：

- `OAuth Token（订阅计划）` 输入框下方保留“自动检测”按钮
- “自动检测”右侧新增“获取方式”按钮，点击后打开当前供应商的官方认证文档
- Anthropic 跳转 `Claude Code Authentication` 官方文档
- OpenAI 跳转 `Codex Authentication` 官方文档
- 下方提示文案要区分“自动检测读取位置”和“官方获取方式”，不要再暗示本地文件是默认必然存在
- OpenAI 文案要兼容官方当前“可能写入 `~/.codex/auth.json`，也可能使用系统凭据库”的现状

### 19. 主界面底部已支持精简 / 详细显示模式

文件：

- `src/components/widget/WidgetContainer.tsx`
- `src/components/widget/ProviderCard.tsx`
- `src/assets/styles/widget.css`
- `src/i18n/messages.ts`
- `src/types/settings.ts`
- `src-tauri/src/config/app_config.rs`

当前要求：

- 主界面底部提供显示模式切换入口，使用紧凑的分段按钮
- 主界面底部提供显示模式切换入口，使用和其他底部按钮一致的单个图标按钮
- 持久化字段是 `widgetDisplayMode`
- 当前允许值只有 `detailed`、`compact`
- 默认按 `detailed` 处理
- 详细模式保持现有完整卡片结构
- 精简模式保留供应商图标、名称、单卡刷新，以及“标签 + 进度条 + 百分比”的横向摘要行
- 订阅摘要不显示订阅名和重置时间
- 精简模式要保留所有订阅窗口进度条，不能只保留利用率最高的一条
- OpenAI 订阅窗口在精简模式下要按窗口标签分别显示，例如 `5小时`、`7天`
- 多个 API Key 在精简模式下按一行一个显示
- 精简模式不显示逐 Key 的金额/余额明细块和 rate limit badge
- 如果 `autoExpandWindowToFitContent = true`，主界面内容高度变化时会自动调整窗口高度以尽量显示全貌
- 自动调整允许随内容增高或缩小，但不能在用户手动拖拽窗口大小时与手势打架
- 切换模式后刷新和重启都要保持所选显示方式
- 旧配置缺少 `widgetDisplayMode` 时要继续兼容，默认详细模式

### 20. 设置页已支持一键切换 API Key 到系统环境变量

文件：

- `src/components/settings/ProviderConfig.tsx`
- `src/components/settings/SettingsPanel.tsx`
- `src/utils/ipc.ts`
- `src/types/provider.ts`
- `src-tauri/src/commands/provider_commands.rs`
- `src-tauri/src/config/app_config.rs`
- `src-tauri/src/config/system_env.rs`

当前要求：

- 每个供应商的每个 API Key 都可以在设置页里单独点“切换环境”
- 只有用户显式点击后，应用才接管对应供应商的环境变量，不要因为普通保存就覆盖用户原本手动配置的系统环境变量
- 当前激活的 Key 要在设置页中明确显示“当前环境”
- 有未保存改动时，不允许直接切换环境变量，必须先保存，避免把旧值写进系统环境
- OpenAI / Anthropic / OpenRouter 对应的环境变量仍然分别是 `OPENAI_API_KEY`、`ANTHROPIC_API_KEY`、`OPENROUTER_API_KEY`
- Windows 需要写入用户级环境变量
- Linux / macOS 需要同步当前进程，并写入应用托管的 Shell 环境脚本；新开的终端会读取新值
- macOS 额外同步 `launchctl` 会话环境，Linux 当前以 Shell 启动链路为主，不要假设所有图形界面进程都能立即感知

### 21. 主窗口已支持拖拽到屏幕边缘后自动吸附收起

文件：

- `src/App.tsx`
- `src/components/common/TitleBar.tsx`
- `src/components/widget/WidgetContainer.tsx`
- `src/utils/windowBounds.ts`
- `src/assets/styles/main.css`

当前要求：

- 只有用户拖拽窗口到屏幕工作区边缘时，才允许触发吸附和收起
- 不能因为启动恢复窗口位置、代码里的程序化 `setPosition()` / `setSize()`，或者普通贴边状态就自动收起
- 必须是被拖拽方向对应的窗口边缘真正越过屏幕工作区边界后才允许收起，只是贴住边缘不触发
- 如果操作系统在贴边拖拽时触发了原生分屏/最大化并明显改写了窗口尺寸，要优先让系统行为生效，不再继续执行应用内收起
- 收起后窗口会缩成边缘细条；鼠标移入细条后自动展开，鼠标移出后再次收起
- 收起态不能覆盖正常窗口的 `windowSize` / `windowPosition` 持久化；持久化应继续保存展开态边界
- 收起态要暂停主界面按内容自动调高/调低窗口高度，避免和边缘细条状态打架
- 设置页“通用”里提供该功能开关，持久化字段是 `edgeDockCollapseEnabled`
- 该开关放在“自动调整窗口高度以适应内容”后；注意通用区其后还有紧凑色标（`compactColorMarkers`）和“显示灵动岛”（`islandVisible`），灵动岛开关固定为通用区最后一个条目

### 22. Anthropic 已支持更多订阅窗口与 Extra Usage 展示

文件：

- `src-tauri/src/providers/subscription.rs`
- `src-tauri/src/providers/types.rs`
- `src/types/provider.ts`
- `src/components/widget/ProviderCard.tsx`
- `src/i18n/messages.ts`

当前要求：

- Anthropic 订阅展示不再只看单一窗口
- 要兼容 `5 小时`、`7 天`、`7 天 Sonnet`、`7 天 Opus` 等多个订阅窗口
- 订阅窗口 label 必须使用机器常量（`five_hour` / `seven_day` / `seven_day_sonnet` / `seven_day_opus`），前端通过 `windowLabels` 映射成各语言文案；不能再硬编码中文 `"5小时"` / `"7天"` / `"7天(Sonnet)"`
- 如果 OAuth 返回 `extra_usage`，主界面也要展示 Extra Usage 的利用率
- 精简模式下也要保留这些额外窗口和 Extra Usage 的进度条

## 先读哪些文件

如果你是新的 coding agent，按这个顺序进入代码：

1. `README.md`
2. `CLAUDE.md`
3. `src-tauri/src/lib.rs`
4. `src-tauri/src/config/app_config.rs`
5. `src-tauri/src/commands/provider_commands.rs`
6. `src-tauri/src/commands/window_commands.rs`
7. `src-tauri/src/tray/mod.rs`
8. `src/App.tsx`
9. `src/i18n/index.ts`
10. `src/i18n/messages.ts`
11. `src/composables/useWindowControls.ts`
12. `src/stores/updateStore.ts`
13. `src-tauri/src/commands/update_commands.rs`
14. `src/components/settings/UpdateSettings.tsx`
15. `src/components/common/AppSelect.tsx`
16. `src/components/common/ConfirmDialog.tsx`
17. `src/components/widget/WidgetContainer.tsx`
18. `src/components/settings/ProviderConfig.tsx`
19. `src/components/settings/SettingsPanel.tsx`
20. `src-tauri/tauri.linux.conf.json`
21. `.github/workflows/ci.yml`
22. `.github/workflows/release.yml`
23. `src-tauri/tauri.macos.conf.json`

## 快速开发命令

```bash
npm install
npm run dev
npm run tauri dev
npx tsc --noEmit
cargo fmt --all --check
cargo check
npm run tauri:build:linux
npm run tauri:build:macos
```

发 Release 时使用：

```bash
# 先补齐 .github/release-notes/v0.1.0.md，再打标签
git tag v0.1.0
git push origin v0.1.0
```

如果 `cargo` 不在 PATH 中：

```bash
export PATH="$PATH:$HOME/.cargo/bin"
```

## 架构速记

### Rust

- `providers/traits.rs`：`UsageProvider` trait
- `providers/mod.rs`：`ProviderManager`
- `providers/subscription.rs`：OAuth 订阅查询
- `commands/provider_commands.rs`：配置、用量、顺序保存
- `commands/window_commands.rs`：OAuth 自动检测、窗口透明度命令
- `commands/update_commands.rs`：应用内更新检查、安装和版本查询
- `config/app_config.rs`：设置、供应商启停、`provider_order`
- `config/system_env.rs`：当前激活 API Key 到系统环境变量的同步
- `tray/mod.rs`：托盘

### React

- `App.tsx`：widget/settings 视图切换、启动时同步主题与透明度
- `useProviders.ts`：拉取和刷新编排
- `useWindowControls.ts`：窗口隐藏、最小化、透明度同步
- `providerStore`：主数据
- `settingsStore`：设置数据
- `updateStore`：更新状态、手动检查和安装流程
- `i18n`：语言包和运行时语言切换
- `ProviderIcon.tsx`：供应商图标共享组件
- `AppSelect.tsx`：跨平台自定义下拉组件
- `ConfirmDialog.tsx`：应用内确认弹层
- `WidgetContainer.tsx`：主界面卡片和拖拽排序
- `ProviderCard.tsx`：供应商卡片、单卡片刷新和精简/详细两套展示
- `ProviderConfig.tsx`：供应商设置卡片
- `SettingsPanel.tsx`：设置页容器、固定子导航、语言选择、全局刷新和高级分供应商刷新
- `UpdateSettings.tsx`：当前版本、检查更新、更新说明和应用内安装入口

## 核心约束

### 类型同步

以下两处必须同步修改：

- `src-tauri/src/providers/types.rs`
- `src/types/provider.ts`

Rust 使用 snake_case，TS 使用 camelCase，通过 serde 做映射。

### 托盘约束

- 不要创建第二个托盘
- 不要依赖 `tauri.conf.json` 自动托盘
- 处理左键点击时要注意 `MouseButtonState::Up`

### 图标约束

- 不要在多个页面分别写图标逻辑
- 统一通过 `ProviderIcon.tsx` 渲染
- 图标资源统一放在 `src/assets/provider-icons/`
- **应用 logo（2026-07 起）**是「仪表盘 + 瞳孔指针」：靛蓝→紫渐变圆角底板、白色仪表弧、轴心为瞳孔；品牌色取应用主色 `#6366f1 → #7c3aed`
- logo 源文件与再生成脚本在 `.logo-lab/`（`concept-b.svg` 标准版 / `concept-b-small.svg` ≤48px 加粗变体 / `master-*.png` 1024 母图 / `gen-icons.py` 一键重建 `src-tauri/icons/` 全套 PNG + `icon.ico` 多尺寸 + `icon.icns` 手工打包 + `icon.iconset`）；改 logo 只改 SVG 后重跑脚本，不要手改单个 PNG
- macOS 图标按 824/1024 安全边距渲染（`gen-icons.py` 已处理）；Windows 图标平铺

### 排序约束

- 排序不是纯前端状态
- 必须通过 IPC 存到后端 `provider_order`
- `fetch_all_usage` 的返回顺序必须受 `provider_order` 影响

### 设置保存约束

- 保存后前端状态要同步刷新
- 空字符串保存要真正清掉凭据
- 启用状态变化不能只停留在设置页

### 环境变量切换约束

- 环境变量切换是“显式动作”，不能绑定到普通保存按钮后自动执行
- 后端持久化字段是 `active_api_key_id` 和 `manage_api_key_environment`
- 只有 `manage_api_key_environment = true` 的供应商，应用才会继续同步对应环境变量
- 如果当前激活的 Key 被删除，应用要把自己管理的对应环境变量清掉
- 当前实现优先覆盖“新开的终端 / 新启动的进程”读取场景，不要承诺已运行进程会实时切换

### 下拉组件约束

- 涉及核心交互的设置下拉，优先复用 `AppSelect.tsx`
- 不要为供应商选择继续写原生 `<select>`
- 供应商选项中的图标必须继续走 `ProviderIcon.tsx`
- 小窗口下浮层不能被父容器裁切

### 轮询约束

- 刷新相关持久化字段是 `pollingMode`、`pollingInterval`、`pollingUnit`、`providerPollingOverridesEnabled`、`providerPollingOverrides`、`refreshOnSettingsClose`
- `pollingMode = manual` 时不能继续启动自动轮询
- 秒和分钟都属于自动刷新模式，不要再把 `pollingInterval` 固定解释成“分钟”
- 分供应商定时器要按每个供应商的生效策略独立调度，不能继续假设全局只有一个定时器
- 要兼容旧配置缺少新字段的情况，默认按“5 分钟自动刷新”处理

### 多语言约束

- 语言持久化字段是 `language`
- 允许值当前只有 `zh-Hans`、`zh-Hant`、`en`
- 设置页语言选项顺序固定为“简体中文”“繁体中文”“English”，不要擅自改顺序
- 新增界面文案时，优先写进 `src/i18n/messages.ts`，不要继续把可见文案硬编码在组件里
- 通用组件的默认占位符、按钮文案和 `aria-label` 也要跟随语言切换
- 旧配置缺少 `language` 时要继续兼容，默认简体中文

### 显示模式约束

- 显示模式持久化字段是 `widgetDisplayMode`
- 允许值当前只有 `detailed`、`compact`
- 详细模式保持现有完整卡片，不要偷偷删掉已有信息
- 精简模式只显示供应商级汇总，不显示逐 Key 明细和 rate limit badge
- 显示模式切换入口固定在主界面底部，交互要紧凑，且样式要与其他底部图标按钮保持一致
- 两套卡片显示逻辑优先收敛在 `ProviderCard.tsx`，不要散落到多个页面各写一套
- 旧配置缺少 `widgetDisplayMode` 时默认按详细模式兼容

### 透明度约束

- 透明度值持久化字段是 `windowOpacity`
- 设置页滑杆和主界面拖拽把手必须保持同步
- 拖动预览和最终保存要区分，避免每一帧都写配置
- 文案如果使用“透明度”，要注意当前实际语义更接近“不透明度”

### 开机自启约束

- 开机自启持久化字段是 `launchAtStartup`
- 设置页中的“开机自动启动”开关固定放在“透明度”后、“返回时刷新主界面”前
- 开机自启不是纯前端显示状态；切换时必须同步调用 `@tauri-apps/plugin-autostart`
- Rust 侧必须注册 `tauri-plugin-autostart`，并在 capability 中保留 `autostart:default`
- 如果系统开机自启同步失败，不能只把配置写成新值而把系统状态留在旧值

### 窗口尺寸约束

- 窗口大小与位置持久化字段是 `windowSize`、`windowPosition`
- 应用启动后要优先恢复已保存的窗口大小与位置
- 用户手动拖动或缩放窗口后，要把最新结果持久化
- 边缘吸附收起只能由标题栏拖拽到屏幕边缘触发，不能在启动恢复、托盘恢复或其他程序化移动后自动触发
- 边缘吸附收起要求窗口边缘越过对应工作区边界后才触发，不要在仅仅贴边时触发
- Windows 在窗口隐藏或最小化时可能上报离屏哨兵坐标（例如 `-21845`）；这类位置不能继续写回配置，也不能在启动时照单恢复
- `autoExpandWindowToFitContent = true` 时，要按内容变化自动调整高度，支持增高和缩小
- 自动调整窗口时优先保持当前宽度不变，只调整高度
- 用户手动拖拽窗口大小时，要暂时抑制自动调整，避免出现窗口抖动或反向拉扯
- 自动调整后的窗口大小也要写回持久化配置，保证刷新和重启后延续
- 边缘收起时不要把细条态的尺寸和位置写回配置，避免下次启动直接恢复成细条
- 进入设置页时窗口若小于 `SETTINGS_MIN_WIDTH/HEIGHT`（App.tsx，当前 420×540）会临时扩大并做工作区越界校正，返回主界面时恢复进入前尺寸；扩窗/恢复都必须 `markProgrammaticWindowResize()`，且边缘吸附收起/预览态跳过
- 设置页是独立的最小尺寸语义：不要把设置页扩窗后的尺寸当作主浮窗尺寸对待，返回时恢复触发的 onResized 防抖链路会让持久化自然收敛回浮窗尺寸

### 样式层叠约束（血泪教训）

- **严禁**在任何 CSS 文件里写未分层的通配 reset（如 `* { margin: 0; padding: 0 }`）
- Tailwind v4 的全部工具类都在 `@layer utilities` 中；未分层样式在层叠中**压过所有分层样式**，一条 `* { padding: 0 }` 会让全 app 的 `p-*` / `px-*` / `py-*` / `m-*` 工具类集体失效，且元素自身查不出任何异常（`getBoundingClientRect` 无溢出、类名都在）
- 历史上这个 bug 导致全 app 间距丢失、内容贴边/被裁，表面像「窗口太窄」，实际是 padding 被清零；手写组件类（如 `.titlebar` 自带 padding）不受影响，所以表现为「部分区域正常、部分区域贴边」
- 清零外边距/内边距已由 `index.css` 的 Tailwind preflight（`@layer base`）完成，不需要也不允许再写一份
- 手写组件样式如需与工具类共存，优先放进 `@layer components`，保证工具类永远可覆盖
- 排查布局诡异问题时的核武器：用 `WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS="--remote-debugging-port=9222"` 启动应用，走 CDP `Runtime.evaluate` 直接读计算样式（注意 websocket 要 `suppress_origin`）

### 跨平台约束

- 这个项目后续不只支持 Windows，还要兼容 Linux 和 macOS
- 新增交互组件时，优先选择前端可控、自绘、跨平台一致的实现
- 不要优先依赖 Windows 特有 API、系统控件外观或只在单平台稳定的行为
- 能复用现有共享组件时，优先复用 `AppSelect.tsx`、`ConfirmDialog.tsx`、`ProviderIcon.tsx`
- 如果必须做平台差异处理，要先确认是否真的不可避免，并在文档中补充说明
- `identifier` 会影响应用数据目录，改名时必须考虑 Windows、Linux、macOS 的旧数据迁移

### 发版约束

- 改版本号时，必须同步修改 `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml`
- GitHub Release 的标签名必须使用 `v` 前缀并与应用版本完全一致
- 每次发版必须同步提交对应的发版说明文件 `.github/release-notes/vX.Y.Z.md`
- 发版说明必须覆盖本次功能更新和修复内容，不能只发空 Release 或只上传安装包
- Release 流水线结束前还必须校验对应 tag 下的 `latest.json` 可访问且内容合法，避免应用内更新读取失败
- `latest.json` 必须统一由流水线最后的 `verify-updater` 任务生成上传；各构建任务必须保持 `uploadUpdaterJson: false`。matrix 并发下各任务各自上传会互相覆盖丢平台条目（v0.2.4 曾因此丢失 `darwin-aarch64`），不要回退成按任务各自生成
- 校验 `latest.json` 时必须确认 9 个平台条目齐全：`windows-x86_64`（+`-nsis`）、`linux-x86_64`（+`-appimage`、`-deb`）、`darwin-x86_64`（+`-app`）、`darwin-aarch64`（+`-app`）
- Windows 产物是 `nsis`
- Linux 产物当前是 `x86_64` 的 `deb` 和 `appimage`
- macOS 产物是 `x86_64` / `arm64` 的 `app` 和 `dmg`
- Linux 打包目标统一维护在 `src-tauri/tauri.linux.conf.json`
- macOS 打包目标统一维护在 `src-tauri/tauri.macos.conf.json`
- 当前 macOS 产物未签名、未 notarize
- 如果后续要补自动更新或 Apple Developer 签名，先更新文档再改流水线

## 常见排查点

### 托盘异常

检查：

- 是否重复创建托盘
- 是否漏了 `unminimize()`
- 是否把左键按下和抬起都当成一次切换

### OpenAI 自动检测异常

检查：

- `tokens.access_token` 实际是字符串还是对象
- `parse_codex_access_token()` 是否被走到
- 本地文件路径是否正确
- 是否当前机器把 Codex 凭据存到了系统凭据库而不是 `~/.codex/auth.json`
- `auth_mode` 是否为 `"chatgpt"`（其它 mode 时 `oauth_detect::detect_openai` 直接返回 None）
- 多账号场景下 `tokens.account_id` 是否存在；ChatGPT Wham 请求是否带上了 `ChatGPT-Account-Id` header

### OAuth 获取入口异常

检查：

- `ProviderConfig.tsx` 中“获取方式”按钮是否仍放在“自动检测”右侧
- Anthropic / OpenAI 跳转链接是否仍然是各自官方认证文档
- `@tauri-apps/plugin-opener` 是否已在前端接入，且 `src-tauri/capabilities/default.json` 仍保留 `opener:default`

### Kimi 用量解析异常

检查：

- Kimi usages 接口（`https://api.kimi.com/coding/v1/usages`）返回的 `limit` / `remaining` / `used` 是字符串（`"100"`）不是数字；`coding_plan.rs` 的 `utilization_from_quota` 必须通过 `json_number` 同时兼容两种形态
- Kimi 的 5 小时窗口打满后，`limits[0].detail` 会省略 `remaining` 只留 `limit` + `used`；窗口未激活时 `limits` 可能是空数组。`parse_kimi_response` 对这两种情况都必须兜底展示 `five_hour` 窗口（0%），不能让卡片上的 5 小时进度条时有时无
- `totalQuota` 在部分套餐（如 LEVEL_INTERMEDIATE）是空对象 `{}`，此时不展示月度窗口；返回有效额度时才渲染 `monthly` 窗口
- 用量查询链路始终使用应用内已保存的 Key 直接请求官方接口，不读系统环境变量；“切换环境”只是把 Key 写入系统环境变量供终端工具使用，不要给查询链路加环境变量依赖
- Coding Plan 类供应商（Kimi / GLM / MiniMax）的多窗口利用率放在 `UsageData.windows`（`five_hour` / `weekly_limit` / `monthly`），前端逐窗口渲染；不要把多窗口再压回单一 `total_used`（`total_used` 仅保留最高值用于兼容旧展示）
- 百分比型（`currency == "%"`）多 Key 聚合绝不能求和：两个 Key 各 70% 不等于合计 140%。`aggregate_usage_data` 对百分比型取各 Key 最高利用率（预算恒 100），金额型保持求和；卡片 hero 大数字与灵动岛统一按"five_hour 窗口优先、否则第一个窗口"取值，多 Key 时聚合 windows 已按标签取最高利用率
- 百分比型供应商卡片不显示"合计"行、不要用"按量 API"做标题（配额不是按量计费）

### 新增供应商下拉异常

检查：

- `SettingsPanel.tsx` 渲染创建模式 `ProviderConfig` 时，`selectableProviders` 是否传入了真实可选列表（含自定义草稿自身），不能回退成空数组——空数组会让 `AppSelect` 无选项打不开、选中项也无法回显

### 拖拽排序异常

检查：

- 拖拽结束后是否调用 `saveProviderOrder`
- `provider_order` 是否成功写入配置
- `get_enabled_providers()` 是否按保存顺序排序

### 设置保存后主界面不同步

检查：

- 保存后是否刷新 provider 列表
- disabled provider 是否还被主界面保留
- 被清空的 token/key 是否仍在 keystore 中残留

### 设置子导航异常

检查：

- 进入设置页时是否仍默认落在“通用”子页
- `SettingsPanel.tsx` 中的导航项配置和子页渲染映射是否保持同步
- 左上角按钮是否仍直接返回主界面
- 标题下方的固定子导航是否仍显示“通用 / 供应商 / 高级”
- 当前激活项高亮和子页切换是否正常

### 下拉框表现异常

检查：

- 是否误用了原生 `<select>`
- `AppSelect.tsx` 的浮层是否通过 `React portal` 挂到 `body`
- 暗黑模式样式是否仍在走应用 CSS 变量
- 供应商选项是否通过 `ProviderIcon.tsx` 显示图标

### 刷新异常

检查：

- `pollingMode` 当前是不是 `manual`
- `pollingUnit` 是否被正确解释为 `seconds` 或 `minutes`
- `providerPollingOverridesEnabled` 是否已开启且 `providerPollingOverrides` 是否写入了预期供应商
- `refreshOnSettingsClose` 是否按预期保存；未勾选时，从设置返回不应额外触发刷新
- `usePolling.ts` 是否按供应商重建、停止了对应定时器
- 卡片右上角单独刷新按钮是否调用了 `refreshProvider`
- 应用启动时旧配置是否被兼容成“5 分钟自动刷新”

### 环境变量切换异常

检查：

- `ProviderConfig.tsx` 里切换按钮是否在有未保存改动时被正确禁用
- `provider_commands.rs` 是否把 `active_api_key_id` 写回配置
- `system_env.rs` 是否只同步 `manage_api_key_environment = true` 的供应商
- Windows 下用户级环境变量是否成功写入
- Linux / macOS 下 `~/.peekausage/env.sh` 和对应 Shell 启动文件的 source 块是否存在
- macOS 下 `launchctl setenv` / `unsetenv` 是否执行成功

### 透明度异常

检查：

- `windowOpacity` 是否成功保存到设置
- `App.tsx` 启动时是否调用了透明度同步
- 设置页滑杆和 `useWindowControls.ts` 是否使用同一套状态
- 主界面透明度把手调整后是否同步写回设置

### 窗口离屏异常

检查：

- `config.json` 中的 `windowPosition` 是否被写成了类似 `-21845` 的离屏哨兵值
- `src/utils/windowBounds.ts` 是否仍在过滤隐藏/最小化时的异常坐标
- `src/App.tsx` 在保存窗口位置前是否跳过了无效坐标

### 边缘吸附收起异常

检查：

- `src/components/common/TitleBar.tsx` 是否仍然只在拖拽标题栏区域时登记拖拽意图，而不是点按钮也触发
- `src/App.tsx` 是否只在拖拽结束后的越界判定命中时才调用收起，而不是窗口一靠边就立刻收起
- `src/utils/windowBounds.ts` 的工作区边缘判定是否仍基于 monitor work area，而不是整块物理屏幕
- 如果系统原生分屏改写了窗口尺寸，应用是否仍然正确放弃自己的收起逻辑
- `src/components/widget/WidgetContainer.tsx` 在收起态是否暂停了自动高度适配
- 保存到配置里的 `windowSize` / `windowPosition` 是否仍然是展开态，而不是收起后的细条态

### 开机自启异常

检查：

- `src/components/settings/SettingsPanel.tsx` 里的开关位置是否仍在“透明度”后、“返回时刷新主界面”前
- `src/utils/autostart.ts` 是否仍然调用了 `enable()` / `disable()` / `isEnabled()`
- `src-tauri/src/lib.rs` 是否仍注册了 `tauri-plugin-autostart`
- `src-tauri/capabilities/default.json` 是否仍保留 `autostart:default`
- `launchAtStartup` 是否被正确写回配置

### 应用内更新异常

检查：

- `src-tauri/src/lib.rs` 是否只注册了一次 `tauri-plugin-updater` 和 `tauri-plugin-process`
- `src-tauri/src/commands/update_commands.rs` 是否仍暴露 `check_app_update`、`install_app_update`、`get_current_version`
- `src/stores/updateStore.ts` 是否正确同步 `hasUpdate`、`lastCheckAt` 和安装状态
- `src/components/settings/SettingsPanel.tsx` 的子导航里是否仍包含“更新”
- `src/components/settings/UpdateSettings.tsx` 是否还能打开 Release 页面并触发安装

### Release 发布异常

检查：

- `.github/workflows/release.yml` 是否仍然只在 `v*` 标签下触发
- `.github/release-notes/vX.Y.Z.md` 是否已存在且内容非空
- `https://github.com/StarChen4/PeekaUsage/releases/download/vX.Y.Z/latest.json` 是否可访问且包含合法 JSON
- `package.json`、`src-tauri/tauri.conf.json`、`src-tauri/Cargo.toml` 版本号是否一致
- 推送的标签是否与版本完全匹配，例如 `v0.1.0`
- GitHub Actions 是否具有 `contents: write` 权限
- `src-tauri/tauri.linux.conf.json` 是否仍然只负责 Linux `deb` / `appimage`
- Linux runner 是否安装了 `build-essential`、`curl`、`file`、`libfuse2`、`libgtk-3-dev`、`libssl-dev`、`libwebkit2gtk-4.1-dev`、`libayatana-appindicator3-dev`、`librsvg2-dev`、`patchelf`
- 如果后续要恢复 Linux `arm64`，先确认 runner、依赖和文档说明一起恢复
- `src-tauri/tauri.macos.conf.json` 是否仍然只负责 macOS `app` / `dmg`
- macOS job 是否仍然分别产出 `x86_64` / `arm64` 包
- 如果启用了签名或 notarization，相关密钥和证书配置是否完整
- `src-tauri/tauri.conf.json` 的 `identifier` 是否和迁移逻辑中的新旧标识保持一致

## 修改流程

涉及功能改动时，默认按下面流程执行：

1. 先改代码，不要只停留在分析。
2. 如果改动影响已落地行为、交互、约束、组件入口或排查方式，必须同步更新 `AGENTS.md` 和 `CLAUDE.md`。
3. 至少执行：
   - `npx tsc --noEmit`
   - `cargo check`
4. 如果改了交互，再补手动验证关键路径。
5. 确认无误后再提交。

额外要求：

- 不要把“代码改了但文档没更新”的状态提交出去。
- 不要把与本次任务无关的脏改动一并提交。
- 提交信息要能准确描述这次改动是修复、优化还是重构。
- 如果改动会影响 Windows、Linux、macOS 之一的表现，更新文档时要同步写明跨平台约束或取舍。

## 每次提交前至少做的验证

```bash
npx tsc --noEmit
cargo fmt --all --check
cargo check
```

如果改了交互，再手动验：

- 托盘左键、右键、最小化恢复
- 设置保存反馈
- 自动检测 OAuth
- 主界面拖拽推挤和顺序持久化
- 设置页全局刷新、分供应商刷新、秒/分钟切换和“仅手动”是否按预期生效
- 设置页“返回时刷新主界面”开关在勾选和未勾选两种情况下是否都符合预期
- 设置页“自动调整窗口高度以适应内容”开关在勾选和未勾选两种情况下是否都符合预期
- 设置页“拖拽到边缘后自动收起”开关在启用和关闭两种情况下是否都符合预期，且位置仍在“通用”区域最后一个
- 设置页“更新”分区里手动检查、自动检查开关、启动时检查和间隔设置是否按预期工作
- 主界面卡片右上角单独刷新按钮是否只刷新当前供应商
- 设置页自定义下拉在浅色/暗黑模式下的展开和关闭
- 设置页透明度滑杆和主界面透明度把手是否同步
- 设置页“开机自动启动”开关启用和关闭后，系统登录项是否随之变化，刷新或重启后是否保持
- 设置页切换简体中文、繁体中文、English 后，设置页和主界面文案是否即时同步
- 设置页默认是否先显示“通用”，左上角返回按钮和固定子导航切换是否都符合预期
- 主界面底部精简 / 详细切换后，卡片内容和高度是否按预期变化，刷新或重启后是否保持
- 手动拖动窗口位置、缩放窗口高度后，刷新或重启是否恢复到最新状态
- 把窗口拖到左 / 右 / 上 / 下边缘后，是否只有在对应边缘真正越过工作区边界并松手结束拖拽后才吸附收起；仅贴边时不应收起；悬停是否自动展开，移开后是否再次收起
- 收起后刷新或重启应用，窗口是否恢复为正常展开态边界，而不是细条态
- 设置页 API Key “切换环境”后，对应的系统环境变量是否更新；新开终端读取是否符合预期

如果改了 Linux 构建或发布链路，再额外确认：

- `.github/workflows/ci.yml` 的 Linux `x86_64` 检查仍然可跑
- `.github/workflows/release.yml` 的 Linux `x86_64` 产物仍然是 `deb` 和 `appimage`

如果改了 macOS 构建或发布链路，再额外确认：

- `.github/workflows/ci.yml` 的 macOS 检查仍然可跑
- `.github/workflows/release.yml` 的 macOS `x86_64` / `arm64` 产物仍然是 `app` 和 `dmg`

## 补充约束

### 主界面主题切换入口

文件：

- `src/components/widget/WidgetContainer.tsx`

当前要求：

- 主界面底部的主题切换按钮使用固定的半袖上衣图标，不随当前主题切换图标
- 点击后弹出的主题菜单保持紧凑，优先使用小尺寸列表项，不要再做大卡片式选项
- 主题菜单项只显示太阳、月亮、系统三个图标，不显示文字
- 三个主题选项横向排列，保持小浮窗下的紧凑占用
- 主题菜单的水平位置以主题按钮图标为中心，不再贴右对齐
- 主题菜单仍然只负责切换 `light`、`dark`、`system` 三种模式

### 主界面显示模式入口

文件：

- `src/components/widget/WidgetContainer.tsx`
- `src/components/widget/ProviderCard.tsx`

当前要求：

- 主界面底部显示模式切换使用单个图标按钮，点亮表示精简模式开启
- 详细模式保持当前完整展示
- 精简模式使用“标签 + 进度条 + 百分比”的横向行布局
- 精简模式下订阅不显示订阅名和重置时间
- 精简模式下要保留所有订阅窗口进度条，OpenAI 的 `5小时` / `7天` 之类窗口要分别显示
- 精简模式下多个 API Key 按一行一个显示
- 精简模式下 API 行标签优先显示用户自定义的 Key 名称
- 精简模式不显示逐 Key 的金额/余额明细块和 rate limit badge
- 切换结果写入 `widgetDisplayMode`，刷新和重启后都要恢复

### 23. 供应商架构已改为配置驱动 + 自定义供应商

文件：

- `src-tauri/src/providers/registry.rs`
- `src-tauri/src/providers/balance.rs`
- `src-tauri/src/providers/script_engine.rs`
- `src-tauri/src/providers/types.rs`
- `src-tauri/src/providers/mod.rs`
- `src-tauri/src/commands/provider_commands.rs`
- `src-tauri/src/config/app_config.rs`
- `src-tauri/src/config/system_env.rs`
- `src/types/provider.ts`
- `src/components/settings/ProviderWizardDialog.tsx`
- `src/components/settings/ProviderConfig.tsx`
- `src/components/settings/SettingsPanel.tsx`
- `src/components/widget/ProviderCard.tsx`
- `src/components/common/AppSelect.tsx`
- `src/i18n/messages.ts`

当前要求：

- `ProviderId` 已从枚举改为 `String`，新增供应商不再需要改枚举和 7 处 match
- 内置供应商模板统一在 `registry.rs` 注册，新增供应商只需追加一条 `ProviderTemplate`
- `ProviderManager` 通过 `registry::get()` 路由查询，不再硬编码 `match provider_id`
- 查询分四类：Balance（余额）/ CodingPlan（百分比，阶段 2）/ Subscription（OAuth）/ Script（JS 脚本）
- JS 脚本引擎使用 rquickjs 沙箱，安全边界：HTTPS 强制（可配置允许 HTTP）、同源校验、无网络/文件 API、超时上限 60s
- 自定义供应商通过 `ProviderWizardDialog` 3 步向导创建，配置存 `customConfig` 字段
- NewAPI 用 accessToken + userId（存 KeyStore），不是普通 API Key
- 订阅窗口 label 改为机器常量（`five_hour` / `seven_day` 等），前端通过 i18n 映射
- 错误处理区分瞬时（RequestError/RateLimited，保留旧值重试）和确定性（AuthError/ParseError，立即透出）
- HTTP 读体统一改为 `bytes().await` + `serde_json::from_slice`（bytes-then-parse 模式）
- 缓存策略：失败快照不写入，保留上次成功值
- 旧配置缺少 `providerTemplateId` / `customConfig` 字段时按 `providerId` 在 registry 兜底
- 阶段 1 已实现：OpenAI / Anthropic / OpenRouter 迁移 + DeepSeek + NewAPI + 自定义供应商
- 阶段 2-A 已实现：Kimi / GLM / MiniMax（CodingPlan）接入 registry
- 阶段 2-B 已实现：OAuth 凭据自动检测（`providers/oauth_detect.rs`）+ Claude `seven_day_opus` 窗口 + ChatGPT 请求补 `ChatGPT-Account-Id` header
- 阶段 2 剩余待实现：ZenMux（CodingPlan，建议走自定义供应商向导）
- 阶段 3 已实现：3-A SiliconFlow / StepFun / Novita、3-B 火山方舟 SigV4、3-C Gemini OAuth + refresh_token，registry 现内置 12 家
- Gemini 刷新后的 access_token / 新 refresh_token 缓存进 KeyStore（键 `gemini_access_token_cache`），禁止回写用户 `~/.gemini/oauth_creds.json`；查询按「文件 token → 缓存 token → refresh」解析，过期判定留 60 秒余量

### 24. OAuth 凭据自动检测 + ChatGPT-Account-Id + Claude seven_day_opus 已接入

文件：

- `src-tauri/src/providers/oauth_detect.rs`（新增）
- `src-tauri/src/providers/subscription.rs`
- `src-tauri/src/providers/mod.rs`
- `src-tauri/src/providers/registry.rs`
- `src-tauri/src/commands/window_commands.rs`
- `src/utils/ipc.ts`

当前要求：

- 新增 `providers::oauth_detect` 模块，提供纯函数式 OAuth 凭据检测能力：
  - `detect_anthropic()` - 读 `~/.claude/.credentials.json` 的 `claudeAiOauth.accessToken`（兼容旧 key `claude.ai_oauth`）；macOS 额外尝试 Keychain（service=`Claude Code-credentials`）
  - `detect_openai()` - 读 `~/.codex/auth.json`，仅 `auth_mode == "chatgpt"` 有效；返回 `tokens.access_token`（兼容字符串和索引对象）+ `tokens.account_id`
  - home 目录解析复用 `window_commands` 的 `USERPROFILE` → `HOME` 回退逻辑，不依赖 `dirs` crate
- `registry.rs` 的 openai / anthropic 模板填充 `oauth_detect` 字段（`file_path` / `token_path` / `keychain_service`），供前端展示检测来源
- Claude 订阅补 `seven_day_opus` 窗口：`AnthropicOAuthUsageResponse` 新增 `seven_day_opus` 字段，命中时按 `window_labels::SEVEN_DAY_OPUS` 常量构造窗口
- 所有 Anthropic 订阅窗口 label 改用 `window_labels` 常量（`FIVE_HOUR` / `SEVEN_DAY` / `SEVEN_DAY_SONNET` / `SEVEN_DAY_OPUS`），不再硬编码中文
- ChatGPT Wham 查询补 `ChatGPT-Account-Id` header：
  - `fetch_openai_wham` 签名增加 `account_id: Option<&str>`
  - 调用方未传时，由 `oauth_detect::detect_openai()` 自动读取 `tokens.account_id`（fetch 时回退，不需要用户手动配置）
  - `User-Agent` 改为 `codex-cli`（与 cc-switch 一致）
- `SubscriptionFetcher::fetch` / `ProviderManager::fetch_subscription_usage` 签名透传 `account_id`；新增 `fetch_subscription_usage_with_account` 供未来显式传入
- `DetectedToken` 结构新增 `accountId` 字段（`#[serde(default)]`，camelCase），前端 `ipc.ts::DetectedToken` 同步新增可选 `accountId`

### 25. UI 框架已迁移 Tailwind 4 + shadcn/ui（双轨过渡中）

文件：

- `src/index.css`
- `src/components/ui/`
- `src/i18n/windowLabels.ts`
- `src/assets/styles/`（settings.css / widget.css / common.css / main.css，待消化）

当前要求：

- 设计 token 统一在 `index.css` 的 Tailwind 4 `@theme`，包括 `--color-error`、`--color-text-tertiary`（曾缺失导致 38+ 处工具类静默失效，已补）
- 暗色值只维护 `:root` 里的 `--color-dark-*` 源值块；`[data-theme="dark"]` 与 `prefers-color-scheme` 回退块只做 `var()` 映射，新增暗色 token 先登记源值再映射
- `src/components/ui/` 里 dialog / button / input / switch 在用，其余 shadcn 组件（select/tabs/slider/scroll-area/tooltip/badge/card/separator）暂为死代码，清理前不要新增对它们的引用
- 旧 CSS 四个文件仍在承担大量样式：新代码一律用 Tailwind，改到哪个组件就顺手迁移对应样式，最终目标是删除旧 CSS
- 透明度滑杆统一用 `index.css` 末尾的 `.opacity-slider` 类（webkit/moz 双伪元素，含可见滑块），不要再写裸 `appearance-none` range
- 订阅窗口 label 的 i18n 映射统一走 `src/i18n/windowLabels.ts` 的 `getWindowLabel(label, language)`，ProviderCard / SubscriptionBadge / UsageStatsPanel / IslandWidget 共用，禁止在组件里重复实现
- `main.tsx` 按窗口 label 动态导入样式：主窗口加载 `main.css`，灵动岛只加载 `index.css` + `island.css`

### 26. 灵动岛（island 窗口）已可用

文件：

- `src-tauri/tauri.conf.json`
- `src-tauri/capabilities/island.json`
- `src/components/island/IslandWidget.tsx`
- `src/assets/styles/island.css`
- `src/stores/settingsStore.ts`
- `src-tauri/src/tray/mod.rs`
- `src-tauri/src/commands/settings_commands.rs`
- `src-tauri/src/lib.rs`
- `src/App.tsx`
- `src/i18n/messages.ts`

当前要求：

- 灵动岛是第二个 Tauri 窗口（label `island`，200×40、alwaysOnTop、skipTaskbar）；权限走独立的 `capabilities/island.json`，**island 窗口调用任何新 Tauri API 前必须确认该 capability 已覆盖**，否则 ACL 静默拒绝
- 两个窗口都已配置 `shadow: false`：**transparent + 无边框窗口必须禁用 DWM 阴影**，否则 Windows 会在窗口矩形边缘画一圈白色边框（岛条这类小窗口上尤其明显），新增窗口时保持该配置
- 拖动只走 `mousedown → getCurrentWindow().startDragging()`，禁止再混用 `data-tauri-drag-region` / `WebkitAppRegion` / 手写 mousemove（历史三套叠加 + 物理/逻辑像素混用已修掉）
- **startDragging 必须在 mousemove 超过阈值后再调用**，不能在 mousedown 里立即调：Windows 上 startDragging 进入 OS 模态移动循环，纯点击时可能吞掉 mouseup 导致 click 不合成（表现为「岛有时点不开」的时序竞争）；当前阈值 5px
- 展开时先 `setSize` 到 300×400（过渡初始值），渲染后**窗口高度跟随面板内容**（下限 120、上限 420，超出后面板内列表滚动）；不要回退成固定 400 高——供应商少时面板底部会留大片空白；面板尺寸与窗口尺寸必须同步改，只改一边会被裁切
- **高度同步不能用 ResizeObserver 观察面板自身**：面板是 `max-h-full`，内容超出当前窗口高度后就被夹住、高度不再变化，回调永不触发，形成「窗口等面板变高、面板等窗口变高」的死锁（快捷设置曾被整个截断）。当前实现是显式跟踪内容状态（`summaries / expandedProvider / showQuickSettings / settingsLoaded / language`），用「固定头部 40 + 列表 `scrollHeight` + 快捷设置 `offsetHeight` + 边框 2」直接计算期望高度；列表是 `overflow-y-auto`，其 `scrollHeight` 恒等于内容全高，不受 flex 夹取影响
- 展开面板容器是 `max-h-full` + 内容自然撑开（不是 `h-full`），列表区不加 `flex-1`；`#app` 内容贴窗口顶部对齐，展开时面板从岛条位置向下生长
- **岛内一切颜色必须走主题变量**（`bg-ghost` / `bg-surface-elevated` / `border-border` / `text-*`）：`bg-white/N`、`border-white/N` 这类深色残留类在浅色主题下完全隐形（分段控件曾因此看起来像纯文本），新增岛内 UI 不要用 `white/N` 色阶
- 展开后要做**工作区越界校正**（岛贴屏幕右缘时展开面板会把收起按钮画到屏外）；若发生校正平移，**收起时必须恢复校正前位置**（`expandOriginRef`），否则「展开-收起」循环会把岛条逐步推离用户拖放的位置
- 展开面板的关闭入口有三：**点击岛外失焦自动收起**（展开后 400ms 内的 blur 视为 setSize/setFocus 竞争抖动忽略）、**Esc**、**头部收起按钮**；展开时要 `setFocus()` 让岛真正持有焦点（快捷设置输入和失焦收起的完整焦点转换都依赖它），capability 需含 `core:window:allow-set-focus`
- 位置持久化在 localStorage（逻辑像素），恢复时基于 monitor workArea 做离屏校验，不足一半可见则回退工作区顶部居中
- 显隐开关：设置页「通用」最后一项 + 托盘勾选菜单项；持久化字段 `islandVisible`，默认 `true`，旧配置缺省兼容为 true；启动时 Rust 侧按配置恢复显隐，避免「先闪一下再隐藏」
- 岛内必须先 `loadSettings()` 完成（`loaded` 门）再渲染快捷设置，否则 `saveSettings` 会基于 `DEFAULT_SETTINGS` 把用户配置整体洗成默认值
- 设置跨窗口同步：`saveSettings` IPC 成功后 `emit("settings-changed", { source, settings })`；对端 `applySyncedSettings` 只更新内存、不落盘、不再广播，防回环
- 快捷设置保存与主窗口同约定：滑杆拖动中只预览不保存，松手才落盘；数字输入防抖
- 托盘菜单文本已 i18n：`tray/mod.rs` 的 `TrayTexts` 按 `config.language` 三选一；`save_settings` 检测到 `language` 或 `island_visible` 变化会重建托盘菜单
- 岛内文案全部走 `messages.ts` 的 `island.*`，禁止硬编码

### 27. 后端稳健性修复批次（2026-07-19）

文件：

- `src-tauri/src/config/atomic.rs`（新增）
- `src-tauri/src/config/app_config.rs`、`encryption.rs`、`system_env.rs`
- `src-tauri/src/stats/mod.rs`
- `src-tauri/src/commands/provider_commands.rs`、`update_commands.rs`、`window_commands.rs`
- `src-tauri/src/providers/`（mod.rs、subscription.rs、openai.rs、anthropic.rs、openrouter.rs、gemini.rs）
- `src/composables/usePolling.ts`

当前要求：

- `config.json` / `keys.dat` / `usage_stats.json` 全部走 `atomic.rs` 原子写入（tmp + rename）；解析失败备份为 `.bak` 再回退默认，新增文件写入必须复用该 helper
- `fetch_all_usage` 已并发化（futures `buffered(4)` 保序，供应商间 + 多 key/多订阅两层）；单供应商失败用 `build_error_summary` 塞进该卡片 `error_message`，不得再 `?` 传播拖垮全部
- 所有 reqwest client 必须带 `timeout(30s)` + `connect_timeout(10s)`；订阅查询与 legacy providers 已全部补齐
- Anthropic 已移除 rate limit 计费探测（`fetch_rate_limits` 恒 `None`），badge 不再显示；**不要加回 `POST /v1/messages` 探测**——每次刷新都是真实计费调用
- OpenAI costs：`amount.value` 官方口径是美元小数（openai-openapi 规范），**禁止再 `/100`**；已支持分页（`limit=180`，页数封顶 20）
- Windows 写用户环境变量后会广播 `WM_SETTINGCHANGE`；已运行进程仍需重启才能感知属平台限制
- 轮询定时器按「策略指纹」（供应商 id + mode/interval/unit 序列化）重建，providers 数据刷新不再重置定时器；新增影响调度的设置字段时必须同步加入指纹
- 更新状态契约是 camelCase：`"upToDate"`（不是 `"up-to-date"`），前后端保持一致
- 分供应商轮询覆盖对任意 provider id 生效，不再有三供应商白名单
- GitHub Release 链接统一指向 `yaoliwen542-sketch/PeekaUsage`

