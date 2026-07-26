import { createRoot } from "react-dom/client";
import { createElement, type ReactNode } from "react";
import { mockIPC, mockWindows } from "@tauri-apps/api/mocks";
import { emit } from "@tauri-apps/api/event";
import { MOCK_STATS_SNAPSHOT, MOCK_SUMMARIES, buildMockSettings } from "./mockData";
import type { AppSettings } from "../types/settings";
import type { WindowDockEdge } from "../utils/windowBounds";
import "../index.css";

/**
 * UI 预览入口（仅供本地截图审查，不进入正式包）：
 * - 用 @tauri-apps/api/mocks 接管全部 IPC 与事件
 * - 通过 ?scene= 渲染指定界面：widget / widget-compact / stats / island / island-expanded / dock-*
 * - 通过 ?theme=light|dark 控制主题
 */

const params = new URLSearchParams(window.location.search);
const scene = params.get("scene") ?? "widget";
const themeParam = params.get("theme") === "light" ? "light" : "dark";

const settingsOverrides: Partial<AppSettings> = {
  theme: themeParam,
};
if (scene === "widget-compact") {
  settingsOverrides.widgetDisplayMode = "compact";
}
const mockSettings = buildMockSettings(settingsOverrides);

mockWindows("main");

const MONITOR_MOCK = {
  name: "Preview Monitor",
  scaleFactor: 1,
  position: { x: 0, y: 0 },
  size: { width: 1920, height: 1080 },
  workArea: { position: { x: 0, y: 0 }, size: { width: 1920, height: 1040 } },
};

mockIPC((cmd) => {
  switch (cmd) {
    case "get_settings":
      return mockSettings;
    case "save_settings":
      return null;
    case "fetch_all_usage":
      return MOCK_SUMMARIES;
    case "fetch_provider_usage":
      return MOCK_SUMMARIES[0];
    case "get_usage_stats_snapshot":
      return MOCK_STATS_SNAPSHOT;
    case "get_current_version":
      return "0.2.8";
    case "get_provider_configs":
      return [];
    case "get_provider_templates":
      return [];
    case "get_webdav_sync_password":
      return "";
    case "plugin:window|scale_factor":
      return 1;
    case "plugin:window|inner_size":
      return { width: 400, height: 640 };
    case "plugin:window|outer_size":
      return { width: 400, height: 640 };
    case "plugin:window|inner_position":
    case "plugin:window|outer_position":
      return { x: 100, y: 100 };
    case "plugin:window|current_monitor":
    case "plugin:window|primary_monitor":
      return MONITOR_MOCK;
    case "plugin:window|get_all_windows":
      return [{ label: "main" }];
    default:
      // set_size / set_position / set_always_on_top / hide / minimize / show /
      // set_focus / start_dragging / set_skip_taskbar 等写操作全部静默成功
      return null;
  }
}, { shouldMockEvents: true });

function render(node: ReactNode) {
  const container = document.getElementById("app");
  if (!container) {
    throw new Error("未找到应用挂载节点 #app");
  }
  createRoot(container).render(node);
}

async function main() {
  const { I18nProvider } = await import("../i18n");
  const { applyTheme } = await import("../utils/theme");
  // 独立场景（stats/island/dock）不经过 App.tsx，主题需要在预览入口统一应用
  applyTheme(themeParam);

  if (scene === "island" || scene === "island-expanded") {
    await import("../assets/styles/island.css");
    const { default: IslandWidget } = await import("../components/island/IslandWidget");
    render(createElement(I18nProvider, null, createElement(IslandWidget)));
    // 等岛组件完成监听注册后推送用量数据
    window.setTimeout(() => {
      void emit("island-usage-update", MOCK_SUMMARIES);
    }, 500);
    // 展开场景：模拟点击胶囊
    if (scene === "island-expanded") {
      window.setTimeout(() => {
        const pill = document.querySelector<HTMLElement>("#app > div");
        pill?.click();
      }, 1100);
    }
    return;
  }

  if (scene.startsWith("dock-")) {
    await import("../assets/styles/main.css");
    const { default: EdgeDockHandle } = await import("../components/common/EdgeDockHandle");
    const edge = (scene.replace("dock-", "") || "left") as WindowDockEdge;
    document.getElementById("app")?.classList.add("app-edge-docked-collapsed");
    render(
      createElement(
        "div",
        { className: `app-shell is-edge-docked is-edge-docked-collapsed edge-${edge}` },
        createElement(EdgeDockHandle, { edge }),
      ),
    );
    return;
  }

  if (scene === "stats") {
    await import("../assets/styles/main.css");
    const { default: UsageStatsPanel } = await import("../components/widget/UsageStatsPanel");
    render(
      createElement(
        I18nProvider,
        null,
        createElement(
          "div",
          {
            className: "app-shell",
            style: { display: "flex", height: "100%", padding: 8, boxSizing: "border-box" },
          },
          createElement(UsageStatsPanel, {
            open: true,
            providers: MOCK_SUMMARIES,
            onClose: () => undefined,
          }),
        ),
      ),
    );
    return;
  }

  // 默认：完整主悬浮窗
  await import("../assets/styles/main.css");
  const { default: App } = await import("../App");
  render(createElement(I18nProvider, null, createElement(App)));
}

void main();

// 截图保真：持续 rAF 让合成器不断产帧，保证最终帧反映最终窗口尺寸与最新 DOM
if (params.get("spin") === "1") {
  const tick = () => requestAnimationFrame(tick);
  requestAnimationFrame(tick);
}

// 截图用：?fixwidth=400 把布局约束到真实窗口宽（Edge 无头最小窗宽 ~500，无法直接截 400 视口）
const fixwidthPx = parseInt(params.get("fixwidth") ?? "", 10);
if (fixwidthPx > 0) {
  document.documentElement.style.width = `${fixwidthPx}px`;
  document.body.style.width = `${fixwidthPx}px`;
}

// 截图用：?noanim=1 禁用全部 CSS 动画/过渡（虚拟时间下合成器帧与 DOM 终态可能不一致）
if (params.get("noanim") === "1") {
  const st = document.createElement("style");
  st.textContent = "*, *::before, *::after { animation: none !important; transition: none !important; }";
  document.head.appendChild(st);
}
