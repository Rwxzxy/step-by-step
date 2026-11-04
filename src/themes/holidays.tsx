// 节日主题：每个节日一个“叠加层”，画在宠物身上 / 旁边。
//
// 现在用的是程序画的简单占位图形。正式美术资源到位后，不用改代码：
// 把图片放到 public/themes/<节日 id>/，并在 public/themes/manifest.json 里登记即可（见 docs/themes.md）。
// 登记了图片的节日用图片，没登记的继续用这里的占位图形。
import { useEffect, useState } from "react";

export const HOLIDAY_IDS = [
  "spring_festival",
  "lantern",
  "qingming",
  "dragon_boat",
  "qixi",
  "mid_autumn",
  "double_ninth",
  "new_year",
  "valentine",
  "labor_day",
  "children",
  "national_day",
  "halloween",
  "christmas",
  "custom",
] as const;

/** 气泡边框等强调色（美术资源可以在 manifest 里覆盖）。 */
export const DEFAULT_ACCENT: Record<string, string> = {
  spring_festival: "#d8342a",
  lantern: "#e0562d",
  qingming: "#4f9a5c",
  dragon_boat: "#3f8a3a",
  qixi: "#d9578f",
  mid_autumn: "#d9a43a",
  double_ninth: "#d6a417",
  new_year: "#6c5bd4",
  valentine: "#e2507a",
  labor_day: "#2f7fc9",
  children: "#f08a24",
  national_day: "#d8342a",
  halloween: "#e0761f",
  christmas: "#2f8a4a",
  custom: "#c76bd6",
};

export interface ThemeAsset {
  /** 相对 public/themes/ 的图片路径（SVG 或 PNG，按 120×120 的宠物坐标系绘制）。 */
  overlay?: string;
  /** 叠加在宠物身前（front，默认）还是身后（back）。 */
  layer?: "front" | "back";
  accent?: string;
}

export interface ThemeManifest {
  version: number;
  themes: Record<string, ThemeAsset>;
}

let manifestPromise: Promise<ThemeManifest> | null = null;

/** 读取 public/themes/manifest.json；文件不存在或格式不对时当作空清单。 */
export function loadManifest(): Promise<ThemeManifest> {
  if (!manifestPromise) {
    manifestPromise = fetch("/themes/manifest.json")
      .then((r) => (r.ok ? r.json() : { version: 1, themes: {} }))
      .then((m: ThemeManifest) => (m && typeof m.themes === "object" ? m : { version: 1, themes: {} }))
      .catch(() => ({ version: 1, themes: {} }));
  }
  return manifestPromise;
}

export function useManifest(): ThemeManifest {
  const [m, setM] = useState<ThemeManifest>({ version: 1, themes: {} });
  useEffect(() => {
    let alive = true;
    loadManifest().then((x) => alive && setM(x));
    return () => {
      alive = false;
    };
  }, []);
  return m;
}

export function accentOf(id: string | null | undefined, manifest?: ThemeManifest): string | undefined {
  if (!id) return undefined;
  return manifest?.themes[id]?.accent ?? DEFAULT_ACCENT[id];
}

/** 节日叠加层：有正式资源时用图片，否则用占位图形。坐标系与宠物 SVG 相同（120×120）。 */
export function HolidayOverlay({ id, asset }: { id: string; asset?: ThemeAsset }) {
  if (asset?.overlay) {
    return (
      <g className="holiday-overlay" data-holiday={id} data-source="asset">
        <image href={`/themes/${asset.overlay}`} x="0" y="0" width="120" height="120" />
      </g>
    );
  }
  return (
    <g className="holiday-overlay" data-holiday={id} data-source="placeholder">
      {PLACEHOLDERS[id] ?? null}
    </g>
  );
}

const ink = "#4a3f3a";

const heart = (x: number, y: number, s: number, fill: string) => (
  <path
    d={`M${x} ${y + s * 0.9} C${x - s * 1.6} ${y - s * 0.2} ${x - s * 0.6} ${y - s * 1.3} ${x} ${y - s * 0.3} C${x + s * 0.6} ${y - s * 1.3} ${x + s * 1.6} ${y - s * 0.2} ${x} ${y + s * 0.9} Z`}
    fill={fill}
  />
);

const lantern = (cx: number, cy: number, r: number) => (
  <g>
    <line x1={cx} y1={cy - r * 2.2} x2={cx} y2={cy - r} stroke={ink} strokeWidth="1.5" />
    <rect x={cx - r * 0.6} y={cy - r * 1.25} width={r * 1.2} height={r * 0.35} fill="#f2c14e" />
    <ellipse cx={cx} cy={cy} rx={r} ry={r * 1.05} fill="#e23b2e" stroke={ink} strokeWidth="1.5" />
    <line x1={cx} y1={cy - r} x2={cx} y2={cy + r} stroke="#b12a20" strokeWidth="1" />
    <rect x={cx - r * 0.6} y={cy + r * 0.9} width={r * 1.2} height={r * 0.35} fill="#f2c14e" />
    <line x1={cx} y1={cy + r * 1.25} x2={cx} y2={cy + r * 2} stroke="#f2c14e" strokeWidth="2" />
  </g>
);

const burst = (cx: number, cy: number, color: string) => (
  <g stroke={color} strokeWidth="2" strokeLinecap="round">
    {Array.from({ length: 8 }, (_, i) => {
      const a = (i * Math.PI) / 4;
      return <line key={i} x1={cx + Math.cos(a) * 3} y1={cy + Math.sin(a) * 3} x2={cx + Math.cos(a) * 9} y2={cy + Math.sin(a) * 9} />;
    })}
  </g>
);

const balloon = (cx: number, cy: number, fill: string) => (
  <g>
    <path d={`M${cx} ${cy + 11} Q${cx - 6} ${cy + 30} ${cx - 4} ${cy + 52}`} stroke={ink} strokeWidth="1.2" fill="none" />
    <ellipse cx={cx} cy={cy} rx="8" ry="10" fill={fill} stroke={ink} strokeWidth="1.5" />
    <path d={`M${cx - 2} ${cy + 10} L${cx + 2} ${cy + 10} L${cx} ${cy + 13} Z`} fill={fill} />
  </g>
);

/** 占位图形：只用简单几何形状，正式美术资源到位后会被替换。 */
const PLACEHOLDERS: Record<string, JSX.Element> = {
  // 春节：右上方挂一盏红灯笼
  spring_festival: lantern(100, 16, 8),
  // 元宵：两盏小灯笼
  lantern: (
    <g>
      {lantern(14, 18, 6)}
      {lantern(106, 12, 6)}
    </g>
  ),
  // 清明：一枝柳条
  qingming: (
    <g>
      <path d="M98 44 Q104 24 112 6" stroke="#6b8f3a" strokeWidth="2" fill="none" />
      {[0, 1, 2, 3].map((i) => (
        <ellipse key={i} cx={101 + i * 3} cy={36 - i * 9} rx="5" ry="2" fill="#7fbf4a" transform={`rotate(-40 ${101 + i * 3} ${36 - i * 9})`} />
      ))}
    </g>
  ),
  // 端午：右手边一个粽子
  dragon_boat: (
    <g>
      <path d="M98 74 L116 74 L107 56 Z" fill="#4f9a3c" stroke={ink} strokeWidth="1.5" strokeLinejoin="round" />
      <line x1="101" y1="68" x2="113" y2="68" stroke="#e6d39a" strokeWidth="2" />
    </g>
  ),
  // 七夕：爱心和星星
  qixi: (
    <g>
      {heart(104, 20, 6, "#ff7aa8")}
      <path d="M16 8 l2 5 5 1 -4 3 1 5 -4 -3 -4 3 1 -5 -4 -3 5 -1 Z" fill="#ffd45c" />
    </g>
  ),
  // 中秋：左上一轮圆月，右手一块月饼
  mid_autumn: (
    <g>
      <circle cx="16" cy="12" r="10" fill="#ffd45c" />
      <circle cx="104" cy="78" r="9" fill="#d99a45" stroke={ink} strokeWidth="1.5" />
      <circle cx="104" cy="78" r="5" fill="none" stroke="#b07a2e" strokeWidth="1.2" />
    </g>
  ),
  // 重阳：头上一朵菊花
  double_ninth: (
    <g>
      {Array.from({ length: 8 }, (_, i) => (
        <ellipse key={i} cx="44" cy="30" rx="2.2" ry="6" fill="#f5c518" transform={`rotate(${i * 45} 44 36) translate(0 0)`} />
      ))}
      <circle cx="44" cy="36" r="3" fill="#e08e0b" />
    </g>
  ),
  // 元旦：两朵烟花
  new_year: (
    <g>
      {burst(14, 14, "#6c5bd4")}
      {burst(106, 8, "#f08a24")}
    </g>
  ),
  // 情人节：两颗心
  valentine: (
    <g>
      {heart(104, 22, 6, "#e2507a")}
      {heart(14, 30, 4, "#ff8fb0")}
    </g>
  ),
  // 劳动节：右手边一把小锤子
  labor_day: (
    <g>
      <rect x="104" y="58" width="3" height="20" rx="1" fill="#a0683a" stroke={ink} strokeWidth="1" transform="rotate(20 105 68)" />
      <rect x="100" y="54" width="14" height="6" rx="1" fill="#8a9aa8" stroke={ink} strokeWidth="1" transform="rotate(20 105 68)" />
    </g>
  ),
  // 儿童节：一个气球
  children: balloon(106, 12, "#ff6b8a"),
  // 国庆：一红一金两个气球
  national_day: (
    <g>
      {balloon(102, 14, "#e23b2e")}
      {balloon(114, 8, "#f2c14e")}
    </g>
  ),
  // 万圣节：脚边一个南瓜
  halloween: (
    <g>
      <ellipse cx="12" cy="100" rx="11" ry="8" fill="#f08a24" stroke={ink} strokeWidth="1.5" />
      <rect x="11" y="89" width="3" height="5" fill="#5a7a2e" />
      <path d="M7 98 l2 -3 2 3 Z M15 98 l2 -3 2 3 Z M8 103 Q12 106 17 103" fill={ink} stroke={ink} strokeWidth="1" />
    </g>
  ),
  // 圣诞：歪戴的红帽子和雪花
  christmas: (
    <g>
      <path d="M68 42 Q86 16 100 30 L94 44 Z" fill="#d8342a" stroke={ink} strokeWidth="1.5" strokeLinejoin="round" />
      <rect x="66" y="40" width="30" height="6" rx="3" fill="#fff" stroke={ink} strokeWidth="1.2" transform="rotate(12 81 43)" />
      <circle cx="100" cy="30" r="4" fill="#fff" stroke={ink} strokeWidth="1.2" />
      <g stroke="#9cc9ef" strokeWidth="1.5" strokeLinecap="round">
        <line x1="14" y1="6" x2="14" y2="20" />
        <line x1="8" y1="10" x2="20" y2="16" />
        <line x1="8" y1="16" x2="20" y2="10" />
      </g>
    </g>
  ),
  // 自定义纪念日：一个小蛋糕
  custom: (
    <g>
      <rect x="96" y="66" width="18" height="12" rx="2" fill="#ffc6d6" stroke={ink} strokeWidth="1.5" />
      <rect x="96" y="66" width="18" height="4" rx="2" fill="#fff" />
      <rect x="104" y="57" width="2.5" height="9" fill="#7fb3e6" />
      <ellipse cx="105.2" cy="55" rx="2" ry="3" fill="#ffb02e" />
    </g>
  ),
};
