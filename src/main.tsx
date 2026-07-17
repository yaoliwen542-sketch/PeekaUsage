import { createRoot } from "react-dom/client";
import { getCurrentWindow } from "@tauri-apps/api/window";
import App from "./App";
import IslandWidget from "./components/island/IslandWidget";
import { I18nProvider } from "./i18n";
import "./index.css";
import "./assets/styles/main.css";

const container = document.getElementById("app");

if (!container) {
  throw new Error("未找到应用挂载节点 #app");
}

// 根据窗口 label 决定渲染主界面还是灵动岛
const currentWindow = getCurrentWindow();
const windowLabel = currentWindow.label;

createRoot(container).render(
  <I18nProvider>
    {windowLabel === "island" ? <IslandWidget /> : <App />}
  </I18nProvider>,
);
