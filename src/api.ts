// 对 Tauri 命令和事件的类型化封装。
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";
import type {
  AgendaItem,
  BuildStatus,
  HolidayInfo,
  StripView,
  EventData,
  EventRecord,
  FoundPet,
  IntegrationStatus,
  SkinInfo,
  SkinSheet,
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

  listHolidays: () => invoke<HolidayInfo[]>("list_holidays"),

  worldView: () => invoke<StripView | null>("world_view"),
  worldSet: (x: number, y: number, block: string) => invoke<boolean>("world_set", { x, y, block }),
  worldClear: (all: boolean) => invoke<void>("world_clear", { all }),
  getBuildMode: () => invoke<BuildStatus>("get_build_mode"),
  setBuildMode: (on: boolean) => invoke<BuildStatus>("set_build_mode", { on }),
  importTexturePack: () => invoke<{ matched: string[]; missing: string[] } | null>("import_texture_pack"),
  clearTexturePack: () => invoke<void>("clear_texture_pack"),
  getTextures: () => invoke<Record<string, string>>("get_textures"),

  listSkins: () => invoke<SkinInfo[]>("list_skins"),
  discoverSkins: () => invoke<FoundPet[]>("discover_skins"),
  importSkin: () => invoke<SkinInfo | null>("import_skin"),
  importSkinFrom: (path: string) => invoke<SkinInfo>("import_skin_from", { path }),
  deleteSkin: (id: string) => invoke<void>("delete_skin", { id }),
  getSkinSheet: (id: string) => invoke<SkinSheet | null>("get_skin_sheet", { id }),
  setPetSkin: (id: string | null) => invoke<void>("set_pet_skin", { id }),

  getAutostart: () => invoke<boolean>("get_autostart"),
  setAutostart: (enable: boolean) => invoke<boolean>("set_autostart", { enable }),
};

export function on<T>(event: string, handler: (payload: T) => void): Promise<UnlistenFn> {
  return listen<T>(event, (e) => handler(e.payload));
}
