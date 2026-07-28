import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import test from "node:test";
import assert from "node:assert/strict";

const root = resolve(import.meta.dirname, "..");

function read(relativePath: string) {
  return readFileSync(resolve(root, relativePath), "utf8");
}

test("Tauri 配置只创建主窗口", () => {
  const config = JSON.parse(read("src-tauri/tauri.conf.json")) as {
    app?: { windows?: Array<{ label?: string }> };
  };

  assert.deepEqual(config.app?.windows?.map((window) => window.label), ["main"]);
});

test("React 入口不再分支渲染灵动岛", () => {
  const source = read("src/main.tsx");

  assert.doesNotMatch(source, /IslandWidget|island\.css|windowLabel\s*===\s*["']island["']/);
});

test("设置模型不再暴露灵动岛状态", () => {
  const source = read("src/types/settings.ts");

  assert.doesNotMatch(source, /islandVisible|islandPosition|islandDockEdge/);
});

test("主窗口不再同步或控制灵动岛窗口", () => {
  const sources = [
    read("src/App.tsx"),
    read("src/stores/providerStore.ts"),
    read("src/stores/settingsStore.ts"),
    read("src/utils/ipc.ts"),
    read("src/components/settings/SettingsPanel.tsx"),
    read("src-tauri/src/tray/mod.rs"),
    read("src-tauri/src/commands/settings_commands.rs"),
  ];

  for (const source of sources) {
    assert.doesNotMatch(source, /islandVisible|islandPosition|islandDockEdge|island-usage-update|save_island_position|island-visibility/);
  }
});
