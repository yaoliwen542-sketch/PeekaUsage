# PeekaUsage 无框玻璃壳 + 行式布局 + 全局代理 设计文档

日期：2026-11-07
状态：已确认方向，待实施
目标版本：v0.5.0

## 0. 背景与目标

用户提出四点诉求：

1. 程序性能需要优化
2. 需要代理功能
3. UI 太丑，可以全盘重构
4. 整体不好用

经三轮示意图对齐，已确认的产品决策：

| 决策点 | 结论 |
|---|---|
| 视觉语言 | 方向 A「精致工具感」：去嵌套灰框、细分隔线、4px 细进度条、tabular-nums 大数字、层次靠留白 |
| 信息形态 | 行式折叠：每个供应商默认一行，点行就地展开窗口/Key 明细 |
| 壳形态 | 无框玻璃面板为主形态 + 隐藏后托盘图标呼出（弹出态失焦自动收起） |
| 实施顺序 | 代理 → 玻璃壳+行式 → 易用性 → 性能测量与优化 |

成功标准：

- 无代理环境时行为与现状完全一致（默认跟随系统代理）
- 自定义代理配置后无需重启即生效；测试连接能给出具体失败原因
- 主面板无标题栏、玻璃质感；供应商多时默认高度基本恒定（每家一行）
- 托盘呼出 → 失焦收起 → 还原常驻，整个循环无状态错乱；边缘吸附回归无损
- 旧配置文件零迁移成本升级（缺新字段按默认值兼容）

## 1. 范围与非目标

**做**：全局代理（跟随系统/不使用/自定义 URL + 测试连接）、无框玻璃壳、行式折叠布局、托盘呼出模式、首次启动引导、错误可操作化、性能测量点与针对性优化、旧 CSS 清场。

**不做（明确排除）**：

- PAC 脚本 / 每供应商独立代理（YAGNI，全局代理覆盖绝大多数场景）
- 应用内更新器（tauri-plugin-updater）的代理支持——它有独立 client，本期不动，文档注明
- 灵动岛（顶部胶囊）回归——该功能曾实现后移除（AGENTS.md #26），本期不恢复第二个窗口架构
- 自动「风险置顶」排序——provider_order 手动排序继续是唯一顺序来源，风险置顶列为后续可选项

## 2. 阶段一：全局代理

### 2.1 配置字段（AppSettings）

| 字段 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `proxy_mode` | `none \| system \| custom` | `system` | 与 reqwest 现状一致，旧配置缺字段零行为变化 |
| `proxy_url` | String | `""` | custom 时必填；支持 `http://` `https://` `socks5://` |
| `proxy_username` | String | `""` | 可选 Basic 鉴权用户名 |
| 代理密码 | KeyStore 键 `proxy_password` | — | 敏感信息不进 config.json，与 API Key 同等对待 |

TS 侧 `src/types/settings.ts` 同步新增 `proxyMode` / `proxyUrl` / `proxyUsername`（camelCase，serde 映射）。

### 2.2 统一 HTTP client 工厂

新增 `src-tauri/src/net/http_client.rs`：

```rust
pub struct HttpClientFactory { client: RwLock<Arc<reqwest::Client>> }
impl HttpClientFactory {
    pub fn reload(&self, proxy: &ProxySettings);   // 按 proxy_mode 构建：none→no_proxy()，system→默认，custom→Proxy::all(url)+BasicAuth
    pub fn client(&self) -> Arc<reqwest::Client>;  // 快照获取，请求期间不阻塞 reload
}
```

- 工厂统一施加 `timeout(30s)` + `connect_timeout(10s)`，删除各构建点的重复配置
- 改造点（9 处，全部改为从工厂取 client，provider 结构不再自持 `reqwest::Client`）：
  - `providers/mod.rs`（ProviderManager::http_client）
  - `providers/openai.rs`、`anthropic.rs`、`openrouter.rs`、`subscription.rs`、`coding_plan.rs`、`balance.rs`、`script_engine.rs`、`gemini.rs`
  - `commands/data_commands.rs`（GitHub Release 拉取）
- 构造链：`lib.rs` 启动时按配置创建并挂入 Tauri State；`save_settings` 命令保存代理字段后调用 `reload()`，热生效
- client 重建会丢弃旧连接池，仅发生在用户改代理时，可接受

### 2.3 设置 UI 与测试连接

- 设置页「通用」区块末尾新增「网络代理」分区：模式分段按钮（不使用 / 跟随系统 / 自定义）+ URL 输入框（custom 时显示）+ 用户名/密码输入（可选）+「测试连接」按钮
- 新命令 `test_proxy_connection`：按表单当前值临时构建 client（不影响已保存配置），依次尝试 `https://www.gstatic.com/generate_204` → `https://www.baidu.com`（8s 超时），返回 `{ ok, endpoint, error }`；错误信息区分代理拒绝 / 认证失败 / DNS / 超时
- 环境变量切换（`system_env.rs`）与本功能无关，不交叉

## 3. 阶段二：无框玻璃壳 + 行式布局

### 3.1 壳改造（GlassGrip）

现状核对：窗口已是 `decorations:false` + `transparent:true` + `alwaysOnTop:true`，标题栏是自绘 HTML（`TitleBar.tsx`，已带 `data-tauri-drag-region` 和 `onDragIntentStart` 边缘吸附钩子）。因此：

- `TitleBar.tsx` 重构为 `GlassGrip.tsx`（高 36px）：左侧握把 pill + 「N 项接近上限」风险 chip（amber 描边，计算规则：任一窗口利用率 ≥85% 的供应商数）；右侧主题切换、隐藏到托盘（eye-off）两个图标按钮
- `data-tauri-drag-region` 与 `onDragIntentStart` 原样保留——边缘吸附、设置页扩窗、窗口持久化等既有链路（AGENTS.md 21/11.1 条全部约束）不受影响
- 玻璃材质：`tauri.conf.json` 窗口配置加 `windowEffects`（`acrylic` + `mica`，Windows 11 原生合成）；Win10 / Linux / macOS 由 Tauri 自动回退，CSS 层用 rgba 底色兜底（与现有 `windowOpacity` 机制同源，滑杆语义不变）
- 设置页「通用」新增「原生玻璃材质」开关（`nativeGlassEnabled`，默认开），关闭后降级为纯 rgba——同时充当性能逃生门

### 3.2 行式布局（ProviderRow）

- 新组件 `src/components/widget/ProviderRow.tsx`：
  - 折叠行（约 44px）：图标 + 名称 + 摘要行（次要窗口 pct 摘要，如 `7d 61 · Sonnet 88`）+ 主窗口 pct + 56px 迷你条 + 悬停刷新按钮；主窗口取「利用率最高窗口」（与现有 hero 口径一致）
  - 展开区：复用 `SubscriptionBadge` / `UsageStatsPanel` 渲染全部订阅窗口（含重置时间）、Extra Usage、逐 Key 金额/余额/RPM/TPM 明细——信息一个不删，只是收进展开态
  - 展开状态沿用 `provider_card_expanded` 持久化字段
- 按量供应商（无订阅窗口）：折叠行主数字显示今日消耗金额 + 「今日」小标，展开区显示余额等明细
- `widgetDisplayMode`（detailed/compact）废弃：不再参与渲染分支；配置字段保留（读忽略），底部显示模式切换按钮移除；`ProviderCard.tsx` 阶段二冻结不再渲染，阶段三删除
- 风险 chip、空态、加载态、错误态都在行级呈现（见阶段三）

### 3.3 托盘呼出模式

- 「隐藏到托盘」按钮 → `window.hide()`（与现有关闭到托盘行为一致）
- 托盘左键单击（`MouseButtonState::Up`，既有约定）：窗口隐藏时 → 从 `TrayIconEvent` 取光标位置，计算托盘邻近坐标（工作区校正、防越界），`set_position` + `show`，进入**呼出态**；窗口可见时 → 保持既有行为（前置/聚焦窗口）
- 呼出态（前端 `data-shell="popover"`）：
  - GlassGrip 右侧出现「还原」按钮（picture-in-picture 图标）→ 后端恢复持久化的 `windowPosition`/`windowSize`，回到常驻态
  - `WindowEvent::Focused(false)` → 自动 `hide()`（模拟失焦收起）
  - 呼出态**跳过**边缘吸附判定与 autoFit 高度调整（避免与弹出定位打架）；仅「还原」写回持久化边界
- 新增后端命令：`summon_near_tray` / `restore_main_bounds`（放 `window_commands.rs`）；持久化只允许在还原时发生，弹出坐标不写配置（与 AGENTS.md「细条态不写回」同一原则）

## 4. 阶段三：易用性

1. **首次启动引导**：无启用供应商时行式列表区域渲染引导卡（应用 logo + 一句话 + 「添加第一个供应商」按钮直达设置供应商子页）；`hasLoaded` 后判断，无需新持久化字段
2. **错误可操作化**：`providers/types.rs` 的 `UsageSummary` 新增 `error_kind: Option<ErrorKind>`（`network | auth | parse | rate_limited`，`build_error_summary` 处产出），`src/types/provider.ts` 同步；行展开区错误块按 kind 渲染——`network`/`rate_limited` → 「重试」按钮（调 `refreshProvider`），`auth` → 「打开设置」按钮；完整错误文本悬浮展示
3. **旧 CSS 清场收尾**：`main.css` / `common.css` / `widget.css` / `settings.css` 全部迁 Tailwind 后删除；`ProviderCard.tsx` 删除
4. 全部新增文案入 `src/i18n/messages.ts`（简中/繁中/英三份），设置页/玻璃面板风格统一

## 5. 阶段四：性能测量与优化

原则：先测量后优化，无数据不优化。

- **测量点**：
  - `lib.rs` 启动分段耗时（插件注册/配置加载/托盘创建/首屏数据），`debug` log + `PEEKA_PERF_LOG=1` 开关
  - 前端 `performance.mark`（main.tsx → 首帧）经 IPC 汇报 debug log
  - `fetch_all_usage` 每供应商耗时（`fetch_ms`）写入 `usage_stats.json`
- **已知嫌疑按诊断结果处理**：
  - 行组件 `React.memo` + zustand selector 细粒度化（单供应商刷新不重渲整列表）
  - autoFit / 拖拽抑制逻辑回归确认（AGENTS.md 21 条不回退）
  - 轮询间隔下限：< 5s 保存时弹确认提示（秒级轮询 = 持续网络+渲染压力）
  - `nativeGlassEnabled=false` 作为透明合成开销逃生门
  - 工具链：`WEBVIEW2_ADDITIONAL_BROWSER_ARGUMENTS=--remote-debugging-port=9222` CDP 流程写入 CLAUDE.md

## 6. 配置与兼容汇总

| 字段 | 动作 | 兼容规则 |
|---|---|---|
| `proxy_mode`/`proxy_url`/`proxy_username` | 新增 | 缺省 = system，行为与旧版一致 |
| `proxy_password` | 新增（KeyStore） | — |
| `nativeGlassEnabled` | 新增 | 缺省 = true |
| `widgetDisplayMode` | 废弃（读忽略） | 字段保留不删，旧配置不迁移不报错 |
| `provider_card_expanded` | 沿用 | 行式展开状态直接复用 |
| `error_kind`（API 返回） | 新增 | 前端对缺省按 network 处理 |

## 7. 跨平台约束

- **Windows 11**：acrylic/mica 原生材质；**Windows 10**：windowEffects 回退 + rgba 兜底；**macOS**：`windowEffects` 映射 `AppearanceBased` 材质（跟随系统外观）；**Linux**：无原生材质，纯 rgba
- 托盘邻近定位：以 `TrayIconEvent` 光标位置为基准 + 工作区边界校正（`windowBounds.ts` 既有离屏哨兵过滤复用）
- 代理：Linux 无桌面代理设置时 `system` 模式可能为空（reqwest 读 env），文档说明
- 三平台行为差异逐条写入 AGENTS.md 对应章节

## 8. 风险与缓解

| 风险 | 缓解 |
|---|---|
| 透明窗口 + acrylic 合成开销（AGENTS.md 已有透明窗口性能教训） | `nativeGlassEnabled` 开关 + 阶段四诊断决定默认值；CSS 端避免 backdrop-filter，只用原生材质 |
| 玻璃壳重构回归边缘吸附/设置页扩窗/窗口持久化 | GlassGrip 只换皮保留拖拽协议；手动验证清单逐项回归（AGENTS.md「每次提交前验证」） |
| 9 处 client 改造引入查询回归 | 工厂单测（proxy 组合矩阵）+ 每供应商手动刷新验证 |
| 删除 ProviderCard/旧 CSS 的回归面大 | 阶段二冻结不渲染、阶段三才删；删除前截图对比（preview-shots 设施） |
| 呼出态与吸附/autoFit 冲突 | 呼出态明确互斥跳过（与细条态同一处理原则） |

## 9. 验证计划

- 常规：`npx tsc --noEmit` + `cargo fmt --all --check` + `cargo check`（每次提交）
- 新增单测：`http_client.rs` 代理组合矩阵（none/system/custom+auth）；`build_error_summary` 的 `error_kind` 映射
- 手动清单（关键路径）：
  1. 三种代理模式下各供应商查询正常；改配置不重启生效；错误代理时测试连接给出具体原因
  2. 玻璃面板：Win11 acrylic 生效、开关降级正常、明暗主题、透明度滑杆
  3. 行式折叠：展开/收起持久化、展开区信息与旧详细模式等价、按量供应商展示、拖拽排序保存
  4. 托盘：隐藏→呼出→失焦收起→还原常驻全循环；还原后重启位置正确；细条吸附不受影响
  5. 空态引导卡、错误重试/跳设置按钮、三语言切换
  6. 发版：版本三处同步、`latest.json` 9 平台校验（照 AGENTS.md 发版约束）

## 10. 文档同步义务

实施期间同步更新（AGENTS.md 修改流程硬性要求）：

- AGENTS.md：新增「玻璃壳与行式布局」「代理」章节；更新 #19/#23 显示模式废弃说明、#26 灵动岛条目（注明托盘呼出复用单窗口、灵动岛不回归）；托盘约束补充呼出态规则
- CLAUDE.md：镜像同步 + CDP 性能诊断流程
- `.github/release-notes/v0.5.0.md`：发版时同步
