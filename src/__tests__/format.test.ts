import { describe, expect, it } from "vitest";
import { addDays, groupSessions, parseLocalIso, projectName, relativeTime, startOfWeek, toLocalIso } from "../lib/format";
import { NOW, machine, session } from "./fixtures";

describe("projectName", () => {
  it("取路径最后一段，兼容 Windows", () => {
    expect(projectName("/home/me/web-app/")).toBe("web-app");
    expect(projectName("C:\\Users\\me\\code\\api")).toBe("api");
    expect(projectName("")).toBe("（未知目录）");
  });
});

describe("groupSessions", () => {
  it("按机器 → 项目 → 会话分组，需要处理的排在前面", () => {
    const m = machine({
      sessions: [
        session({ session_id: "a", cwd: "/p/alpha", status: "done", updated_ms: NOW - 1000 }),
        session({ session_id: "b", cwd: "/p/beta", status: "waiting" }),
        session({ session_id: "c", cwd: "/p/alpha", status: "error" }),
        session({ session_id: "d", cwd: "/p/alpha", status: "running" }),
      ],
    });
    const [g] = groupSessions([m]);
    expect(g.attention).toBe(2);
    expect(g.projects.map((p) => p.name)).toEqual(["beta", "alpha"]);
    expect(g.projects[1].sessions.map((s) => s.session_id)).toEqual(["c", "d", "a"]);
  });
});

describe("时间", () => {
  it("相对时间", () => {
    expect(relativeTime(NOW - 5_000, NOW)).toBe("刚刚");
    expect(relativeTime(NOW - 45_000, NOW)).toBe("45 秒前");
    expect(relativeTime(NOW - 3 * 60_000, NOW)).toBe("3 分钟前");
    expect(relativeTime(NOW - 2 * 3600_000, NOW)).toBe("2 小时前");
    expect(relativeTime(new Date(2026, 9, 30, 9, 5).getTime(), NOW)).toBe("10-30 09:05");
  });
  it("本地时间字符串往返", () => {
    const d = new Date(2026, 10, 2, 9, 30, 0);
    expect(toLocalIso(d)).toBe("2026-11-02T09:30:00");
    expect(parseLocalIso("2026-11-02T09:30:00").getTime()).toBe(d.getTime());
    expect(parseLocalIso("2026-11-02").getHours()).toBe(0);
  });
  it("周一是一周第一天", () => {
    // 2026-11-04 是周三
    expect(toLocalIso(startOfWeek(new Date(2026, 10, 4, 15)))).toBe("2026-11-02T00:00:00");
    expect(toLocalIso(startOfWeek(new Date(2026, 10, 8, 15)))).toBe("2026-11-02T00:00:00");
    expect(toLocalIso(addDays(new Date(2026, 10, 30), 1))).toBe("2026-12-01T00:00:00");
  });
});
