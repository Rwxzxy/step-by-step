// 步步：原创的圆滚滚小团子，头顶一棵小芽。全部用 SVG 画，动作由 CSS 动画驱动。
import type { Anim, Side } from "../types";
import { HolidayOverlay, type ThemeAsset } from "../themes/holidays";

interface Props {
  anim: Anim;
  facing: Side;
  hue: number;
  size?: number;
  /** 日历上显示的日期（看日历动作）。 */
  day?: number;
  /** 节日主题 id（没有节日时为空）。 */
  holiday?: string | null;
  /** 节日的正式美术资源（manifest 里登记的），没有时用占位图形。 */
  holidayAsset?: ThemeAsset;
}

type Eyes = "open" | "closed" | "happy" | "wide";

function eyesFor(anim: Anim): Eyes {
  switch (anim) {
    case "sleep":
      return "closed";
    case "happy":
      return "happy";
    case "drag":
    case "fall":
      return "wide";
    default:
      return "open";
  }
}

export function PetSprite({ anim, facing, hue, size = 112, day, holiday, holidayAsset }: Props) {
  const body = `hsl(${hue} 70% 86%)`;
  const belly = `hsl(${hue} 75% 93%)`;
  const line = `hsl(${hue} 40% 32%)`;
  const cheek = `hsl(${(hue + 340) % 360} 80% 78%)`;
  const leaf = "hsl(112 45% 48%)";
  const eyes = eyesFor(anim);
  const lookDown = anim === "calendar";
  const today = day ?? new Date().getDate();

  return (
    <svg
      className={`pet-svg anim-${anim} facing-${facing}`}
      data-anim={anim}
      width={size}
      height={size}
      viewBox="0 0 120 120"
      role="img"
      aria-label={`步步：${ANIM_LABEL[anim]}`}
    >
      <ellipse className="pet-shadow" cx="60" cy="113" rx="30" ry="4" fill="rgba(0,0,0,0.16)" />
      <g className="pet-flip">
        {holiday && holidayAsset?.layer === "back" && <HolidayOverlay id={holiday} asset={holidayAsset} />}
        <g className="pet-feet">
          <ellipse className="foot foot-l" cx="46" cy="105" rx="9" ry="6" fill={line} />
          <ellipse className="foot foot-r" cx="74" cy="105" rx="9" ry="6" fill={line} />
        </g>
        <g className="pet-body">
          <Arms anim={anim} body={body} line={line} />
          <g className="sprout">
            <path d="M60 38 C60 31 61 27 62 23" stroke={leaf} strokeWidth="3" fill="none" strokeLinecap="round" />
            <path d="M61.5 25 C54 16 44 19 45 26 C51 29 57 28 61.5 25 Z" fill={leaf} />
            <path d="M62.5 24 C68 14 79 15 78 22 C73 26 67 26 62.5 24 Z" fill={leaf} />
          </g>
          <ellipse cx="60" cy="72" rx="40" ry="36" fill={body} stroke={line} strokeWidth="3" />
          <ellipse cx="60" cy="86" rx="22" ry="15" fill={belly} />
          <g className="face" transform={lookDown ? "translate(0 4)" : undefined}>
            <ellipse cx="40" cy="78" rx="7" ry="4" fill={cheek} opacity="0.85" />
            <ellipse cx="80" cy="78" rx="7" ry="4" fill={cheek} opacity="0.85" />
            {eyes === "open" && (
              <g className="eyes blink">
                <ellipse cx="47" cy="66" rx="4.5" ry="5.5" fill={line} />
                <ellipse cx="73" cy="66" rx="4.5" ry="5.5" fill={line} />
                <circle cx="48.5" cy="64" r="1.6" fill="#fff" />
                <circle cx="74.5" cy="64" r="1.6" fill="#fff" />
              </g>
            )}
            {eyes === "wide" && (
              <g className="eyes">
                <circle cx="47" cy="65" r="6.5" fill="#fff" stroke={line} strokeWidth="2.5" />
                <circle cx="73" cy="65" r="6.5" fill="#fff" stroke={line} strokeWidth="2.5" />
                <circle cx="47" cy="66" r="3" fill={line} />
                <circle cx="73" cy="66" r="3" fill={line} />
              </g>
            )}
            {eyes === "closed" && (
              <g className="eyes" stroke={line} strokeWidth="3" fill="none" strokeLinecap="round">
                <path d="M41 67 Q47 71 53 67" />
                <path d="M67 67 Q73 71 79 67" />
              </g>
            )}
            {eyes === "happy" && (
              <g className="eyes" stroke={line} strokeWidth="3" fill="none" strokeLinecap="round">
                <path d="M41 68 Q47 60 53 68" />
                <path d="M67 68 Q73 60 79 68" />
              </g>
            )}
            {anim === "anxious" && (
              <g className="brows" stroke={line} strokeWidth="2.5" strokeLinecap="round">
                <path d="M40 57 L52 60" />
                <path d="M80 57 L68 60" />
              </g>
            )}
            <Mouth anim={anim} line={line} />
          </g>
        </g>
        {anim === "calendar" && (
          <g className="calendar-card">
            <rect x="42" y="78" width="36" height="30" rx="4" fill="#fff" stroke={line} strokeWidth="2.5" />
            <rect x="42" y="78" width="36" height="9" rx="4" fill="#e85d5d" />
            <rect x="42" y="83" width="36" height="4" fill="#e85d5d" />
            <circle cx="50" cy="78" r="2" fill={line} />
            <circle cx="70" cy="78" r="2" fill={line} />
            <text x="60" y="104" textAnchor="middle" fontSize="15" fontWeight="700" fill={line}>
              {today}
            </text>
            {/* 两只小手扶着日历 */}
            <ellipse cx="42" cy="95" rx="6" ry="7.5" fill={body} stroke={line} strokeWidth="2.5" />
            <ellipse cx="78" cy="95" rx="6" ry="7.5" fill={body} stroke={line} strokeWidth="2.5" />
          </g>
        )}
        {holiday && holidayAsset?.layer !== "back" && <HolidayOverlay id={holiday} asset={holidayAsset} />}
      </g>
      <PetEffects anim={anim} line={line} />
    </svg>
  );
}

/** 各动作的附加小图标（Zzz、星星、感叹号、汗滴），画在 120 × 120 的坐标系里。导入的形象也用它。 */
export function PetEffects({ anim, line }: { anim: Anim; line: string }) {
  return (
    <>
      {anim === "sleep" && (
        <g className="zzz" fill={line} fontWeight="700">
          <text x="88" y="34" fontSize="12">z</text>
          <text x="97" y="22" fontSize="15">z</text>
          <text x="107" y="10" fontSize="18">Z</text>
        </g>
      )}
      {anim === "happy" && (
        <g className="sparkles" fill="#ffc83d">
          <path d="M14 30 l3 7 7 3 -7 3 -3 7 -3 -7 -7 -3 7 -3 Z" />
          <path d="M100 26 l2.5 5.5 5.5 2.5 -5.5 2.5 -2.5 5.5 -2.5 -5.5 -5.5 -2.5 5.5 -2.5 Z" />
          <path d="M106 64 l2 4 4 2 -4 2 -2 4 -2 -4 -4 -2 4 -2 Z" />
        </g>
      )}
      {anim === "raise_hand" && (
        <g className="exclaim">
          <circle cx="60" cy="9" r="9" fill="#ffb02e" stroke={line} strokeWidth="2" />
          <rect x="58.4" y="3" width="3.2" height="8" rx="1.5" fill="#fff" />
          <circle cx="60" cy="14" r="1.8" fill="#fff" />
        </g>
      )}
      {anim === "anxious" && (
        <g className="sweat" fill="#7cc4f2" stroke={line} strokeWidth="1.5">
          <path d="M96 40 C92 47 93 52 97 52 C101 52 101 47 96 40 Z" />
          <path d="M22 46 C19 51 20 55 23 55 C26 55 26 51 22 46 Z" />
        </g>
      )}
    </>
  );
}

function Arms({ anim, body, line }: { anim: Anim; body: string; line: string }) {
  // 手臂的摆动用 SVG 自带的 animateTransform，以肩膀为支点，避免各家 WebView 对 CSS transform-origin 的差异。
  const arm = (cx: number, cy: number, rot: number, cls: string, ry = 10, swing = 0, dur = "0.8s") => {
    // 举起的手臂肩膀在下端，垂下的手臂肩膀在上端。
    const raised = Math.abs(rot) > 0 || cy < 60;
    const rad = (rot * Math.PI) / 180;
    const k = raised ? 1 : -1;
    const px = +(cx - k * ry * Math.sin(rad)).toFixed(1);
    const py = +(cy + k * ry * Math.cos(rad)).toFixed(1);
    return (
      <g className={`arm ${cls}`}>
        <ellipse cx={cx} cy={cy} rx="7" ry={ry} transform={`rotate(${rot} ${cx} ${cy})`} fill={body} stroke={line} strokeWidth="3" />
        {swing !== 0 && (
          <animateTransform
            attributeName="transform"
            type="rotate"
            values={`${-swing} ${px} ${py}; ${swing} ${px} ${py}; ${-swing} ${px} ${py}`}
            dur={dur}
            repeatCount="indefinite"
          />
        )}
      </g>
    );
  };
  switch (anim) {
    case "happy":
    case "fall":
      // 双手举高
      return (
        <>
          {arm(26, 52, -35, "arm-l arm-up", 12, 8, "0.7s")}
          {arm(94, 52, 35, "arm-r arm-up", 12, 8, "0.7s")}
        </>
      );
    case "raise_hand":
      // 右手高高举起挥动
      return (
        <>
          {arm(22, 80, 0, "arm-l")}
          {arm(102, 48, 20, "arm-r arm-wave", 13, 15)}
        </>
      );
    case "calendar":
      // 手画在日历两侧
      return null;
    case "walk":
      // 走路时前后摆臂（以肩膀为支点，左右相位相反）
      return (
        <>
          {arm(22, 80, 0, "arm-l", 10, 14, "0.5s")}
          {arm(98, 80, 0, "arm-r", 10, -14, "0.5s")}
        </>
      );
    default:
      return (
        <>
          {arm(22, 80, 0, "arm-l")}
          {arm(98, 80, 0, "arm-r")}
        </>
      );
  }
}

function Mouth({ anim, line }: { anim: Anim; line: string }) {
  switch (anim) {
    case "happy":
      return <path d="M51 79 Q60 92 69 79 Z" fill={line} />;
    case "anxious":
      return <path d="M52 84 q4 -4 8 0 t8 0" stroke={line} strokeWidth="2.5" fill="none" strokeLinecap="round" />;
    case "drag":
    case "fall":
      return <ellipse cx="60" cy="83" rx="4" ry="5" fill={line} />;
    case "sleep":
      return <path d="M56 83 L64 83" stroke={line} strokeWidth="2.5" strokeLinecap="round" />;
    case "raise_hand":
      return <path d="M54 80 Q60 87 66 80" stroke={line} strokeWidth="2.5" fill="none" strokeLinecap="round" />;
    default:
      return <path d="M55 81 Q60 85 65 81" stroke={line} strokeWidth="2.5" fill="none" strokeLinecap="round" />;
  }
}

export const ANIM_LABEL: Record<Anim, string> = {
  idle: "待机",
  walk: "走路",
  sleep: "睡觉",
  happy: "开心",
  raise_hand: "举手",
  anxious: "着急",
  calendar: "看日历",
  drag: "被拎起来",
  fall: "下落",
};
