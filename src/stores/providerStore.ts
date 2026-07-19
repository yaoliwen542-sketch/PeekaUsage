import { create } from "zustand";
import type { ProviderId, UsageSummary } from "../types/provider";
import { fetchAllUsage, fetchProviderUsage } from "../utils/ipc";

/** 把用量数据同步给灵动岛窗口（如果存在） */
async function syncToIsland(providers: UsageSummary[]) {
  try {
    const { emit } = await import("@tauri-apps/api/event");
    await emit("island-usage-update", providers);
  } catch {
    // 灵动岛窗口可能不存在，忽略错误
  }
}

type ProviderStoreState = {
  providers: UsageSummary[];
  isRefreshing: boolean;
  refreshingProviders: Partial<Record<ProviderId, boolean>>;
  lastError: string | null;
  refreshAll: () => Promise<void>;
  refreshProvider: (providerId: ProviderId) => Promise<void>;
  isProviderRefreshing: (providerId: ProviderId) => boolean;
};

// 全局刷新序号：每次发起 refreshAll 递增。
// 单卡刷新发起时记录当时序号，返回时若序号已推进，说明期间发生过全局刷新，
// 单卡结果属于过期数据，直接丢弃，避免慢请求用旧数据覆盖新数据。
let globalFetchSeq = 0;

export const useProviderStore = create<ProviderStoreState>((set, get) => ({
  providers: [],
  isRefreshing: false,
  refreshingProviders: {},
  lastError: null,

  async refreshAll() {
    if (get().isRefreshing) {
      return;
    }

    globalFetchSeq += 1;
    set({
      isRefreshing: true,
      lastError: null,
    });

    try {
      const providers = await fetchAllUsage();
      set({
        providers,
        isRefreshing: false,
      });
      void syncToIsland(providers);
    } catch (error: unknown) {
      set({
        isRefreshing: false,
        lastError: error instanceof Error ? error.toString() : "未知错误",
      });
    }
  },

  async refreshProvider(providerId: ProviderId) {
    const state = get();
    if (state.isRefreshing || state.refreshingProviders[providerId]) {
      return;
    }

    // 记录发起时的全局刷新序号，返回时用于判定结果是否已过期
    const seqAtStart = globalFetchSeq;

    set({
      refreshingProviders: {
        ...state.refreshingProviders,
        [providerId]: true,
      },
    });

    try {
      const updated = await fetchProviderUsage(providerId);

      if (seqAtStart !== globalFetchSeq) {
        // 请求在途期间发生过全局刷新，单卡结果已过期，丢弃
        return;
      }

      const nextProviders = [...get().providers];
      const index = nextProviders.findIndex((provider) => provider.providerId === providerId);

      if (index < 0) {
        // 请求在途期间该供应商已被禁用/移除，丢弃结果，
        // 不能再 push 回列表形成幽灵卡片
        return;
      }

      nextProviders[index] = updated;

      set({
        providers: nextProviders,
      });
      void syncToIsland(nextProviders);
    } catch (error: unknown) {
      set({
        lastError: error instanceof Error ? error.toString() : "未知错误",
      });
    } finally {
      set((current) => ({
        refreshingProviders: {
          ...current.refreshingProviders,
          [providerId]: false,
        },
      }));
    }
  },

  isProviderRefreshing(providerId: ProviderId) {
    const state = get();
    return state.isRefreshing || !!state.refreshingProviders[providerId];
  },
}));
