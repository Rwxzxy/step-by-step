// 面板第三块：局域网里的其他机器、宠物在哪、来访的宠物。
import { api } from "../api";
import type { Snapshot } from "../types";

export function MachinesSection({ snap, onOpenSettings }: { snap: Snapshot; onOpenSettings: () => void }) {
  const others = snap.machines.filter((m) => !m.is_me);
  const pet = snap.pet;
  const nameOf = (id: string | null) => snap.machines.find((m) => m.node_id === id)?.name ?? "";
  const online = (id: string | null) => !!id && !!snap.machines.find((m) => m.node_id === id && m.online);

  return (
    <section className="card" aria-labelledby="machines-title">
      <header className="card-head">
        <h2 id="machines-title">局域网里的机器</h2>
        <button className="link" onClick={onOpenSettings}>
          家庭组设置 →
        </button>
      </header>

      <div className="pet-where" data-testid="pet-where">
        {pet.location === "home" && <span>我的步步在家。</span>}
        {pet.location === "leaving" && <span>步步正在去「{pet.away_at_name}」的路上…</span>}
        {pet.location === "away" && (
          <>
            <span>我的步步在「{pet.away_at_name}」串门。</span>
            <button onClick={() => api.petRecall()}>召回</button>
          </>
        )}
        {pet.location === "returning" && <span>和「{pet.away_at_name}」失去联系，步步正在回家…</span>}
        {pet.location === "home" && (
          <span className="visit-buttons">
            {online(pet.left_neighbor) && <button onClick={() => api.petGoVisit("left")}>← 去「{nameOf(pet.left_neighbor)}」串门</button>}
            {online(pet.right_neighbor) && <button onClick={() => api.petGoVisit("right")}>去「{nameOf(pet.right_neighbor)}」串门 →</button>}
          </span>
        )}
      </div>

      {pet.guests.length > 0 && (
        <ul className="guests">
          {pet.guests.map((g) => (
            <li key={g.owner}>
              {g.owner_name}的步步（来自「{g.machine_name}」）正在这里玩
              <button onClick={() => api.petSendGuestHome(g.owner)}>送它回家</button>
            </li>
          ))}
        </ul>
      )}

      {!snap.group ? (
        <p className="empty">
          还没有家庭组。{snap.discovered_unpaired > 0 && `局域网里发现了 ${snap.discovered_unpaired} 台装了步步的电脑。`}
          <button className="link" onClick={onOpenSettings}>
            创建或加入家庭组
          </button>
        </p>
      ) : others.length === 0 ? (
        <p className="empty">家庭组里只有这台电脑。在设置里点“添加新设备”，把其他电脑加进来。</p>
      ) : (
        <ul className="machines">
          {others.map((m) => {
            const running = m.sessions.filter((s) => s.status === "running").length;
            const waiting = m.sessions.filter((s) => s.status === "waiting").length;
            return (
              <li key={m.node_id} data-testid="machine-row">
                <span className={`dot ${m.online ? "on" : "off"}`} aria-hidden />
                <span className="name">{m.name}</span>
                <span className="muted small">{m.owner_name}</span>
                <span className="spacer" />
                <span className="small">{m.online ? "在线" : "离线"}</span>
                {m.online && (
                  <span className="small muted">
                    {running} 个运行中{waiting > 0 && `，${waiting} 个在等`}
                  </span>
                )}
              </li>
            );
          })}
        </ul>
      )}
    </section>
  );
}
