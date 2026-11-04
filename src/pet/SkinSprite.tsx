// 用导入的精灵图（Codex 宠物格式）画宠物。逐帧动画完全由 CSS 驱动，不占 JS。
import type { CSSProperties } from "react";
import type { Anim, Side } from "../types";
import { HolidayOverlay, type ThemeAsset } from "../themes/holidays";
import { ANIM_LABEL, PetEffects } from "./PetSprite";
import { COLUMNS, FRAME_H, FRAME_W, STATES, stateFor, type LoadedSkin } from "./skin";

interface Props {
  skin: LoadedSkin;
  anim: Anim;
  facing: Side;
  /** 和默认形象的 size 含义相同：大致的显示高度。 */
  size?: number;
  day?: number;
  holiday?: string | null;
  holidayAsset?: ThemeAsset;
}

const INK = "#3b3b4a";

/** 显示比例：角色高度约为 size 的 0.85，同时宽度不超过 size 的 1.6 倍。 */
export function skinScale(skin: LoadedSkin, size: number): number {
  const b = skin.bounds;
  const w = Math.max(1, b.maxX - b.minX);
  const h = Math.max(1, b.maxY - b.minY);
  return Math.min((size * 0.85) / h, (size * 1.6) / w);
}

export function SkinSprite({ skin, anim, facing, size = 112, day, holiday, holidayAsset }: Props) {
  const k = skinScale(skin, size);
  const b = skin.bounds;
  const fw = FRAME_W * k;
  const fh = FRAME_H * k;
  const { state, slow } = stateFor(anim, facing);
  const { row, frames, ms } = STATES[state];
  const boxW = (b.maxX - b.minX) * k;
  const boxH = (b.maxY - b.minY) * k;
  const sheetStyle = {
    left: -b.minX * k,
    top: -b.minY * k,
    width: fw,
    height: fh,
    backgroundImage: `url(${skin.src})`,
    backgroundSize: `${COLUMNS * fw}px auto`,
    backgroundPositionY: `${-row * fh}px`,
    imageRendering: k >= 1 ? "pixelated" : "auto",
    "--frames": frames,
    "--fw": `${fw}px`,
    animation: `skin-frames ${((ms * slow) / 1000).toFixed(3)}s steps(${frames}) infinite`,
  } as CSSProperties;
  const overlay = (layer: "front" | "back", children: React.ReactNode) => (
    <svg className={`skin-overlay skin-overlay-${layer}`} width={boxW} height={boxH} viewBox="0 0 120 120" preserveAspectRatio="xMidYMid meet" aria-hidden>
      {children}
    </svg>
  );
  const today = day ?? new Date().getDate();
  return (
    <div
      className={`skin-sprite anim-${anim}`}
      data-anim={anim}
      data-state={state}
      role="img"
      aria-label={`${skin.name}：${ANIM_LABEL[anim]}`}
      style={{ width: boxW, height: boxH }}
    >
      {holiday && holidayAsset?.layer === "back" && overlay("back", <HolidayOverlay id={holiday} asset={holidayAsset} />)}
      {/* key 让换动作时动画从第一帧开始。 */}
      <div key={state} className="skin-sheet" style={sheetStyle} />
      {overlay(
        "front",
        <>
          {holiday && holidayAsset?.layer !== "back" && <HolidayOverlay id={holiday} asset={holidayAsset} />}
          <PetEffects anim={anim} line={INK} />
          {anim === "calendar" && (
            <g className="calendar-card">
              <rect x="86" y="2" width="30" height="26" rx="4" fill="#fff" stroke={INK} strokeWidth="2" />
              <rect x="86" y="2" width="30" height="8" rx="4" fill="#e85d5d" />
              <rect x="86" y="6" width="30" height="4" fill="#e85d5d" />
              <text x="101" y="25" textAnchor="middle" fontSize="13" fontWeight="700" fill={INK}>
                {today}
              </text>
            </g>
          )}
        </>,
      )}
    </div>
  );
}
