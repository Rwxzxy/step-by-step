// 展示相关的纯函数（有单元测试）。
import type { AgentSession, MachineView, PlanItem, SessionStatus } from "../types";

export const STATUS_LABEL: Record<SessionStatus, string> = {
  idle: "空闲",
  running: "运行中",
  waiting: "等我回复",
  done: "完成",
  error: "出错",
  ended: "已结束",
};

export const AGENT_LABEL = { claude: "Claude", codex: "Codex" } as const;

/** 路径最后一段作为项目名，兼容 Windows 和 Unix。 */
export function projectName(cwd: string): string {
  const trimmed = cwd.replace(/[\\/]+$/, "");
  const parts = trimmed.split(/[\\/]/);
  const last = parts[parts.length - 1];
  return last || cwd || "（未知目录）";
}

export interface ProjectGroup {
  cwd: string;
  name: string;
  sessions: AgentSession[];
}

export interface MachineGroup {
  machine: MachineView;
  projects: ProjectGroup[];
  attention: number;
}

const STATUS_ORDER: Record<SessionStatus, number> = {
  waiting: 0,
  error: 1,
  running: 2,
  done: 3,
  idle: 4,
  ended: 5,
};

/** 按 机器 → 项目目录 → 会话 分组。需要我处理的会话排在前面。 */
export function groupSessions(machines: MachineView[]): MachineGroup[] {
  return machines.map((machine) => {
    const byCwd = new Map<string, AgentSession[]>();
    for (const s of machine.sessions) {
      const list = byCwd.get(s.cwd) ?? [];
      list.push(s);
      byCwd.set(s.cwd, list);
    }
    const projects: ProjectGroup[] = [...byCwd.entries()].map(([cwd, sessions]) => ({
      cwd,
      name: projectName(cwd),
      sessions: [...sessions].sort(
        (a, b) => STATUS_ORDER[a.status] - STATUS_ORDER[b.status] || b.updated_ms - a.updated_ms,
      ),
    }));
    projects.sort((a, b) => {
      const pa = Math.min(...a.sessions.map((s) => STATUS_ORDER[s.status]));
      const pb = Math.min(...b.sessions.map((s) => STATUS_ORDER[s.status]));
      return pa - pb || a.name.localeCompare(b.name, "zh-CN");
    });
    const attention = machine.sessions.filter((s) => s.status === "waiting" || s.status === "error").length;
    return { machine, projects, attention };
  });
}

export function planProgress(plan: PlanItem[]): { done: number; total: number } {
  return { done: plan.filter((p) => p.status === "completed").length, total: plan.length };
}

/** “刚刚 / 3 分钟前 / 2 小时前 / 11-02 09:15” */
export function relativeTime(ms: number, now: number = Date.now()): string {
  const diff = Math.max(0, now - ms);
  const s = Math.floor(diff / 1000);
  if (s < 30) return "刚刚";
  if (s < 60) return `${s} 秒前`;
  const m = Math.floor(s / 60);
  if (m < 60) return `${m} 分钟前`;
  const h = Math.floor(m / 60);
  if (h < 24) return `${h} 小时前`;
  const d = new Date(ms);
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/** 本机时间转为 "YYYY-MM-DDTHH:MM:SS"（后端使用的本地浮动时间）。 */
export function toLocalIso(d: Date): string {
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${d.getFullYear()}-${pad(d.getMonth() + 1)}-${pad(d.getDate())}T${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

export function parseLocalIso(s: string): Date {
  const [date, time = "00:00:00"] = s.split("T");
  const [y, mo, d] = date.split("-").map(Number);
  const [h, mi, se = 0] = time.split(":").map(Number);
  return new Date(y, mo - 1, d, h, mi, se);
}

export function hhmm(s: string): string {
  return s.slice(11, 16);
}

export function startOfDay(d: Date): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate());
}

export function addDays(d: Date, n: number): Date {
  return new Date(d.getFullYear(), d.getMonth(), d.getDate() + n, d.getHours(), d.getMinutes(), d.getSeconds());
}

/** 周一为一周的第一天。 */
export function startOfWeek(d: Date): Date {
  const day = (d.getDay() + 6) % 7;
  return startOfDay(addDays(d, -day));
}

export const WEEKDAYS = ["周一", "周二", "周三", "周四", "周五", "周六", "周日"];

export function dateLabel(d: Date): string {
  return `${d.getMonth() + 1} 月 ${d.getDate()} 日 ${WEEKDAYS[(d.getDay() + 6) % 7]}`;
}

export const REPEAT_LABEL = { none: "不重复", daily: "每天", weekly: "每周", weekdays: "工作日" } as const;

/** 描述一条日程的时间。 */
export function describeWhen(item: { start: string; end: string; data: { all_day: boolean } }): string {
  if (item.data.all_day) return "全天";
  return `${hhmm(item.start)}–${hhmm(item.end)}`;
}
