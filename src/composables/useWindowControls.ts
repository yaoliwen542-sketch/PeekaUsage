import { create } from "zustand";
import { getCurrentWindow } from "@tauri-apps/api/window";
import { useSettingsStore } from "../stores/settingsStore";

type WindowControlsState = {
  opacity: number;
  isDraggingOpacity: boolean;
  setOpacityState: (value: number) => void;
  setDraggingOpacity: (value: boolean) => void;
};

const useWindowControlsStore = create<WindowControlsState>((set) => ({
  opacity: 100,
  isDraggingOpacity: false,
  setOpacityState: (value) => set({ opacity: value }),
  setDraggingOpacity: (value) => set({ isDraggingOpacity: value }),
}));

function clampOpacity(value: number) {
  return Math.max(10, Math.min(100, Math.round(value)));
}

async function applyOpacity(value: number) {
  const clamped = clampOpacity(value);
  useWindowControlsStore.getState().setOpacityState(clamped);

  // 透明度纯前端实现：直接改 #app 的 CSS opacity，
  // 不再调用后端 set_window_opacity（该命令是空实现，拖滑杆时每帧白发 IPC）
  const appEl = document.getElementById("app");
  if (appEl) {
    appEl.style.opacity = `${clamped / 100}`;
  }

  return clamped;
}

export function useWindowControls() {
  const opacity = useWindowControlsStore((state) => state.opacity);
  const isDraggingOpacity = useWindowControlsStore((state) => state.isDraggingOpacity);

  async function hideWindow() {
    await getCurrentWindow().hide();
  }

  async function minimizeWindow() {
    await getCurrentWindow().minimize();
  }

  async function closeToTray() {
    await getCurrentWindow().hide();
  }

  async function updateOpacity(value: number, persist = false) {
    const clamped = await applyOpacity(value);

    if (persist && useSettingsStore.getState().settings.windowOpacity !== clamped) {
      await useSettingsStore.getState().saveSettings({ windowOpacity: clamped });
    }

    return clamped;
  }

  function startOpacityDrag(startY: number) {
    useWindowControlsStore.getState().setDraggingOpacity(true);
    const startOpacity = useWindowControlsStore.getState().opacity;
    let lastOpacity = startOpacity;
    let finished = false;

    function onMouseMove(event: MouseEvent) {
      const deltaY = startY - event.clientY;
      const deltaOpacity = deltaY * 0.5;
      lastOpacity = clampOpacity(startOpacity - deltaOpacity);
      void updateOpacity(lastOpacity, false);
    }

    // 拖拽有多个结束路径：窗口内松手（document mouseup）、指针松手/被取消
    // （pointerup / pointercancel）、光标移出窗口后松手导致窗口失焦（blur）。
    // 只监听 document mouseup 时，移出窗口松开会漏掉结束事件，
    // 导致 isDraggingOpacity 卡死且最终值不持久化；这里统一收口，
    // 任何结束路径都复位拖拽态并持久化最终值。
    function finishDrag() {
      if (finished) {
        return;
      }
      finished = true;

      useWindowControlsStore.getState().setDraggingOpacity(false);
      document.removeEventListener("mousemove", onMouseMove);
      document.removeEventListener("mouseup", finishDrag);
      window.removeEventListener("pointerup", finishDrag);
      window.removeEventListener("pointercancel", finishDrag);
      window.removeEventListener("blur", finishDrag);
      void updateOpacity(lastOpacity, true);
    }

    document.addEventListener("mousemove", onMouseMove);
    document.addEventListener("mouseup", finishDrag);
    window.addEventListener("pointerup", finishDrag);
    window.addEventListener("pointercancel", finishDrag);
    window.addEventListener("blur", finishDrag);
  }

  return {
    opacity,
    isDraggingOpacity,
    hideWindow,
    minimizeWindow,
    closeToTray,
    updateOpacity,
    startOpacityDrag,
    applyOpacity,
  };
}
