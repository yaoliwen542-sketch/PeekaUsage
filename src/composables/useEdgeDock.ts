import { useCallback, useEffect, useRef, useState } from "react";
import {
  LogicalPosition,
  LogicalSize,
  currentMonitor,
  getCurrentWindow,
} from "@tauri-apps/api/window";
import { useSettingsStore } from "../stores/settingsStore";
import {
  areWindowPositionsEqual,
  areWindowSizesEqual,
  isProgrammaticWindowMove,
  isProgrammaticWindowResize,
  markProgrammaticWindowMove,
  markProgrammaticWindowResize,
  normalizeWindowPosition,
  normalizeWindowSize,
  resolveWindowDockBounds,
  suppressAutoFitAfterManualResize,
  toLogicalWindowPosition,
  toLogicalWindowSize,
  wasLikelyResizedBySystemSnap,
  type LogicalWindowPosition,
  type LogicalWindowSize,
  type WindowDockBounds,
  type WindowDockEdge,
} from "../utils/windowBounds";

type WindowDockPhase = "collapsed" | "preview";

/** 收起时序（消除几何突变与把手渲染的冲突）：
 *
 * 旧方案（v0.2.8）：t=0 切 collapsed 视觉态 + 挂载把手 + 内容淡出，t=190 才切窗口几何。
 * 问题：把手在【展开态窗口】里渲染（inset:0 撑满 400x600），t=190 几何突变时把手
 * 因 flex 居中而位置跳变 + scale 动画基准突变 = 多次闪动。
 *
 * 新方案：把手推迟到几何落定后才挂载。t=0 只切视觉态让内容淡出（150ms），窗口仍是展开态；
 * t=SNAP_DELAY(170ms) 切窗口几何 + 标记 snapped=true 让把手在【已是细条态】的窗口上挂载。
 * 把手从一开始就在 16x136 窗口里渲染，定位基准稳定，全程不跳。
 * 代价：收起时内容淡出后到把手出现之间有 ~20ms 的短暂空白期（人眼对"消失后
 * 短暂空白再出现"的容忍度远高于"闪动"）。 */
const EDGE_DOCK_COLLAPSE_SNAP_DELAY_MS = 170;

export type WindowDockVisualState = {
  edge: WindowDockEdge;
  phase: WindowDockPhase;
  /** collapsed 态下窗口几何是否已落定成细条。
   * false：内容淡出中，窗口仍是展开态，把手不应渲染。
   * true：几何已切到细条，把手可挂载。 */
  snapped: boolean;
};

type WindowDockState = {
  edge: WindowDockEdge;
  phase: WindowDockPhase;
  expandedBounds: {
    windowPosition: LogicalWindowPosition;
    windowSize: LogicalWindowSize;
  };
  collapsedBounds: {
    windowPosition: LogicalWindowPosition;
    windowSize: LogicalWindowSize;
  };
};

/** scaleFactor 缓存及其所属显示器的物理边界（物理像素坐标系） */
type ScaleFactorCache = {
  scaleFactor: number;
  monitorX: number;
  monitorY: number;
  monitorWidth: number;
  monitorHeight: number;
};

/**
 * 窗口生命周期 composable，从 App.tsx 抽出：
 * - 启动时恢复已保存的窗口大小/位置，并监听移动/缩放持久化窗口边界
 * - 标题栏拖拽越界后的边缘吸附收起状态机（collapsed / preview / expand）
 * - scaleFactor 按显示器缓存（M13）：同一显示器内移动/缩放复用缓存，
 *   避免拖拽时每帧 IPC；物理坐标越出缓存显示器边界（拖到不同 DPI 显示器）
 *   时按当前显示器重新解析，避免物理/逻辑换算全部错位
 */
export function useEdgeDock() {
  const [dockVisualState, setDockVisualState] = useState<WindowDockVisualState | null>(null);
  const restoringWindowBoundsRef = useRef(false);
  const windowBoundsSaveTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const pendingWindowSizeRef = useRef<LogicalWindowSize | null>(null);
  const pendingWindowPositionRef = useRef<LogicalWindowPosition | null>(null);
  // scaleFactor 缓存：记录所属显示器的物理边界，跨显示器拖拽时重取（M13）
  const scaleFactorCacheRef = useRef<ScaleFactorCache | null>(null);
  // 进行中的 scaleFactor 请求去重，避免跨屏瞬间并发多次 currentMonitor()
  const scaleFactorRequestRef = useRef<Promise<number | null> | null>(null);
  // 缓存最新窗口尺寸，供 onMoved 的边缘吸附检测使用，避免每次都 await innerSize()
  const latestWindowSizeRef = useRef<LogicalWindowSize | null>(null);
  const edgeDockCollapseTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const edgeDockEvaluateTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  // 收起淡出后的延迟几何落定定时器（展开/清除时取消，避免半路把窗口收成细条）
  const edgeDockSnapTimerRef = useRef<ReturnType<typeof setTimeout> | null>(null);
  const titlebarDragIntentUntilRef = useRef(0);
  const titlebarDraggingRef = useRef(false);
  const dragStartWindowSizeRef = useRef<LogicalWindowSize | null>(null);
  const latestDragBoundsRef = useRef<{
    windowPosition: LogicalWindowPosition;
    windowSize: LogicalWindowSize;
  } | null>(null);
  const dockStateRef = useRef<WindowDockState | null>(null);

  // ---- scaleFactor 解析（M13：按显示器缓存，越界重取）----

  async function fetchAndCacheScaleFactor(): Promise<number | null> {
    if (scaleFactorRequestRef.current) {
      return scaleFactorRequestRef.current;
    }

    const request = (async () => {
      try {
        const monitor = await currentMonitor();
        if (monitor) {
          scaleFactorCacheRef.current = {
            scaleFactor: monitor.scaleFactor,
            monitorX: monitor.position.x,
            monitorY: monitor.position.y,
            monitorWidth: monitor.size.width,
            monitorHeight: monitor.size.height,
          };
          return monitor.scaleFactor;
        }

        // 取不到显示器信息时退化为窗口自身 scaleFactor，不写边界缓存
        return await getCurrentWindow().scaleFactor();
      } catch {
        return null;
      } finally {
        scaleFactorRequestRef.current = null;
      }
    })();

    scaleFactorRequestRef.current = request;
    return request;
  }

  async function resolveScaleFactor(physicalPoint?: { x: number; y: number }): Promise<number | null> {
    const cached = scaleFactorCacheRef.current;
    if (cached) {
      if (!physicalPoint) {
        return cached.scaleFactor;
      }

      // 物理坐标仍在缓存显示器范围内：同一显示器，直接复用缓存
      const withinCachedMonitor =
        physicalPoint.x >= cached.monitorX
        && physicalPoint.x < cached.monitorX + cached.monitorWidth
        && physicalPoint.y >= cached.monitorY
        && physicalPoint.y < cached.monitorY + cached.monitorHeight;
      if (withinCachedMonitor) {
        return cached.scaleFactor;
      }
      // 越出缓存显示器边界：拖到了另一台显示器，按当前显示器重新解析
    }

    return fetchAndCacheScaleFactor();
  }

  // ---- 窗口边界持久化 ----

  function clearWindowBoundsSaveTimer() {
    if (windowBoundsSaveTimerRef.current) {
      clearTimeout(windowBoundsSaveTimerRef.current);
      windowBoundsSaveTimerRef.current = null;
    }
  }

  function scheduleWindowBoundsSave(next: {
    windowSize?: LogicalWindowSize | null;
    windowPosition?: LogicalWindowPosition | null;
  }) {
    if (next.windowSize !== undefined) {
      pendingWindowSizeRef.current = next.windowSize;
    }

    if (next.windowPosition !== undefined) {
      pendingWindowPositionRef.current = next.windowPosition;
    }

    clearWindowBoundsSaveTimer();
    windowBoundsSaveTimerRef.current = setTimeout(() => {
      const size = pendingWindowSizeRef.current;
      const position = pendingWindowPositionRef.current;
      pendingWindowSizeRef.current = null;
      pendingWindowPositionRef.current = null;
      windowBoundsSaveTimerRef.current = null;

      const currentSettings = useSettingsStore.getState().settings;
      const patch: Partial<typeof currentSettings> = {};

      if (size && !areWindowSizesEqual(currentSettings.windowSize, size)) {
        patch.windowSize = size;
      }

      if (position && !areWindowPositionsEqual(currentSettings.windowPosition, position)) {
        patch.windowPosition = position;
      }

      if (Object.keys(patch).length > 0) {
        void useSettingsStore.getState().saveSettings(patch);
      }
    }, 180);
  }

  async function restoreWindowBounds() {
    const currentWindow = getCurrentWindow();
    const currentSettings = useSettingsStore.getState().settings;
    const windowSize = normalizeWindowSize(currentSettings.windowSize);
    const windowPosition = normalizeWindowPosition(currentSettings.windowPosition);

    if (!windowSize && !windowPosition) {
      return;
    }

    restoringWindowBoundsRef.current = true;

    try {
      if (windowSize) {
        markProgrammaticWindowResize();
        await currentWindow.setSize(new LogicalSize(windowSize.width, windowSize.height));
      }

      if (windowPosition) {
        await currentWindow.setPosition(new LogicalPosition(windowPosition.x, windowPosition.y));
      }
    } catch {
      // 忽略无效的历史窗口边界，避免阻塞启动
    } finally {
      restoringWindowBoundsRef.current = false;
    }
  }

  // ---- 边缘吸附收起状态机 ----

  function clearEdgeDockCollapseTimer() {
    if (edgeDockCollapseTimerRef.current) {
      clearTimeout(edgeDockCollapseTimerRef.current);
      edgeDockCollapseTimerRef.current = null;
    }
  }

  function clearEdgeDockEvaluateTimer() {
    if (edgeDockEvaluateTimerRef.current) {
      clearTimeout(edgeDockEvaluateTimerRef.current);
      edgeDockEvaluateTimerRef.current = null;
    }
  }

  function clearEdgeDockSnapTimer() {
    if (edgeDockSnapTimerRef.current) {
      clearTimeout(edgeDockSnapTimerRef.current);
      edgeDockSnapTimerRef.current = null;
    }
  }

  /** 收起统一入口：先切 collapsed 视觉态（内容淡出、#app 背景淡出），
   * snapped=false 期间不渲染把手；SNAP_DELAY 后切窗口几何 + snapped=true 让把手
   * 在已落定的细条窗口上挂载，全程把手定位基准稳定不跳变 */
  function scheduleCollapseSnap(nextState: WindowDockState) {
    clearEdgeDockSnapTimer();
    updateDockState(nextState, false);
    persistDockExpandedBounds(nextState);
      edgeDockSnapTimerRef.current = setTimeout(() => {
        edgeDockSnapTimerRef.current = null;
        // 内容已淡出不可见，瞬时落定肉眼不可察觉
        void setProgrammaticWindowBounds(
          nextState.collapsedBounds.windowPosition,
          nextState.collapsedBounds.windowSize,
          { edge: nextState.edge, collapsing: true },
        ).then(() => {
          // 几何落定后才让把手挂载：此刻窗口已是细条态，把手定位基准稳定
          updateDockState(nextState, true);
        });
      }, EDGE_DOCK_COLLAPSE_SNAP_DELAY_MS);
  }

  function updateDockState(nextState: WindowDockState | null, snapped: boolean) {
    dockStateRef.current = nextState;
    setDockVisualState(nextState
      ? {
        edge: nextState.edge,
        phase: nextState.phase,
        snapped,
      }
      : null);
  }

  function persistDockExpandedBounds(dockState: WindowDockState) {
    scheduleWindowBoundsSave({
      windowSize: dockState.expandedBounds.windowSize,
      windowPosition: dockState.expandedBounds.windowPosition,
    });
  }

  async function setProgrammaticWindowBounds(
    windowPosition: LogicalWindowPosition,
    windowSize: LogicalWindowSize,
    options?: { edge?: WindowDockEdge; collapsing?: boolean },
  ) {
    const currentWindow = getCurrentWindow();
    markProgrammaticWindowMove();
    markProgrammaticWindowResize();
    // 右侧吸附时按方向选择 setSize/setPosition 顺序：让落定过程中窗口右缘
    // 先越过屏幕右缘（不可见）再回到屏幕右缘，避免把手在落定瞬间出现在屏幕中部。
    // 收起（expanded→collapsed）：setPosition 先把窗口左移，右缘越屏到不可见，setSize 再收缩到 16；
    // 展开（collapsed→expanded）：setSize 先把宽度撑开，右缘越屏到不可见，setPosition 再左移贴齐。
    // 左/上吸附锚定边在左上角，setPosition 通常是 no-op，顺序无影响，走默认即可。
    if (options?.edge === "right" && options.collapsing) {
      await currentWindow.setPosition(new LogicalPosition(windowPosition.x, windowPosition.y));
      await currentWindow.setSize(new LogicalSize(windowSize.width, windowSize.height));
    } else {
      await currentWindow.setSize(new LogicalSize(windowSize.width, windowSize.height));
      await currentWindow.setPosition(new LogicalPosition(windowPosition.x, windowPosition.y));
    }
  }

  async function collapseDockedWindow() {
    if (!useSettingsStore.getState().settings.edgeDockCollapseEnabled) {
      return;
    }

    const dockState = dockStateRef.current;
    if (!dockState || dockState.phase === "collapsed") {
      return;
    }

    // 复用 dockState 里已有的 collapsedBounds，不重新调 resolveWindowDockBounds。
    // 原因：展开后 expandedBounds 可能被 autoFit / onResized 改动（高度/位置微调），
    // 若每次收起都基于当前 expandedBounds 重算 collapsedBounds，细条位置会随
    // 展开态边界的微小漂移而累积漂移。collapsedBounds 在首次 activateWindowDock /
    // evaluateWindowDock 时已确定，展开->收起循环中应保持稳定。
    // 只有用户手动拖拽/缩放后才会 clearWindowDock 清状态，重新走 evaluateWindowDock。
    const nextState: WindowDockState = {
      edge: dockState.edge,
      phase: "collapsed",
      expandedBounds: dockState.expandedBounds,
      collapsedBounds: dockState.collapsedBounds,
    };

    scheduleCollapseSnap(nextState);
  }

  async function expandDockedWindow() {
    const dockState = dockStateRef.current;
    if (!dockState || dockState.phase !== "collapsed") {
      return;
    }

    // 取消可能未落地的收起 snap，避免展开途中窗口被收成细条
    clearEdgeDockSnapTimer();
    // 展开时窗口瞬时切回展开态几何。此时把手还挂在细条窗口上（snapped=true），
    // 几何突变会让把手位置跳变 -- 所以必须先把 snapped 设为 false 让把手卸载，
    // 再切几何，避免把手在展开态窗口里渲染导致闪动。
    // 内容此时 opacity 仍是 0（collapsed 态下 opacity:0），切几何不可见。
    const nextState: WindowDockState = {
      ...dockState,
      phase: "preview",
    };
    updateDockState(nextState, false);
    await setProgrammaticWindowBounds(
      nextState.expandedBounds.windowPosition,
      nextState.expandedBounds.windowSize,
      { edge: nextState.edge },
    );
    persistDockExpandedBounds(nextState);
  }

  async function activateWindowDock(dockBounds: WindowDockBounds) {
    if (!useSettingsStore.getState().settings.edgeDockCollapseEnabled) {
      return;
    }

    clearEdgeDockCollapseTimer();

    const nextState: WindowDockState = {
      edge: dockBounds.edge,
      phase: "collapsed",
      expandedBounds: {
        windowPosition: dockBounds.expandedPosition,
        windowSize: dockBounds.expandedSize,
      },
      collapsedBounds: {
        windowPosition: dockBounds.collapsedPosition,
        windowSize: dockBounds.collapsedSize,
      },
    };

    scheduleCollapseSnap(nextState);
  }

  function clearWindowDock(bounds?: {
    windowPosition?: LogicalWindowPosition | null;
    windowSize?: LogicalWindowSize | null;
  }) {
    clearEdgeDockCollapseTimer();
    clearEdgeDockEvaluateTimer();
    clearEdgeDockSnapTimer();
    updateDockState(null, false);

    if (bounds) {
      scheduleWindowBoundsSave(bounds);
    }
  }

  async function evaluateWindowDock(
    windowPosition: LogicalWindowPosition,
    windowSize: LogicalWindowSize,
  ) {
    if (!useSettingsStore.getState().settings.edgeDockCollapseEnabled) {
      dragStartWindowSizeRef.current = null;
      clearWindowDock({
        windowPosition,
        windowSize,
      });
      return;
    }

    // 系统原生分屏/最大化明显改写了窗口尺寸：让系统行为生效，放弃应用内收起
    if (wasLikelyResizedBySystemSnap(dragStartWindowSizeRef.current, windowSize)) {
      dragStartWindowSizeRef.current = null;
      clearWindowDock({
        windowPosition,
        windowSize,
      });
      return;
    }

    dragStartWindowSizeRef.current = null;
    const dockBounds = await resolveWindowDockBounds(windowPosition, windowSize, {
      requireExceeded: true,
    });
    if (dockBounds) {
      await activateWindowDock(dockBounds);
    } else {
      clearWindowDock({
        windowPosition,
        windowSize,
      });
    }
  }

  function scheduleWindowDockEvaluation(
    windowPosition: LogicalWindowPosition,
    windowSize: LogicalWindowSize,
  ) {
    if (!titlebarDraggingRef.current || Date.now() >= titlebarDragIntentUntilRef.current) {
      return;
    }

    latestDragBoundsRef.current = {
      windowPosition,
      windowSize,
    };
    clearEdgeDockEvaluateTimer();
    edgeDockEvaluateTimerRef.current = setTimeout(() => {
      edgeDockEvaluateTimerRef.current = null;
      titlebarDraggingRef.current = false;
      titlebarDragIntentUntilRef.current = 0;
      const latestBounds = latestDragBoundsRef.current;
      latestDragBoundsRef.current = null;

      if (!latestBounds) {
        return;
      }

      void (async () => {
        await evaluateWindowDock(latestBounds.windowPosition, latestBounds.windowSize);
      })();
    }, 420);
  }

  function registerTitlebarDragIntent() {
    titlebarDraggingRef.current = true;
    titlebarDragIntentUntilRef.current = Date.now() + 5000;
    latestDragBoundsRef.current = null;
    dragStartWindowSizeRef.current = null;
    clearEdgeDockCollapseTimer();

    void (async () => {
      try {
        const currentWindow = getCurrentWindow();
        // 拖拽起点不换显示器，直接复用缓存的 scaleFactor
        const scaleFactor = await resolveScaleFactor();
        if (scaleFactor === null) {
          dragStartWindowSizeRef.current = null;
          return;
        }
        const size = await currentWindow.innerSize();
        dragStartWindowSizeRef.current = toLogicalWindowSize(size, scaleFactor);
      } catch {
        dragStartWindowSizeRef.current = null;
      }
    })();
  }

  function finishTitlebarDragIntent() {
    if (!titlebarDraggingRef.current) {
      return;
    }

    titlebarDraggingRef.current = false;
    titlebarDragIntentUntilRef.current = 0;
    clearEdgeDockEvaluateTimer();

    // 拖动结束：恢复 backdrop-filter（与 TitleBar 的 handleMouseUp 配合，
    // 这里覆盖鼠标松手在窗口外的场景）
    document.documentElement.style.setProperty("--backdrop-blur", "");

    const latestBounds = latestDragBoundsRef.current;
    latestDragBoundsRef.current = null;
    if (!latestBounds) {
      dragStartWindowSizeRef.current = null;
      return;
    }

    void evaluateWindowDock(latestBounds.windowPosition, latestBounds.windowSize);
  }

  function handleAppMouseEnter() {
    if (!useSettingsStore.getState().settings.edgeDockCollapseEnabled) {
      return;
    }

    clearEdgeDockCollapseTimer();

    if (dockStateRef.current?.phase === "collapsed") {
      void expandDockedWindow();
    }
  }

  function handleAppMouseLeave() {
    if (!useSettingsStore.getState().settings.edgeDockCollapseEnabled) {
      return;
    }

    const dockState = dockStateRef.current;
    if (!dockState || dockState.phase !== "preview" || Date.now() < titlebarDragIntentUntilRef.current) {
      return;
    }

    clearEdgeDockCollapseTimer();
    edgeDockCollapseTimerRef.current = setTimeout(() => {
      edgeDockCollapseTimerRef.current = null;
      void collapseDockedWindow();
    }, 180);
  }

  // ---- 对外初始化：恢复窗口边界 + 注册移动/缩放监听，返回清理函数 ----

  const initializeWindowLifecycle = useCallback(async () => {
    // 初始化 scaleFactor 缓存，后续拖拽时 onMoved/onResized 复用，避免高频 IPC 卡顿
    await fetchAndCacheScaleFactor();
    await restoreWindowBounds();

    const currentWindow = getCurrentWindow();

    const unlistenWindowResized = await currentWindow.onResized(async ({ payload }) => {
      if (restoringWindowBoundsRef.current) {
        return;
      }

      const isManualResize = !isProgrammaticWindowResize();

      if (isManualResize) {
        suppressAutoFitAfterManualResize();
      }

      // 缩放不换显示器，直接复用缓存的 scaleFactor；缓存缺失时才回退到异步获取
      const scaleFactor = await resolveScaleFactor();
      if (scaleFactor === null) {
        return;
      }
      const nextSize = toLogicalWindowSize(payload, scaleFactor);
      // 维护最新窗口尺寸缓存，供 onMoved 边缘吸附检测复用
      latestWindowSizeRef.current = nextSize;
      const dockState = dockStateRef.current;

      if (dockState) {
        if (dockState.phase === "preview" && isManualResize) {
          clearWindowDock({
            windowPosition: dockState.expandedBounds.windowPosition,
            windowSize: nextSize,
          });
          return;
        }

        // programmatic 的 setSize（如展开时切回展开态几何）会触发 onResized，
        // 此时不应把实际落定尺寸写回 expandedBounds -- DPI 取整可能让 nextSize
        // 与原 expandedBounds.windowSize 差 1px，反复展开收起会累积漂移。
        // 只更新 latestWindowSizeRef 供 onMoved 复用，expandedBounds 保持原值。
        if (isProgrammaticWindowResize()) {
          return;
        }

        if (dockState.phase !== "collapsed") {
          dockStateRef.current = {
            ...dockState,
            expandedBounds: {
              ...dockState.expandedBounds,
              windowSize: nextSize,
            },
          };
        }

        persistDockExpandedBounds(dockStateRef.current ?? dockState);
        return;
      }

      scheduleWindowBoundsSave({
        windowSize: nextSize,
      });
    });

    const unlistenWindowMoved = await currentWindow.onMoved(async ({ payload }) => {
      if (restoringWindowBoundsRef.current) {
        return;
      }

      // 物理坐标越出缓存显示器边界（拖到不同 DPI 显示器）时重取 scaleFactor（M13）
      const scaleFactor = await resolveScaleFactor({ x: payload.x, y: payload.y });
      if (scaleFactor === null) {
        return;
      }
      const nextPosition = toLogicalWindowPosition(payload, scaleFactor);
      if (!nextPosition) {
        return;
      }

      const dockState = dockStateRef.current;
      if (dockState) {
        if (isProgrammaticWindowMove()) {
          persistDockExpandedBounds(dockState);
          return;
        }

        if (dockState.phase === "preview") {
          // 用缓存的窗口尺寸，避免每次 onMoved 都 await innerSize()
          const cachedSize = latestWindowSizeRef.current ?? dockState.expandedBounds.windowSize;
          clearWindowDock({
            windowPosition: nextPosition,
            windowSize: cachedSize,
          });
        } else {
          dockStateRef.current = dockState.phase === "collapsed"
            ? dockState
            : {
              ...dockState,
              expandedBounds: {
                ...dockState.expandedBounds,
                windowPosition: nextPosition,
              },
            };
          persistDockExpandedBounds(dockStateRef.current ?? dockState);
        }
      } else {
        scheduleWindowBoundsSave({
          windowPosition: nextPosition,
        });
      }

      if (!isProgrammaticWindowMove()) {
        // 用缓存的窗口尺寸做边缘吸附检测，避免拖拽时高频 await innerSize()
        const cachedSize = latestWindowSizeRef.current;
        if (cachedSize) {
          scheduleWindowDockEvaluation(nextPosition, cachedSize);
        }
      }
    });

    return () => {
      unlistenWindowResized();
      unlistenWindowMoved();
      clearWindowBoundsSaveTimer();
      clearEdgeDockCollapseTimer();
      clearEdgeDockEvaluateTimer();
      clearEdgeDockSnapTimer();
    };
  }, []);

  // 全局 mouseup：结束标题栏拖拽意图（与 TitleBar 的 handleMouseUp 配合，
  // 覆盖鼠标松手在窗口外的场景）
  useEffect(() => {
    function handleGlobalMouseUp() {
      finishTitlebarDragIntent();
    }

    window.addEventListener("mouseup", handleGlobalMouseUp);
    return () => {
      window.removeEventListener("mouseup", handleGlobalMouseUp);
    };
  }, []);

  // 关闭边缘吸附开关时：恢复展开态边界并清掉 dock 状态
  const edgeDockCollapseEnabled = useSettingsStore((state) => state.settings.edgeDockCollapseEnabled);
  useEffect(() => {
    if (edgeDockCollapseEnabled) {
      return;
    }

    clearEdgeDockCollapseTimer();
    clearEdgeDockEvaluateTimer();
    titlebarDraggingRef.current = false;
    titlebarDragIntentUntilRef.current = 0;
    latestDragBoundsRef.current = null;
    dragStartWindowSizeRef.current = null;

    const dockState = dockStateRef.current;
    if (!dockState) {
      return;
    }

    void (async () => {
      clearEdgeDockSnapTimer();
      if (dockState.phase === "collapsed") {
        await setProgrammaticWindowBounds(
          dockState.expandedBounds.windowPosition,
          dockState.expandedBounds.windowSize,
          { edge: dockState.edge },
        );
      }

      updateDockState(null, false);
      scheduleWindowBoundsSave({
        windowPosition: dockState.expandedBounds.windowPosition,
        windowSize: dockState.expandedBounds.windowSize,
      });
    })();
  }, [edgeDockCollapseEnabled]);

  // 收起态给 #app 挂 class，驱动边缘细条样式
  useEffect(() => {
    const appElement = document.getElementById("app");
    if (!appElement) {
      return;
    }

    const isCollapsed = dockVisualState?.phase === "collapsed";
    appElement.classList.toggle("app-edge-docked-collapsed", isCollapsed);

    return () => {
      appElement.classList.remove("app-edge-docked-collapsed");
    };
  }, [dockVisualState?.phase]);

  return {
    dockVisualState,
    initializeWindowLifecycle,
    registerTitlebarDragIntent,
    handleAppMouseEnter,
    handleAppMouseLeave,
  };
}
