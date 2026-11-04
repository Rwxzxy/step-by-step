// 设置 → 宠物形象：默认的步步，或者导入的 Codex 宠物格式像素形象。
import { useCallback, useEffect, useState } from "react";
import { api } from "../api";
import type { Anim, FoundPet, SkinInfo } from "../types";
import { ANIM_LABEL, PetSprite } from "../pet/PetSprite";
import { SkinSprite } from "../pet/SkinSprite";
import { useSkin, useSkinVersion } from "../pet/skin";

const PREVIEW_ANIMS: Anim[] = ["idle", "walk", "happy", "raise_hand", "anxious", "calendar", "sleep"];

function SkinPreview({ id, anim = "idle", size = 56, version }: { id: string; anim?: Anim; size?: number; version: number }) {
  const skin = useSkin(id, version);
  if (!skin) return <div className="skin-loading" style={{ width: size, height: size }} />;
  return <SkinSprite skin={skin} anim={anim} facing="right" size={size} />;
}

export function SkinSection({ current, hue, notify }: { current: string | null; hue: number; notify: (m: string) => void }) {
  const [skins, setSkins] = useState<SkinInfo[]>([]);
  const [found, setFound] = useState<FoundPet[]>([]);
  const [selected, setSelected] = useState<string | null>(current);
  const version = useSkinVersion();

  const load = useCallback(() => {
    api.listSkins().then(setSkins).catch(() => undefined);
    api.discoverSkins().then(setFound).catch(() => undefined);
  }, []);
  useEffect(load, [load, version]);
  useEffect(() => setSelected(current), [current]);

  const choose = (id: string | null) => {
    setSelected(id);
    api.setPetSkin(id).catch((e) => notify(String(e)));
  };
  const imported = (info: SkinInfo | null) => {
    if (!info) return;
    setSelected(info.id);
    notify(`已导入并换上「${info.name}」`);
    load();
  };
  const notYet = found.filter((f) => !f.imported);

  return (
    <section className="card" aria-labelledby="skin-title">
      <h2 id="skin-title">宠物形象</h2>
      <p className="muted small">
        可以换成 Codex 宠物格式的像素形象（比如 Petdex 网站上的宠物，或者按同样格式自己画的）。导入的图片只保存在这台电脑上，不会同步、不会上传；宠物去别人电脑串门时，对方看到的仍是默认的步步。
      </p>
      <div className="skin-grid" role="radiogroup" aria-label="宠物形象">
        <button className={`skin-card${selected === null ? " selected" : ""}`} role="radio" aria-checked={selected === null} onClick={() => choose(null)}>
          <PetSprite anim="idle" facing="right" hue={hue} size={56} />
          <span>步步（默认）</span>
        </button>
        {skins.map((s) => (
          <div key={s.id} className={`skin-card${selected === s.id ? " selected" : ""}`}>
            <button className="skin-pick" role="radio" aria-checked={selected === s.id} title={s.description ?? s.name} onClick={() => choose(s.id)}>
              <SkinPreview id={s.id} version={version} />
              <span>{s.name}</span>
            </button>
            <button
              className="skin-delete"
              aria-label={`删除${s.name}`}
              title="删除"
              onClick={() =>
                window.confirm(`删除形象「${s.name}」？`) &&
                api
                  .deleteSkin(s.id)
                  .then(() => {
                    if (selected === s.id) setSelected(null);
                    load();
                  })
                  .catch((e) => notify(String(e)))
              }
            >
              ×
            </button>
          </div>
        ))}
      </div>
      <div className="row">
        <button onClick={() => api.importSkin().then(imported).catch((e) => notify(String(e)))}>从文件夹导入…</button>
        <span className="muted small">文件夹里要有 spritesheet.webp 或 spritesheet.png（压缩包请先解压）。</span>
      </div>
      {notYet.length > 0 && (
        <>
          <h3>在本机找到的 Codex / Petdex 宠物</h3>
          <ul className="found-pets">
            {notYet.map((f) => (
              <li key={f.path}>
                <span title={f.path}>{f.name}</span>
                <button onClick={() => api.importSkinFrom(f.path).then(imported).catch((e) => notify(String(e)))}>导入并换上</button>
              </li>
            ))}
          </ul>
        </>
      )}
      {selected && skins.some((s) => s.id === selected) && (
        <>
          <h3>动作预览</h3>
          <div className="skin-actions">
            {PREVIEW_ANIMS.map((a) => (
              <figure key={a}>
                <SkinPreview id={selected} anim={a} size={48} version={version} />
                <figcaption>{ANIM_LABEL[a]}</figcaption>
              </figure>
            ))}
          </div>
        </>
      )}
    </section>
  );
}
