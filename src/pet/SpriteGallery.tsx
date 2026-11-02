// 在普通浏览器里（npm run dev）打开时显示全部动作，方便预览和调整形象。
import { useState } from "react";
import type { Anim } from "../types";
import { ANIM_LABEL, PetSprite } from "./PetSprite";
import "./pet.css";

export function SpriteGallery() {
  const [hue, setHue] = useState(32);
  return (
    <div style={{ padding: 24, fontFamily: "system-ui, sans-serif", background: "#f7f3ee", minHeight: "100vh" }}>
      <h1 style={{ margin: "0 0 4px" }}>步步 Steppy · 动作预览</h1>
      <p style={{ color: "#857a72" }}>这是在浏览器里打开时的预览页；在应用里，宠物会出现在屏幕底部。</p>
      <label>
        颜色 <input type="range" min={0} max={359} value={hue} onChange={(e) => setHue(Number(e.target.value))} />
      </label>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 16, marginTop: 16 }}>
        {(Object.keys(ANIM_LABEL) as Anim[]).map((a) => (
          <figure key={a} style={{ margin: 0, textAlign: "center", background: "#fff", borderRadius: 12, padding: 12 }}>
            <PetSprite anim={a} facing="right" hue={hue} size={128} day={2} />
            <figcaption>{ANIM_LABEL[a]}</figcaption>
          </figure>
        ))}
        <figure style={{ margin: 0, textAlign: "center", background: "#fff", borderRadius: 12, padding: 12 }}>
          <PetSprite anim="walk" facing="left" hue={hue} size={128} />
          <figcaption>朝左走</figcaption>
        </figure>
      </div>
    </div>
  );
}
