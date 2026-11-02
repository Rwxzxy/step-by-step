// 对 Tauri 命令和事件的类型化封装。
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AgendaItem,
  EventData,
  EventRecord,
  IntegrationStatus,
  Settings,
  Side,
  Snapshot,
  WindowInfo,
} from "./types";

export const api = {
  snapshot: () => invoke<Snapshot>("get_snapshot"),
  getSettings: () => invoke<Settings>("get_settings"),
  saveSettings: (settings: Settings) => invoke<Settings>("save_settings", { settings }),
  integrationStatus: () => invoke<IntegrationStatus>("integration_status"),
  setIntegration: (agent: "claude" | "codex", enable: boolean) => invoke<string>("set_integration", { agent, enable }),

  listAgenda: (from: string, to: string) => invoke<AgendaItem[]>("list_agenda", { from, to }),
  saveEvent: (id: string | null, event: EventData) => invoke<string>("save_event", { id, event }),
  deleteEvent: (id: string) => invoke<void>("delete_event", { id }),
  getEvent: (id: string) => invoke<EventRecord | null>("get_event", { id }),
  importIcs: () => invoke<string>("import_ics"),
  exportIcs: () => invoke<string>("export_ics"),

  createGroup: () => invoke<void>("create_group"),
  startPairing: () => invoke<string>("start_pairing"),
  cancelPairing: () => invoke<void>("cancel_pairing"),
  joinGroup: (code: string) => invoke<string>("join_group", { code }),
  kickMember: (node: string) => invoke<void>("kick_member", { node }),
  leaveGroup: () => invoke<void>("leave_group"),
  setLayout: (order: string[]) => invoke<void>("set_layout", { order }),
  setShareContent: (node: string, share: boolean) => invoke<void>("set_share_content", { node, share }),

  petWindowInfo: () => invoke<WindowInfo>("pet_window_info"),
  petHitRegions: (rects: { x: number; y: number; w: number; h: number }[]) => invoke<void>("pet_hit_regions", { rects }),
  petDragStart: () => invoke<void>("pet_drag_start"),
  petDragEnd: () => invoke<void>("pet_drag_end"),
  petClicked: () => invoke<void>("pet_clicked"),
  petRecall: () => invoke<void>("pet_recall"),
  petGoVisit: (side: Side) => invoke<void>("pet_go_visit", { side }),
  petSendGuestHome: (owner: string) => invoke<void>("pet_send_guest_home", { owner }),
  bubbleAction: (bubbleId: string, action: string) => invoke<void>("bubble_action", { bubbleId, action }),
  openPanel: (tab?: string) => invoke<void>("open_panel", { tab }),

  getAutostart: () => invoke<boolean>("get_autostart"),
  setAutostart: (enable: boolean) => invoke<boolean>("set_autostart", { enable }),
};

export function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload));
}
