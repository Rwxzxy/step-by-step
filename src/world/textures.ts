// 方块的内置像素纹理：16×16，全部由程序按固定种子生成（原创，不包含任何外部美术资源）。
// 用户也可以在设置里从自己电脑上的材质包导入纹理，导入的图片只保存在本机。

export const BLOCKS = [
  "grass",
  "dirt",
  "stone",
  "cobble",
  "planks",
  "log",
  "bricks",
  "glass",
  "sand",
  "leaves",
  "bookshelf",
  "lamp",
  "wool",
  "flower",
  "bed",
  "workbench",
] as const;

export type BlockId = (typeof BLOCKS)[number];

export const BLOCK_LABEL: Record<BlockId, string> = {
  grass: "草地",
  dirt: "泥土",
  stone: "石头",
  cobble: "碎石",
  planks: "木板",
  log: "原木",
  bricks: "砖块",
  glass: "玻璃",
  sand: "沙子",
  leaves: "树叶",
  bookshelf: "书架",
  lamp: "台灯",
  wool: "羊毛",
  flower: "花",
  bed: "床",
  workbench: "工作台",
};

const N = 16;

/** 确定性的伪随机数（同一种方块每次生成的纹理都一样）。 */
function rng(seed: number) {
  let s = seed >>> 0 || 1;
  return () => {
    s ^= s << 13;
    s ^= s >>> 17;
    s ^= s << 5;
    return ((s >>> 0) % 10000) / 10000;
  };
}

function hashId(id: string) {
  let h = 2166136261;
  for (const c of id) h = Math.imul(h ^ c.charCodeAt(0), 16777619);
  return h >>> 0;
}

/** 返回 16×16 的颜色数组（逐行，从上到下），null 表示透明。 */
export function pattern(id: BlockId): (string | null)[] {
  const r = rng(hashId(id));
  const pick = (cs: string[]) => cs[Math.floor(r() * cs.length)];
  const px: (string | null)[] = new Array(N * N).fill(null);
  const set = (x: number, y: number, c: string | null) => {
    if (x >= 0 && x < N && y >= 0 && y < N) px[y * N + x] = c;
  };
  const fill = (cs: string[]) => {
    for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) set(x, y, pick(cs));
  };
  const dirt = ["#8a5a3b", "#7a4e33", "#9c6b48", "#83553a"];
  const wood = ["#b98a4e", "#a87a42", "#c79a5c"];
  switch (id) {
    case "grass": {
      fill(dirt);
      for (let x = 0; x < N; x++) {
        const depth = 3 + Math.floor(r() * 3);
        for (let y = 0; y < depth; y++) set(x, y, pick(["#5aa54b", "#6cbf55", "#4e9a40"]));
      }
      break;
    }
    case "dirt":
      fill(dirt);
      break;
    case "stone":
      fill(["#8d8d8d", "#9a9a9a", "#868686", "#939393"]);
      break;
    case "cobble": {
      fill(["#7f7f7f", "#929292", "#a3a3a3"]);
      for (let i = 0; i < 26; i++) set(Math.floor(r() * N), Math.floor(r() * N), "#5f5f5f");
      for (let y = 0; y < N; y += 5) for (let x = 0; x < N; x++) if (r() < 0.6) set(x, y, "#666666");
      break;
    }
    case "planks": {
      fill(wood);
      for (let y = 3; y < N; y += 4) for (let x = 0; x < N; x++) set(x, y, "#7c5631");
      for (let row = 0; row < 4; row++) set(((row * 5) % 12) + 2, row * 4 + 1, "#7c5631");
      break;
    }
    case "log": {
      for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) set(x, y, x % 4 === 0 ? "#4f3620" : pick(["#6b4a2b", "#5a3d22", "#7a5634"]));
      break;
    }
    case "bricks": {
      fill(["#b4533c", "#a14630", "#ba5d44"]);
      for (let y = 0; y < N; y++) {
        const mortarRow = y % 4 === 3;
        for (let x = 0; x < N; x++) {
          const offset = Math.floor(y / 4) % 2 === 0 ? 0 : 4;
          if (mortarRow || (x + offset) % 8 === 7) set(x, y, "#d9cbb5");
        }
      }
      break;
    }
    case "glass": {
      for (let i = 0; i < N; i++) {
        set(i, 0, "#cfe9f5");
        set(i, N - 1, "#cfe9f5");
        set(0, i, "#cfe9f5");
        set(N - 1, i, "#cfe9f5");
      }
      for (let i = 3; i < 8; i++) set(i, 11 - i, "#eaf6fb");
      break;
    }
    case "sand":
      fill(["#e6d39a", "#dcc78a", "#efe0aa"]);
      break;
    case "leaves": {
      for (let y = 0; y < N; y++) for (let x = 0; x < N; x++) set(x, y, r() < 0.12 ? null : pick(["#4f9a3c", "#5fae47", "#3f8a32"]));
      break;
    }
    case "bookshelf": {
      fill(wood);
      const spines = ["#c0392b", "#2e86c1", "#27ae60", "#f1c40f", "#8e44ad", "#e67e22"];
      for (const top of [2, 9]) {
        for (let x = 1; x < N - 1; x++) {
          const c = x % 3 === 0 ? "#3b2a1a" : spines[(x * 7 + top) % spines.length];
          for (let y = top; y < top + 5; y++) set(x, y, c);
        }
      }
      break;
    }
    case "lamp": {
      fill(["#b8742a"]);
      for (let y = 2; y < N - 2; y++) for (let x = 2; x < N - 2; x++) set(x, y, (x + y) % 5 === 0 ? "#ffe9a3" : "#ffd76a");
      break;
    }
    case "wool":
      fill(["#f2f2f2", "#e6e6e6", "#ececec"]);
      break;
    case "flower": {
      for (let y = 8; y < N; y++) set(7, y, "#4f9a3c");
      set(6, 11, "#5fae47");
      set(8, 13, "#5fae47");
      for (const [dx, dy] of [
        [0, -2],
        [0, 2],
        [-2, 0],
        [2, 0],
        [-1, -1],
        [1, -1],
        [-1, 1],
        [1, 1],
      ])
        set(7 + dx, 5 + dy, "#ff6b8a");
      set(7, 5, "#ffd45c");
      break;
    }
    case "bed": {
      for (let y = 6; y < 12; y++) for (let x = 0; x < N; x++) set(x, y, x < 5 ? "#f5f5f5" : pick(["#d8342a", "#c92f26"]));
      for (let x = 0; x < N; x++) set(x, 12, "#7c5631");
      for (const x of [0, 1, 14, 15]) for (let y = 13; y < N; y++) set(x, y, "#5a3d22");
      break;
    }
    case "workbench": {
      fill(wood);
      for (let x = 0; x < N; x++) for (let y = 0; y < 3; y++) set(x, y, "#7c5631");
      // 一把锤子和一把锯子的小图标。
      for (let y = 5; y < 12; y++) set(4, y, "#6b4a2b");
      for (let x = 2; x < 7; x++) set(x, 5, "#9aa5ad");
      for (let x = 9; x < 14; x++) for (let y = 7; y < 10; y++) set(x, y, "#c3ccd2");
      for (let x = 9; x < 14; x += 2) set(x, 10, "#c3ccd2");
      break;
    }
  }
  return px;
}

const cache = new Map<string, HTMLCanvasElement>();

/** 内置纹理画成 16×16 的离屏画布（结果缓存，只生成一次）。 */
export function builtinTexture(id: BlockId): HTMLCanvasElement | null {
  const hit = cache.get(id);
  if (hit) return hit;
  const c = document.createElement("canvas");
  c.width = N;
  c.height = N;
  const ctx = c.getContext("2d");
  if (!ctx) return null;
  const px = pattern(id);
  for (let y = 0; y < N; y++)
    for (let x = 0; x < N; x++) {
      const col = px[y * N + x];
      if (col) {
        ctx.fillStyle = col;
        ctx.fillRect(x, y, 1, 1);
      }
    }
  cache.set(id, c);
  return c;
}

/** 有些材质包里的树叶 / 草是灰度图（游戏里按地形着色）。灰度时给它染上绿色。 */
export function needsTint(id: string, data: Uint8ClampedArray): boolean {
  if (id !== "leaves" && id !== "grass") return false;
  let sat = 0;
  let n = 0;
  for (let i = 0; i < data.length; i += 4) {
    if (data[i + 3] < 10) continue;
    const r = data[i],
      g = data[i + 1],
      b = data[i + 2];
    const max = Math.max(r, g, b),
      min = Math.min(r, g, b);
    sat += max === 0 ? 0 : (max - min) / max;
    n++;
  }
  return n > 0 && sat / n < 0.12;
}

/** 把导入的纹理（data URL）加载成可绘制的画布，必要时染色。 */
export function loadPackTexture(id: string, url: string): Promise<HTMLCanvasElement | null> {
  return new Promise((resolve) => {
    const img = new Image();
    img.onload = () => {
      const c = document.createElement("canvas");
      c.width = img.width;
      c.height = img.width; // 动画纹理是竖条，只取第一帧
      const ctx = c.getContext("2d");
      if (!ctx) return resolve(null);
      ctx.drawImage(img, 0, 0);
      try {
        const data = ctx.getImageData(0, 0, c.width, c.height).data;
        if (needsTint(id, data)) {
          ctx.globalCompositeOperation = "multiply";
          ctx.fillStyle = "#6fae4a";
          ctx.fillRect(0, 0, c.width, c.height);
          ctx.globalCompositeOperation = "destination-in";
          ctx.drawImage(img, 0, 0);
        }
      } catch {
        /* 跨域等原因读不到像素时不染色 */
      }
      resolve(c);
    };
    img.onerror = () => resolve(null);
    img.src = url;
  });
}
