type EdgeDockHandleProps = {
  edge: "left" | "right" | "top";
  /** 展开瞬间保留 150ms 的退场溶解（App.tsx 延迟卸载驱动） */
  exiting?: boolean;
};

export default function EdgeDockHandle({ edge, exiting = false }: EdgeDockHandleProps) {
  return (
    <div className={`edge-dock-handle edge-${edge}${exiting ? " is-exiting" : ""}`} aria-hidden="true">
      <div className="edge-dock-handle-shell">
        <span className="edge-dock-handle-glow" />
        <span className="edge-dock-handle-core" />
        <span className="edge-dock-handle-shine" />
      </div>
    </div>
  );
}
