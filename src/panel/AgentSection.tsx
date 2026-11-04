// 面板第一块：Agent 进度。按 机器 → 项目目录 → 会话 分组。
import { useEffect, useState } from "react";
import type { AgentSession, MachineView, PlanItem } from "../types";
import { AGENT_LABEL, STATUS_LABEL, groupSessions, planProgress, relativeTime } from "../lib/format";

export function AgentSection({ machines, now }: { machines: MachineView[]; now?: number }) {
  const [tick, setTick] = useState(Date.now());
  useEffect(() => {
    if (now !== undefined) return;
    const t = window.setInterval(() => setTick(Date.now()), 30_000);
    return () => window.clearInterval(t);
  }, [now]);
  const current = now ?? tick;
  const groups = groupSessions(machines);
  const total = machines.reduce((n, m) => n + m.sessions.length, 0);

  return (
    <section className="card" aria-labelledby="agents-title">
      <header className="card-head">
        <h2 id="agents-title">Agent 进度</h2>
        <span className="muted">{total} 个会话</span>
      </header>
      {total === 0 ? (
        <p className="empty">还没有 Agent 会话。在“设置 → 接入 Claude / Codex”里一键接入后，这里会显示进度和计划。</p>
      ) : (
        groups
          .filter((g) => g.machine.sessions.length > 0)
          .map((g) => (
            <div className="machine-block" key={g.machine.node_id} data-testid={`machine-${g.machine.node_id}`}>
              <h3 className="machine-title">
                <span className={`dot ${g.machine.online ? "on" : "off"}`} aria-hidden />
                {g.machine.name}
                {g.machine.is_me && <span className="tag">本机</span>}
                {!g.machine.online && <span className="tag gray">离线</span>}
                {g.attention > 0 && <span className="tag warn">{g.attention} 个需要处理</span>}
              </h3>
              {g.projects.map((p) => (
                <div className="project" key={p.cwd}>
                  <div className="project-title" title={p.cwd}>
                    📁 {p.name}
                    <span className="muted small path">{p.cwd}</span>
                  </div>
                  <ul className="sessions">
                    {p.sessions.map((s) => (
                      <SessionCard key={`${s.agent}:${s.session_id}`} s={s} now={current} showContent={g.machine.is_me || g.machine.shares_content_with_me} />
                    ))}
                  </ul>
                </div>
              ))}
            </div>
          ))
      )}
    </section>
  );
}

export function SessionCard({ s, now, showContent }: { s: AgentSession; now: number; showContent: boolean }) {
  const { done, total } = planProgress(s.plan);
  return (
    <li className={`session status-${s.status}`} data-testid="session">
      <div className="session-head">
        <span className={`agent agent-${s.agent}`}>{AGENT_LABEL[s.agent]}</span>
        <span className={`pill pill-${s.status}`}>{STATUS_LABEL[s.status]}</span>
        {s.detail && <span className="detail">{s.detail}</span>}
        <span className="spacer" />
        <time className="muted small" dateTime={new Date(s.updated_ms).toISOString()}>
          {relativeTime(s.updated_ms, now)}
        </time>
      </div>
      {showContent && s.title && <div className="session-title">{s.title}</div>}
      {total > 0 && (
        <div className="plan">
          <div className="plan-head">
            <span>计划</span>
            <span className="muted small">
              {done}/{total}
            </span>
            <span className="bar" aria-hidden>
              <span style={{ width: `${(done / total) * 100}%` }} />
            </span>
          </div>
          <ol className="plan-items">
            {s.plan.map((p, i) => (
              <PlanRow key={p.id ?? i} p={p} />
            ))}
          </ol>
        </div>
      )}
      {showContent && (s.last_prompt || s.last_reply) && (
        <details className="content">
          <summary>最近的消息</summary>
          {s.last_prompt && (
            <p>
              <b>我：</b>
              {s.last_prompt}
            </p>
          )}
          {s.last_reply && (
            <p>
              <b>{AGENT_LABEL[s.agent]}：</b>
              {s.last_reply}
            </p>
          )}
        </details>
      )}
    </li>
  );
}

function PlanRow({ p }: { p: PlanItem }) {
  const icon = p.status === "completed" ? "✓" : p.status === "in_progress" ? "▶" : "○";
  const label = p.status === "completed" ? "已完成" : p.status === "in_progress" ? "进行中" : "未开始";
  return (
    <li className={`plan-item plan-${p.status}`} aria-label={`${label}：${p.text}`}>
      <span className="plan-icon" aria-hidden>
        {icon}
      </span>
      <span>{p.status === "in_progress" && p.active_text ? p.active_text : p.text}</span>
    </li>
  );
}
