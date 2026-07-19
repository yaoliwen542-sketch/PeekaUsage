import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import IslandWidget from "./components/island/IslandWidget";
import { I18nProvider } from "./i18n";
import "./index.css";

const container = document.getElementById("app");

if (!container) {
  throw new Error("未找到应用挂载节点 #app");
}

// 另存一份收窄后的引用，供异步 bootstrap 闭包使用
const appContainer: HTMLElement = container;

// 根据窗口 label 决定渲染主界面还是灵动岛
const currentWindow = getCurrentWindow();
const windowLabel = currentWindow.label;

async function bootstrap() {
  // 灵动岛窗口只加载自身需要的基础样式，
  // 避免全量注入 main.css 连带 settings/widget 的近千行无用规则
  if (windowLabel === "island") {
    await import("./assets/styles/island.css");
  } else {
    await import("./assets/styles/main.css");
  }

  createRoot(appContainer).render(
    <I18nProvider>
      {windowLabel === "island" ? <IslandWidget /> : <App />}
    </I18nProvider>,
  );
}

void bootstrap();
