// 在普通浏览器里（npm run dev）打开时显示全部动作，方便预览和调整形象。
import { useState } from "react";
import type { Anim } from "../types";
import { ANIM_LABEL, PetSprite } from "./PetSprite";
import { HOLIDAY_IDS, useManifest } from "../themes/holidays";
import { BLOCKS, BLOCK_LABEL, builtinTexture } from "../world/textures";
import { useEffect, useRef } from "react";

function BlockPreview({ id }: { id: (typeof BLOCKS)[number] }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const ctx = ref.current?.getContext("2d");
    const tex = builtinTexture(id);
    if (ctx && tex) {
      ctx.imageSmoothingEnabled = false;
      ctx.drawImage(tex, 0, 0, 48, 48);
    }
  }, [id]);
  return <canvas ref={ref} width={48} height={48} style={{ imageRendering: "pixelated" }} />;
}

const HOLIDAY_NAME: Record<string, string> = {
  spring_festival: "春节",
  lantern: "元宵",
  qingming: "清明",
  dragon_boat: "端午",
  qixi: "七夕",
  mid_autumn: "中秋",
  double_ninth: "重阳",
  new_year: "元旦",
  valentine: "情人节",
  labor_day: "劳动节",
  children: "儿童节",
  national_day: "国庆",
  halloween: "万圣节",
  christmas: "圣诞",
  custom: "纪念日",
};
import "./pet.css";

export function SpriteGallery() {
  const [hue, setHue] = useState(32);
  const manifest = useManifest();
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
      <h2 style={{ margin: "24px 0 4px" }}>方块（自带像素纹理）</h2>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 12 }}>
        {BLOCKS.map((b) => (
          <figure key={b} style={{ margin: 0, textAlign: "center", background: "#fff", borderRadius: 10, padding: 8 }}>
            <BlockPreview id={b} />
            <figcaption style={{ fontSize: 12 }}>{BLOCK_LABEL[b]}</figcaption>
          </figure>
        ))}
      </div>
      <h2 style={{ margin: "24px 0 4px" }}>节日主题</h2>
      <p style={{ color: "#857a72" }}>没有正式美术资源的节日显示占位图形（见 docs/themes.md）。</p>
      <div style={{ display: "flex", flexWrap: "wrap", gap: 16 }}>
        {HOLIDAY_IDS.map((id) => (
          <figure key={id} style={{ margin: 0, textAlign: "center", background: "#fff", borderRadius: 12, padding: 12 }}>
            <PetSprite anim="idle" facing="right" hue={hue} size={112} holiday={id} holidayAsset={manifest.themes[id]} />
            <figcaption>{HOLIDAY_NAME[id]}</figcaption>
          </figure>
        ))}
      </div>
    </div>
  );
}
