import { useI18n } from "../../i18n";
import { useWindowControls } from "../../composables/useWindowControls";
import { cn } from "@/lib/utils";

/** 右侧透明度拖拽把手：窄条幽灵样式，hover / 拖拽时浮现 */
export default function OpacityHandle() {
  const { isDraggingOpacity, opacity, startOpacityDrag } = useWindowControls();
  const { t } = useI18n();

  return (
    <div
      className={cn(
        "absolute top-8 right-0 bottom-0 z-10 flex w-1.5 cursor-ns-resize items-center justify-center",
        "opacity-0 transition-opacity duration-200 hover:opacity-100",
        isDraggingOpacity && "opacity-100",
      )}
      onMouseDown={(event) => {
        event.preventDefault();
        startOpacityDrag(event.clientY);
      }}
      title={t("widget.opacityHandle.title", { opacity })}
    >
      <div className="flex h-[80%] w-[3px] flex-col-reverse overflow-hidden rounded-full bg-progress-track">
        <div
          className="w-full rounded-full bg-primary transition-[height] duration-100"
          style={{ height: `${opacity}%` }}
        />
      </div>
    </div>
  );
}
