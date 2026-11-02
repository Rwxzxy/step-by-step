// 与 Rust 端（serde，snake_case）一致的类型定义。

export type AgentKind = "claude" | "codex";
export type SessionStatus = "idle" | "running" | "waiting" | "done" | "error" | "ended";
export type PlanStatus = "pending" | "in_progress" | "completed";

export interface PlanItem {
  id?: string;
  text: string;
  status: PlanStatus;
  active_text?: string;
}

export interface AgentSession {
  agent: AgentKind;
  session_id: string;
  cwd: string;
  status: SessionStatus;
  detail?: string;
  plan: PlanItem[];
  started_ms: number;
  updated_ms: number;
  title?: string;
  last_prompt?: string;
  last_reply?: string;
}

export interface MachineView {
  node_id: string;
  name: string;
  owner_name: string;
  is_me: boolean;
  online: boolean;
  shares_content_with_me: boolean;
  i_share_content: boolean;
  sessions: AgentSession[];
}

export type Repeat = "none" | "daily" | "weekly" | "weekdays";

export interface EventData {
  title: string;
  notes: string;
  all_day: boolean;
  /** 本地时间 "YYYY-MM-DDTHH:MM:SS" */
  start: string;
  end: string;
  repeat: Repeat;
  remind_before_min: number | null;
}

export interface AgendaItem {
  event_id: string;
  start: string;
  end: string;
  data: EventData;
  version: number;
  modified_by: string;
  conflict_of: string | null;
}

export interface EventRecord {
  id: string;
  vv: Record<string, number>;
  data: EventData | null;
  modified_ms: number;
  modified_by: string;
  conflict_of?: string;
}

export interface GuestView {
  owner: string;
  owner_name: string;
  machine_name: string;
}

export interface PetStatus {
  location: "home" | "leaving" | "away" | "returning";
  away_at: string | null;
  away_at_name: string | null;
  guests: GuestView[];
  left_neighbor: string | null;
  right_neighbor: string | null;
}

export interface Snapshot {
  me: { node_id: string; machine_name: string; owner_name: string; hue: number };
  machines: MachineView[];
  today: AgendaItem[];
  group: { group_id: string; epoch: number; layout: string[] } | null;
  pairing: { code: string; expires_in_sec: number } | null;
  pet: PetStatus;
  quiet_now: boolean;
  discovered_unpaired: number;
  net_port: number;
}

export interface QuietPeriod {
  start: string;
  end: string;
  enabled: boolean;
}

export interface Settings {
  machine_name: string;
  owner_name: string;
  pet_hue: number;
  pet_visible: boolean;
  system_notifications: boolean;
  bubble_seconds: number;
  quiet_periods: QuietPeriod[];
  remind_remote_agents: boolean;
  calendar_reminders: boolean;
  allow_visits: boolean;
  wander_visits: boolean;
  visit_minutes: number;
  share_content_with: Record<string, boolean>;
  manual_peers: string[];
  net_port: number;
  onboarded: boolean;
}

export interface AgentIntegration {
  installed: boolean;
  config_path: string;
  config_exists: boolean;
  error: string | null;
}

export interface IntegrationStatus {
  claude: AgentIntegration;
  codex: AgentIntegration;
  hook_path: string;
  hook_available: boolean;
}

export type Anim = "idle" | "walk" | "sleep" | "happy" | "raise_hand" | "anxious" | "calendar" | "drag" | "fall";
export type Side = "left" | "right";

export interface FramePayload {
  anim: Anim;
  facing: Side;
  visible: boolean;
}

export interface WindowInfo {
  guest: boolean;
  owner_name: string;
  machine_name: string;
  hue: number;
  frame: FramePayload | null;
}

export interface BubbleAction {
  id: string;
  label: string;
}

export interface Bubble {
  id: string;
  text: string;
  kind: "done" | "waiting" | "error" | "calendar" | "info";
  actions: BubbleAction[];
  ttl_ms: number;
}
