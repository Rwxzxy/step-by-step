// 设置：基本、提醒与免打扰、接入 Claude / Codex、家庭组、机器排列、串门、隐私。
import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { CustomHoliday, HolidayInfo, IntegrationStatus, QuietPeriod, Settings, Snapshot } from "../types";
import { PetSprite } from "../pet/PetSprite";

export function SettingsView({ snap, notify }: { snap: Snapshot; notify: (m: string) => void }) {
  const [s, setS] = useState<Settings | null>(null);
  const [autostart, setAutostart] = useState(false);

  useEffect(() => {
    api.getSettings().then(setS);
    api.getAutostart().then(setAutostart).catch(() => undefined);
  }, []);

  const save = useCallback(
    (patch: Partial<Settings>) => {
      setS((cur) => {
        if (!cur) return cur;
        const next = { ...cur, ...patch };
        api.saveSettings(next).then(setS).catch((e) => notify(String(e)));
        return next;
      });
    },
    [notify],
  );

  if (!s) return <p className="muted">加载中…</p>;

  return (
    <div className="settings">
      <section className="card">
        <h2>基本</h2>
        <div className="form-grid">
          <label>
            这台电脑的名字
            <input defaultValue={s.machine_name} onBlur={(e) => e.target.value !== s.machine_name && save({ machine_name: e.target.value })} />
          </label>
          <label>
            主人名字（宠物串门时显示）
            <input defaultValue={s.owner_name} onBlur={(e) => e.target.value !== s.owner_name && save({ owner_name: e.target.value })} />
          </label>
          <label className="hue">
            宠物颜色
            <span className="hue-row">
              <input type="range" min={0} max={359} value={s.pet_hue} onChange={(e) => setS({ ...s, pet_hue: Number(e.target.value) })} onMouseUp={() => save({ pet_hue: s.pet_hue })} onKeyUp={() => save({ pet_hue: s.pet_hue })} />
              <PetSprite anim="idle" facing="right" hue={s.pet_hue} size={56} />
            </span>
          </label>
        </div>
        <Toggle label="显示宠物" checked={s.pet_visible} onChange={(v) => save({ pet_visible: v })} />
        <Toggle
          label="开机自启"
          checked={autostart}
          onChange={(v) =>
            api
              .setAutostart(v)
              .then(setAutostart)
              .catch((e) => notify(String(e)))
          }
        />
      </section>

      <section className="card">
        <h2>提醒与免打扰</h2>
        <Toggle label="同时发系统通知" checked={s.system_notifications} onChange={(v) => save({ system_notifications: v })} />
        <Toggle label="其他电脑上的 Agent 完成或在等我时也提醒" checked={s.remind_remote_agents} onChange={(v) => save({ remind_remote_agents: v })} />
        <Toggle label="在这台电脑上提醒日程" checked={s.calendar_reminders} onChange={(v) => save({ calendar_reminders: v })} />
        <label className="inline">
          气泡显示
          <input type="number" min={2} max={120} value={s.bubble_seconds} onChange={(e) => setS({ ...s, bubble_seconds: Number(e.target.value) })} onBlur={() => save({ bubble_seconds: s.bubble_seconds })} />
          秒
        </label>
        <h3>免打扰时段 {snap.quiet_now && <span className="tag warn">现在处于免打扰</span>}</h3>
        <p className="muted small">时段内不冒气泡、不发系统通知。可以跨午夜，比如 22:30–08:00。</p>
        <QuietEditor periods={s.quiet_periods} onChange={(q) => save({ quiet_periods: q })} />
      </section>

      <HolidaySection settings={s} snap={snap} save={save} />

      <Integrations notify={notify} />

      <Group snap={snap} settings={s} save={save} notify={notify} />
    </div>
  );
}

export function HolidaySection({ settings, snap, save }: { settings: Settings; snap: Snapshot; save: (p: Partial<Settings>) => void }) {
  const [list, setList] = useState<HolidayInfo[]>([]);
  useEffect(() => {
    api.listHolidays().then(setList).catch(() => undefined);
  }, []);
  const custom = settings.custom_holidays;
  const setCustom = (c: CustomHoliday[]) => save({ custom_holidays: c });
  const update = (i: number, p: Partial<CustomHoliday>) => setCustom(custom.map((x, j) => (j === i ? { ...x, ...p } : x)));
  const days = (m: number) => new Date(2024, m, 0).getDate();
  return (
    <section className="card" aria-labelledby="holiday-title">
      <h2 id="holiday-title">
        节日主题 {snap.holiday && <span className="tag holiday">现在：{snap.holiday.name}</span>}
      </h2>
      <Toggle label="节日时自动切换宠物外观，并在当天送上祝福" checked={settings.holiday_theme} onChange={(v) => save({ holiday_theme: v })} />
      <p className="muted small">
        包括春节（除夕到初七）、元宵、清明、端午、七夕、中秋、重阳等农历节日，以及元旦、情人节、劳动节、儿童节、国庆、万圣节、圣诞。正式美术资源还没放进来时显示占位图形。
      </p>
      <label className="inline">
        预览
        <select
          aria-label="预览节日外观"
          value={settings.holiday_preview ?? ""}
          onChange={(e) => save({ holiday_preview: e.target.value || null })}
        >
          <option value="">不预览（按日期自动切换）</option>
          {list.map((h) => (
            <option key={h.id} value={h.id}>
              {h.name}
              {h.next_date ? `（${h.next_date.slice(5).replace("-", "月")}日）` : ""}
            </option>
          ))}
        </select>
      </label>
      <h3>自定义纪念日</h3>
      <p className="muted small">比如家人的生日（公历，每年重复）。当天宠物会换上纪念日外观并送上祝福。</p>
      {custom.map((c, i) => (
        <div className="row" key={i}>
          <input aria-label="名称" placeholder="名称" value={c.name} onChange={(e) => update(i, { name: e.target.value })} />
          <select aria-label="月" value={c.month} onChange={(e) => update(i, { month: Number(e.target.value), day: Math.min(c.day, days(Number(e.target.value))) })}>
            {Array.from({ length: 12 }, (_, k) => (
              <option key={k + 1} value={k + 1}>
                {k + 1} 月
              </option>
            ))}
          </select>
          <select aria-label="日" value={c.day} onChange={(e) => update(i, { day: Number(e.target.value) })}>
            {Array.from({ length: days(c.month) }, (_, k) => (
              <option key={k + 1} value={k + 1}>
                {k + 1} 日
              </option>
            ))}
          </select>
          <input aria-label="祝福语" placeholder="祝福语（可不填）" value={c.greeting} onChange={(e) => update(i, { greeting: e.target.value })} />
          <button onClick={() => setCustom(custom.filter((_, j) => j !== i))}>删除</button>
        </div>
      ))}
      <button onClick={() => setCustom([...custom, { name: "生日", month: new Date().getMonth() + 1, day: new Date().getDate(), greeting: "" }])}>
        + 添加纪念日
      </button>
    </section>
  );
}

function Toggle({ label, checked, onChange }: { label: string; checked: boolean; onChange: (v: boolean) => void }) {
  return (
    <label className="toggle">
      <input type="checkbox" checked={checked} onChange={(e) => onChange(e.target.checked)} />
      <span>{label}</span>
    </label>
  );
}

function QuietEditor({ periods, onChange }: { periods: QuietPeriod[]; onChange: (p: QuietPeriod[]) => void }) {
  const update = (i: number, p: Partial<QuietPeriod>) => onChange(periods.map((x, j) => (j === i ? { ...x, ...p } : x)));
  return (
    <div className="quiet">
      {periods.map((p, i) => (
        <div className="row" key={i}>
          <input type="checkbox" aria-label="启用" checked={p.enabled} onChange={(e) => update(i, { enabled: e.target.checked })} />
          <input type="time" value={p.start} onChange={(e) => e.target.value && update(i, { start: e.target.value })} />
          <span>到</span>
          <input type="time" value={p.end} onChange={(e) => e.target.value && update(i, { end: e.target.value })} />
          <button onClick={() => onChange(periods.filter((_, j) => j !== i))}>删除</button>
        </div>
      ))}
      <button onClick={() => onChange([...periods, { start: "12:00", end: "13:30", enabled: true }])}>+ 添加时段</button>
    </div>
  );
}

function Integrations({ notify }: { notify: (m: string) => void }) {
  const [st, setSt] = useState<IntegrationStatus | null>(null);
  const [busy, setBusy] = useState(false);
  const [msg, setMsg] = useState("");
  const load = () => api.integrationStatus().then(setSt);
  useEffect(() => {
    load();
  }, []);
  const run = async (agent: "claude" | "codex", enable: boolean) => {
    setBusy(true);
    try {
      const m = await api.setIntegration(agent, enable);
      setMsg(m);
      notify(enable ? "接入成功" : "已撤销");
    } catch (e) {
      setMsg(String(e));
    } finally {
      setBusy(false);
      load();
    }
  };
  if (!st) return null;
  return (
    <section className="card">
      <h2>接入 Claude / Codex</h2>
      <p className="muted small">
        修改前会自动备份原配置，并且只追加步步自己的条目，不会覆盖你已有的 hooks 和 notify。撤销时只删除步步的条目。
      </p>
      {!st.hook_available && <p className="tag warn">找不到 steppy-hook 程序，请重新安装步步。</p>}
      <div className="integration">
        <div>
          <b>Claude Code</b> {st.claude.installed ? <span className="tag ok">已接入</span> : <span className="tag gray">未接入</span>}
          <div className="muted small path">{st.claude.config_path}</div>
          {st.claude.error && <div className="tag warn">{st.claude.error}</div>}
        </div>
        <button disabled={busy} className={st.claude.installed ? "" : "primary"} onClick={() => run("claude", !st.claude.installed)}>
          {st.claude.installed ? "撤销接入" : "一键接入"}
        </button>
      </div>
      <div className="integration">
        <div>
          <b>Codex CLI</b> {st.codex.installed ? <span className="tag ok">已接入</span> : <span className="tag gray">未接入</span>}
          <div className="muted small path">{st.codex.config_path}</div>
          {st.codex.error && <div className="tag warn">{st.codex.error}</div>}
        </div>
        <button disabled={busy} className={st.codex.installed ? "" : "primary"} onClick={() => run("codex", !st.codex.installed)}>
          {st.codex.installed ? "撤销接入" : "一键接入"}
        </button>
      </div>
      {msg && <p className="result">{msg}</p>}
      <p className="muted small">
        Codex 的计划和进度从 ~/.codex/sessions 自动读取，不接入也能看到；接入 notify 后“一轮完成”会更及时。Codex 不会把“等待批准”写进会话记录，所以 Codex 会话只显示运行中 / 完成 / 出错。
      </p>
    </section>
  );
}

function Group({ snap, settings, save, notify }: { snap: Snapshot; settings: Settings; save: (p: Partial<Settings>) => void; notify: (m: string) => void }) {
  const [code, setCode] = useState("");
  const [joining, setJoining] = useState(false);
  const [layout, setLayout] = useState<string[]>(snap.group?.layout ?? []);
  const [dragId, setDragId] = useState<string | null>(null);
  useEffect(() => setLayout(snap.group?.layout ?? []), [snap.group?.layout]);
  const nameOf = (id: string) => snap.machines.find((m) => m.node_id === id)?.name ?? id.slice(0, 6);
  const others = snap.machines.filter((m) => !m.is_me);

  const join = async () => {
    setJoining(true);
    try {
      notify(await api.joinGroup(code));
      setCode("");
    } catch (e) {
      notify(String(e));
    } finally {
      setJoining(false);
    }
  };

  const move = (id: string, delta: number) => {
    const i = layout.indexOf(id);
    const j = i + delta;
    if (i < 0 || j < 0 || j >= layout.length) return;
    const next = [...layout];
    [next[i], next[j]] = [next[j], next[i]];
    setLayout(next);
    api.setLayout(next).catch((e) => notify(String(e)));
  };

  const dropOn = (target: string) => {
    if (!dragId || dragId === target) return;
    const next = layout.filter((x) => x !== dragId);
    next.splice(next.indexOf(target), 0, dragId);
    setLayout(next);
    setDragId(null);
    api.setLayout(next).catch((e) => notify(String(e)));
  };

  return (
    <section className="card">
      <h2>家庭组（局域网联机）</h2>
      {!snap.group ? (
        <>
          <p className="muted small">
            同一个家庭组里的电脑会同步 Agent 进度和日程，宠物还能互相串门。所有通信都经过加密；没配对的电脑什么都看不到。
            {snap.discovered_unpaired > 0 && ` 局域网里发现了 ${snap.discovered_unpaired} 台装了步步的电脑。`}
          </p>
          <div className="row">
            <button className="primary" onClick={() => api.createGroup().catch((e) => notify(String(e)))}>
              创建家庭组（第一台电脑）
            </button>
          </div>
          <h3>加入已有的家庭组</h3>
          <p className="muted small">先在已经入组的电脑上打开“设置 → 家庭组 → 添加新设备”，它会显示一个 6 位配对码，再在这里输入。</p>
          <div className="row">
            <input
              className="code-input"
              inputMode="numeric"
              maxLength={6}
              placeholder="6 位配对码"
              value={code}
              onChange={(e) => setCode(e.target.value.replace(/\D/g, ""))}
            />
            <button className="primary" disabled={code.length !== 6 || joining} onClick={join}>
              {joining ? "正在加入…" : "加入"}
            </button>
          </div>
        </>
      ) : (
        <>
          <div className="row">
            {snap.pairing ? (
              <div className="pairing" role="status">
                <div>在新电脑上输入配对码：</div>
                <div className="pair-code">{snap.pairing.code}</div>
                <div className="muted small">{Math.ceil(snap.pairing.expires_in_sec / 60)} 分钟内有效，输错 5 次自动作废</div>
                <button onClick={() => api.cancelPairing()}>取消</button>
              </div>
            ) : (
              <button className="primary" onClick={() => api.startPairing().catch((e) => notify(String(e)))}>
                添加新设备
              </button>
            )}
          </div>
          <h3>成员</h3>
          <ul className="members">
            {snap.machines.map((m) => (
              <li key={m.node_id}>
                <span className={`dot ${m.online ? "on" : "off"}`} aria-hidden />
                <span className="name">{m.name}</span>
                <span className="muted small">{m.owner_name}</span>
                {m.is_me && <span className="tag">本机</span>}
                <span className="spacer" />
                {!m.is_me && (
                  <>
                    <label className="toggle small">
                      <input type="checkbox" checked={m.i_share_content} onChange={(e) => api.setShareContent(m.node_id, e.target.checked)} />
                      <span>把消息正文和提示词同步给它</span>
                    </label>
                    <button
                      className="danger"
                      onClick={() => {
                        if (window.confirm(`把「${m.name}」移出家庭组？之后会更换组密钥，它将无法再看到任何同步内容。`)) {
                          api.kickMember(m.node_id).catch((e) => notify(String(e)));
                        }
                      }}
                    >
                      踢出
                    </button>
                  </>
                )}
              </li>
            ))}
          </ul>
          <p className="muted small">默认只同步 Agent 状态、计划条目和日程；消息正文、提示词默认不发给其他电脑，可以按电脑单独打开。</p>

          <h3>摆放位置</h3>
          <p className="muted small">像排列多块显示器一样，从左到右摆放各台电脑。宠物走出屏幕左 / 右边缘时，会出现在相邻那台电脑上。</p>
          <div className="layout-row" aria-label="电脑从左到右的排列">
            {layout.map((id, i) => (
              <div
                key={id}
                className={`layout-chip${id === snap.me.node_id ? " me" : ""}`}
                draggable
                onDragStart={() => setDragId(id)}
                onDragOver={(e) => e.preventDefault()}
                onDrop={() => dropOn(id)}
              >
                <button aria-label="左移" disabled={i === 0} onClick={() => move(id, -1)}>
                  ‹
                </button>
                <span>🖥 {id === snap.me.node_id ? `${nameOf(id)}（本机）` : nameOf(id)}</span>
                <button aria-label="右移" disabled={i === layout.length - 1} onClick={() => move(id, 1)}>
                  ›
                </button>
              </div>
            ))}
          </div>

          <h3>串门</h3>
          <Toggle label="允许串门（出门和接待别人的宠物）" checked={settings.allow_visits} onChange={(v) => save({ allow_visits: v })} />
          <Toggle label="宠物偶尔会自己溜达到隔壁电脑" checked={settings.wander_visits} onChange={(v) => save({ wander_visits: v })} />
          <label className="inline">
            来访的宠物待
            <input type="number" min={1} max={240} defaultValue={settings.visit_minutes} onBlur={(e) => save({ visit_minutes: Number(e.target.value) })} />
            分钟后回家
          </label>

          <div className="row">
            <button
              className="danger"
              onClick={() => window.confirm("退出家庭组？其他电脑会更换组密钥。") && api.leaveGroup().catch((e) => notify(String(e)))}
            >
              退出家庭组
            </button>
          </div>
        </>
      )}

      <h3>网络</h3>
      <p className="muted small">
        本机监听端口：{snap.net_port || "未启动"}。如果局域网屏蔽了 mDNS（比如某些路由器或 Windows 防火墙设置），可以手动填写其他电脑的地址，每行一个，例如 192.168.1.20:{settings.net_port}。
      </p>
      <textarea
        rows={2}
        defaultValue={settings.manual_peers.join("\n")}
        onBlur={(e) =>
          save({
            manual_peers: e.target.value
              .split(/\s+/)
              .map((x) => x.trim())
              .filter(Boolean),
          })
        }
      />
      {others.length > 0 && <p className="muted small">提示：踢出成员后会立即更换组密钥。</p>}
    </section>
  );
}
