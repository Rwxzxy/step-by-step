import { render, screen, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { NOW, machine, session, snapshot } from "./fixtures";

const calls: { name: string; args: unknown[] }[] = [];
vi.mock("../api", () => {
  const record =
    (name: string) =>
    (...args: unknown[]) => {
      calls.push({ name, args });
      return Promise.resolve();
    };
  return {
    api: new Proxy({}, { get: (_t, name: string) => record(name) }),
    on: () => Promise.resolve(() => undefined),
  };
});

import { AgentSection } from "../panel/AgentSection";
import { MachinesSection } from "../panel/MachinesSection";
import { Overview } from "../panel/PanelApp";
import { TodaySection } from "../panel/TodaySection";

beforeEach(() => {
  calls.length = 0;
});

describe("Agent 进度", () => {
  it("没有会话时提示去接入", () => {
    render(<AgentSection machines={[machine({})]} now={NOW} />);
    expect(screen.getByText(/一键接入后/)).toBeInTheDocument();
  });

  it("显示 Agent 类型、状态、计划完成情况和更新时间", () => {
    const m = machine({
      sessions: [
        session({
          session_id: "x",
          status: "waiting",
          detail: "等待批准：Bash",
          plan: [
            { text: "读代码", status: "completed" },
            { text: "写测试", status: "in_progress", active_text: "正在写测试" },
            { text: "提交", status: "pending" },
          ],
          updated_ms: NOW - 3 * 60_000,
        }),
        session({ session_id: "y", agent: "codex", cwd: "/home/me/api", status: "done", plan: [] }),
      ],
    });
    render(<AgentSection machines={[m]} now={NOW} />);
    const cards = screen.getAllByTestId("session");
    expect(cards).toHaveLength(2);
    const waiting = cards[0];
    expect(within(waiting).getByText("Claude")).toBeInTheDocument();
    expect(within(waiting).getByText("等我回复")).toBeInTheDocument();
    expect(within(waiting).getByText("等待批准：Bash")).toBeInTheDocument();
    expect(within(waiting).getByText("1/3")).toBeInTheDocument();
    expect(within(waiting).getByLabelText("已完成：读代码")).toBeInTheDocument();
    expect(within(waiting).getByLabelText("进行中：写测试")).toHaveTextContent("正在写测试");
    expect(within(waiting).getByLabelText("未开始：提交")).toBeInTheDocument();
    expect(within(waiting).getByText("3 分钟前")).toBeInTheDocument();
    const done = cards[1];
    expect(within(done).getByText("Codex")).toBeInTheDocument();
    expect(within(done).getByText("完成")).toBeInTheDocument();
    expect(screen.getByText("1 个需要处理")).toBeInTheDocument();
  });

  it("按机器和项目目录分组", () => {
    const me = machine({ sessions: [session({ cwd: "/home/me/web-app" })] });
    const other = machine({
      node_id: "n2",
      name: "笔记本",
      is_me: false,
      online: false,
      shares_content_with_me: false,
      sessions: [session({ session_id: "r", cwd: "D:\\work\\game", status: "error", detail: "rate_limit" })],
    });
    render(<AgentSection machines={[me, other]} now={NOW} />);
    const laptop = screen.getByTestId("machine-n2");
    expect(within(laptop).getByText("笔记本")).toBeInTheDocument();
    expect(within(laptop).getByText("离线")).toBeInTheDocument();
    expect(within(laptop).getByText("D:\\work\\game")).toBeInTheDocument();
    expect(within(laptop).getByTitle("D:\\work\\game")).toHaveTextContent("📁 game");
    expect(within(laptop).getByText("出错")).toBeInTheDocument();
    expect(within(screen.getByTestId("machine-me")).getByText("本机")).toBeInTheDocument();
  });

  it("没有授权时不显示其他机器的消息正文", () => {
    const other = machine({
      node_id: "n2",
      is_me: false,
      shares_content_with_me: false,
      sessions: [session({ last_prompt: "秘密的提示词", status: "done" })],
    });
    render(<AgentSection machines={[other]} now={NOW} />);
    expect(screen.queryByText("秘密的提示词")).not.toBeInTheDocument();
  });

  it("本机会话可以展开最近的消息", () => {
    render(<AgentSection machines={[machine({ sessions: [session({ last_prompt: "帮我修 bug", last_reply: "修好了" })] })]} now={NOW} />);
    expect(screen.getByText("最近的消息")).toBeInTheDocument();
    expect(screen.getByText("帮我修 bug")).toBeInTheDocument();
  });
});

describe("今天的日程", () => {
  it("显示时间、标题、重复和提醒，标记进行中和已结束", async () => {
    const now = new Date(2026, 10, 2, 10, 0);
    const mk = (id: string, start: string, end: string, title: string, extra = {}) => ({
      event_id: id,
      start,
      end,
      version: 1,
      modified_by: "me",
      conflict_of: null,
      data: { title, notes: "", all_day: false, start, end, repeat: "none" as const, remind_before_min: null, ...extra },
    });
    const items = [
      mk("a", "2026-11-02T00:00:00", "2026-11-03T00:00:00", "假期", { all_day: true }),
      mk("b", "2026-11-02T08:00:00", "2026-11-02T09:00:00", "早会"),
      mk("c", "2026-11-02T09:30:00", "2026-11-02T10:30:00", "评审", { repeat: "weekdays", remind_before_min: 10 }),
    ];
    const open = vi.fn();
    render(<TodaySection items={items} onOpenCalendar={open} now={now} />);
    expect(screen.getByText("全天")).toBeInTheDocument();
    expect(screen.getByText("08:00–09:00").closest("li")).toHaveClass("past");
    const review = screen.getByText("09:30–10:30").closest("li")!;
    expect(review).toHaveClass("ongoing");
    expect(within(review).getByText("进行中")).toBeInTheDocument();
    expect(within(review).getByText("工作日 · 提前 10 分钟提醒")).toBeInTheDocument();
    await userEvent.click(screen.getByText("打开日程表 →"));
    expect(open).toHaveBeenCalled();
  });

  it("空状态", () => {
    render(<TodaySection items={[]} onOpenCalendar={() => undefined} />);
    expect(screen.getByText("今天没有日程。")).toBeInTheDocument();
  });
});

describe("局域网里的机器", () => {
  it("没有家庭组时引导创建", () => {
    render(<MachinesSection snap={snapshot({ discovered_unpaired: 2 })} onOpenSettings={() => undefined} />);
    expect(screen.getByText(/发现了 2 台装了步步的电脑/)).toBeInTheDocument();
  });

  it("列出其他机器的在线状态，宠物在串门时可以召回", async () => {
    const snap = snapshot({
      group: { group_id: "g", epoch: 0, layout: ["me", "n2"] },
      machines: [
        machine({}),
        machine({ node_id: "n2", name: "笔记本", is_me: false, online: true, sessions: [session({ status: "running" }), session({ session_id: "w", status: "waiting" })] }),
        machine({ node_id: "n3", name: "旧电脑", is_me: false, online: false }),
      ],
      pet: { location: "away", away_at: "n2", away_at_name: "笔记本", guests: [{ owner: "n3", owner_name: "小红", machine_name: "旧电脑" }], left_neighbor: null, right_neighbor: "n2" },
    });
    render(<MachinesSection snap={snap} onOpenSettings={() => undefined} />);
    const rows = screen.getAllByTestId("machine-row");
    expect(rows).toHaveLength(2);
    expect(within(rows[0]).getByText("在线")).toBeInTheDocument();
    expect(within(rows[0]).getByText(/1 个运行中，1 个在等/)).toBeInTheDocument();
    expect(within(rows[1]).getByText("离线")).toBeInTheDocument();
    expect(screen.getByText("我的步步在「笔记本」串门。")).toBeInTheDocument();
    await userEvent.click(screen.getByText("召回"));
    expect(calls.some((c) => c.name === "petRecall")).toBe(true);
    expect(screen.getByText(/小红的步步/)).toBeInTheDocument();
    await userEvent.click(screen.getByText("送它回家"));
    expect(calls.find((c) => c.name === "petSendGuestHome")?.args).toEqual(["n3"]);
  });

  it("在家时可以去在线的邻居家串门", async () => {
    const snap = snapshot({
      group: { group_id: "g", epoch: 0, layout: ["n2", "me"] },
      machines: [machine({}), machine({ node_id: "n2", name: "笔记本", is_me: false, online: true })],
      pet: { location: "home", away_at: null, away_at_name: null, guests: [], left_neighbor: "n2", right_neighbor: null },
    });
    render(<MachinesSection snap={snap} onOpenSettings={() => undefined} />);
    await userEvent.click(screen.getByText("← 去「笔记本」串门"));
    expect(calls.find((c) => c.name === "petGoVisit")?.args).toEqual(["left"]);
  });
});

describe("概览", () => {
  it("同时显示三块内容", () => {
    render(<Overview snap={snapshot()} go={() => undefined} />);
    expect(screen.getByRole("heading", { name: "Agent 进度" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "今天的日程" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "局域网里的机器" })).toBeInTheDocument();
  });
});
