import type { AgentSession, MachineView, Snapshot } from "../types";

export const NOW = new Date(2026, 10, 2, 10, 0, 0).getTime();

export function session(p: Partial<AgentSession>): AgentSession {
  return {
    agent: "claude",
    session_id: "s1",
    cwd: "/home/me/web-app",
    status: "running",
    plan: [],
    started_ms: NOW - 600_000,
    updated_ms: NOW - 60_000,
    ...p,
  };
}

export function machine(p: Partial<MachineView>): MachineView {
  return {
    node_id: "me",
    name: "台式机",
    owner_name: "小明",
    is_me: true,
    online: true,
    shares_content_with_me: true,
    i_share_content: true,
    sessions: [],
    ...p,
  };
}

export function snapshot(p: Partial<Snapshot> = {}): Snapshot {
  return {
    me: { node_id: "me", machine_name: "台式机", owner_name: "小明", hue: 32 },
    machines: [machine({})],
    today: [],
    group: null,
    pairing: null,
    pet: { location: "home", away_at: null, away_at_name: null, guests: [], left_neighbor: null, right_neighbor: null },
    quiet_now: false,
    discovered_unpaired: 0,
    net_port: 47900,
    ...p,
  };
}
