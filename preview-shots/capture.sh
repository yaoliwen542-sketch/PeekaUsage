#!/usr/bin/env bash
# UI 预览截图：启动 vite dev → Edge 无头截取各场景 → PIL 裁剪缩放到目标 CSS 尺寸
#
# 注意：Edge 无头模式强制最小窗口宽度约 500 CSS px，无法直接截 400px 宽的浮窗。
# 方案：--window-size 给足 500 宽，DSF 1.25 保证位图不丢内容，
#       浮窗场景用 ?fixwidth=400 把 body 约束到真实窗口宽，再 PIL 裁剪左区并 0.8 倍还原。
set -u
cd "$(dirname "$0")/.."

PORT=5199
EDGE="/c/Program Files (x86)/Microsoft/Edge/Application/msedge.exe"
ROOT_WIN="G:\\Project\\my\\PeekaUsage"
OUT_DIR="preview-shots"
OUT_DIR_WIN="$ROOT_WIN\\preview-shots"
PROFILE_DIR_WIN="$OUT_DIR_WIN\\.edge-profile"
mkdir -p "$OUT_DIR"

node node_modules/vite/bin/vite.js dev --port $PORT --strictPort --logLevel warn > "$OUT_DIR/vite.log" 2>&1 &
VITE_PID=$!

cleanup() {
  kill "$VITE_PID" 2>/dev/null
  wait "$VITE_PID" 2>/dev/null
}
trap cleanup EXIT

for i in $(seq 1 60); do
  if curl -s -o /dev/null "http://localhost:$PORT/preview.html"; then
    break
  fi
  sleep 0.5
done

# shot <name> <scene> <theme> <目标宽w> <目标高h> [fixwidth] [budget]
shot() {
  local name="$1" scene="$2" theme="$3" w="$4" h="$5" fixw="${6:-0}" budget="${7:-9000}"
  local vw=$w
  if [ "$vw" -lt 500 ]; then vw=500; fi
  local extra=""
  if [ "$fixw" -gt 0 ]; then extra="&fixwidth=$fixw"; fi
  local raw="$OUT_DIR/.raw-$name.png"
  timeout 90 "$EDGE" --headless=new --disable-gpu --no-first-run --no-default-browser-check \
    --user-data-dir="$PROFILE_DIR_WIN" --hide-scrollbars --force-device-scale-factor=1.25 \
    --window-size="$vw,$h" --virtual-time-budget="$budget" \
    --screenshot="$OUT_DIR_WIN\\.raw-$name.png" \
    "http://localhost:$PORT/preview.html?scene=$scene&theme=$theme&noanim=1$extra" > /dev/null 2>&1
  if [ -f "$raw" ]; then
    python "$OUT_DIR/post.py" "$raw" "$OUT_DIR/$name.png" "$w" "$h" && rm -f "$raw"
    echo "OK  $name.png"
  else
    echo "FAIL $name.png"
  fi
}

which="${1:-all}"

if [ "$which" = "all" ] || [ "$which" = "widget" ]; then
  shot widget-dark            widget            dark  400 960 400
  shot widget-light           widget            light 400 960 400
  shot widget-compact-dark    widget-compact    dark  400 900 400
  shot widget-compact-light   widget-compact    light 400 900 400
  shot stats-dark             stats             dark  400 800 400
  shot stats-light            stats             light 400 800 400
fi

if [ "$which" = "all" ] || [ "$which" = "island" ]; then
  shot island-dark            island            dark  200 40  200
  shot island-light           island            light 200 40  200
  shot island-expanded-dark   island-expanded   dark  300 400 300
  shot island-expanded-light  island-expanded   light 300 400 300
  shot dock-left-dark         dock-left         dark  24  136 24
  shot dock-top-dark          dock-top          dark  132 24  132
fi
echo DONE
