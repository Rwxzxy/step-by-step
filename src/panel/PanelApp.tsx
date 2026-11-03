// 面板窗口：概览（三块）/ 日程表 / 设置 / 首次使用引导。
import { useCallback, useEffect, useState } from "react";
import { api, on } from "../api";
import type { Snapshot } from "../types";
import { PetSprite } from "../pet/PetSprite";
import { AgentSection } from "./AgentSection";
import { CalendarView } from "./CalendarView";
import { MachinesSection } from "./MachinesSection";
import { SettingsView } from "./SettingsView";
import { TodaySection } from "./TodaySection";
import "./panel.css";

export type Tab = "overview" | "calendar" | "settings" | "welcome";

export function Overview({ snap, go }: { snap: Snapshot; go: (t: Tab) => void }) {
  return (
    <div className="overview">
      <AgentSection machines={snap.machines} />
      <div className="side">
        <TodaySection items={snap.today} onOpenCalendar={() => go("calendar")} />
        <MachinesSection snap={snap} onOpenSettings={() => go("settings")} />
      </div>
    </div>
  );
}

export function PanelApp() {
  // 面板窗口是按需创建的，打开哪个页面由 URL 参数决定。
  const [tab, setTab] = useState<Tab>(() => (new URLSearchParams(window.location.search).get("tab") as Tab) || "overview");
  const [snap, setSnap] = useState<Snapshot | null>(null);
  const [toasts, setToasts] = useState<{ id: number; text: string }[]>([]);

  const notify = useCallback((text: string) => {
    const id = Date.now() + Math.random();
    setToasts((t) => [...t, { id, text }]);
    window.setTimeout(() => setToasts((t) => t.filter((x) => x.id !== id)), 5000);
  }, []);

  const refresh = useCallback(() => {
    api.snapshot().then(setSnap).catch(() => undefined);
  }, []);

  useEffect(() => {
    refresh();
    const subs = [
      on("steppy://changed", refresh),
      on<string>("steppy://navigate", (t) => setTab((t as Tab) || "overview")),
      on<string>("steppy://toast", notify),
    ];
    const poll = window.setInterval(refresh, 15_000);
    return () => {
      subs.forEach((p) => p.then((f) => f()));
      window.clearInterval(poll);
    };
  }, [refresh, notify]);

  if (!snap) return <div className="loading">步步正在醒来…</div>;

  const attention = snap.machines.flatMap((m) => m.sessions).filter((s) => s.status === "waiting" || s.status === "error").length;

  return (
    <div className="app">
      <nav className="sidebar" aria-label="导航">
        <div className="brand">
          <PetSprite anim="idle" facing="right" hue={snap.me.hue} size={44} holiday={snap.holiday?.id} />
          <div>
            <div className="brand-name">步步</div>
            <div className="brand-en">Steppy</div>
          </div>
        </div>
        <button className={tab === "overview" ? "on" : ""} onClick={() => setTab("overview")}>
          概览 {attention > 0 && <span className="badge">{attention}</span>}
        </button>
        <button className={tab === "calendar" ? "on" : ""} onClick={() => setTab("calendar")}>
          日程表
        </button>
        <button className={tab === "settings" ? "on" : ""} onClick={() => setTab("settings")}>
          设置
        </button>
        <div className="sidebar-foot muted small">
          {snap.me.machine_name}
          {snap.holiday && (
            <div className="tag holiday" title={snap.holiday.greeting}>
              🎉 {snap.holiday.name}
            </div>
          )}
          {snap.quiet_now && <div className="tag warn">免打扰中</div>}
        </div>
      </nav>
      <main className="main">
        {tab === "overview" && <Overview snap={snap} go={setTab} />}
        {tab === "calendar" && <CalendarView notify={notify} />}
        {tab === "settings" && <SettingsView snap={snap} notify={notify} />}
        {tab === "welcome" && <Welcome go={setTab} hue={snap.me.hue} />}
      </main>
      <div className="toasts" aria-live="polite">
        {toasts.map((t) => (
          <div key={t.id} className="toast">
            {t.text}
          </div>
        ))}
      </div>
    </div>
  );
}

function Welcome({ go, hue }: { go: (t: Tab) => void; hue: number }) {
  const finish = async () => {
    const s = await api.getSettings();
    await api.saveSettings({ ...s, onboarded: true });
    go("settings");
  };
  return (
    <div className="welcome card">
      <PetSprite anim="happy" facing="right" hue={hue} size={120} />
      <h1>你好，我是步步！</h1>
      <p>我会待在屏幕底部，帮你盯着 Claude Code 和 Codex 的进度，按时提醒日程，还能去家里其他电脑上串门。</p>
      <ol>
        <li>在“设置 → 接入 Claude / Codex”里一键接入（会自动备份你的配置）。</li>
        <li>第一台电脑点“创建家庭组”；其他电脑点“加入”，输入第一台电脑上显示的 6 位配对码。</li>
        <li>在“摆放位置”里按真实位置从左到右排好各台电脑。</li>
        <li>Windows 第一次运行时如果弹出防火墙提示，请允许“专用网络”，否则局域网联机不可用。</li>
      </ol>
      <p className="muted">点我打开面板，按住可以把我拖到别处；托盘图标里可以隐藏我、打开设置或退出。</p>
      <button className="primary" onClick={finish}>
        开始设置
      </button>
    </div>
  );
}
