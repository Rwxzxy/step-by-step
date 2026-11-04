// 建造带窗口：屏幕底部一条透明长条，画出方块；建造模式下可以放置 / 拆除。
// 只在内容变化时重画一次画布，平时不跑渲染循环。
import { useCallback, useEffect, useRef, useState } from "react";
import { api, on } from "../api";
import type { StripView } from "../types";
import { BLOCKS, BLOCK_LABEL, TRANSLUCENT, builtinTexture, loadPackTexture, type BlockId } from "./textures";
import "./strip.css";

type Tex = Record<string, HTMLCanvasElement | null>;

/** 把画布上的鼠标位置换成格子坐标（y 从下往上）。 */
export function cellAt(view: StripView, offsetX: number, offsetY: number): { x: number; y: number } | null {
  const x = Math.floor(offsetX / view.cell);
  const yFromTop = Math.floor(offsetY / view.cell);
  const y = view.rows - 1 - yFromTop;
  if (x < 0 || x >= view.cols || y < 0 || y >= view.rows) return null;
  return { x, y };
}

export function StripWindow() {
  const [view, setView] = useState<StripView | null>(null);
  const [selected, setSelected] = useState<BlockId>("grass");
  const [hover, setHover] = useState<{ x: number; y: number } | null>(null);
  const [pack, setPack] = useState<Tex>({});
  const canvasRef = useRef<HTMLCanvasElement>(null);
  const painting = useRef<null | "place" | "erase">(null);

  const reload = useCallback(() => {
    api.worldView().then(setView).catch(() => undefined);
  }, []);

  const reloadTextures = useCallback(() => {
    api
      .getTextures()
      .then(async (map) => {
        const entries = await Promise.all(Object.entries(map).map(async ([id, url]) => [id, await loadPackTexture(id, url)] as const));
        setPack(Object.fromEntries(entries));
      })
      .catch(() => undefined);
  }, []);

  useEffect(() => {
    document.documentElement.classList.add("strip-window");
    reload();
    reloadTextures();
    const subs = [on("world://changed", reload), on("world://build", reload), on("world://textures", reloadTextures)];
    const onKey = (e: KeyboardEvent) => e.key === "Escape" && api.setBuildMode(false);
    window.addEventListener("keydown", onKey);
    return () => {
      subs.forEach((p) => p.then((f) => f()));
      window.removeEventListener("keydown", onKey);
    };
  }, [reload, reloadTextures]);

  // 内容变化时重画。
  useEffect(() => {
    const c = canvasRef.current;
    if (!c || !view) return;
    const dpr = window.devicePixelRatio || 1;
    const w = view.cols * view.cell;
    const h = view.rows * view.cell;
    c.width = Math.round(w * dpr);
    c.height = Math.round(h * dpr);
    c.style.width = `${w}px`;
    c.style.height = `${h}px`;
    const ctx = c.getContext("2d");
    if (!ctx) return;
    ctx.setTransform(dpr, 0, 0, dpr, 0, 0);
    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, w, h);
    for (let y = 0; y < view.rows; y++) {
      for (let x = 0; x < view.cols; x++) {
        const id = view.cells[y * view.cols + x];
        if (!id || id === "air") continue;
        const tex = pack[id] ?? builtinTexture(id as BlockId);
        if (!tex) continue;
        const px = x * view.cell;
        const py = (view.rows - 1 - y) * view.cell;
        ctx.globalAlpha = TRANSLUCENT[id as BlockId] ?? 1;
        ctx.drawImage(tex, px, py, view.cell, view.cell);
        // 水面（上面不是水）画一道浅色的边。
        if (id === "water" && view.cells[(y + 1) * view.cols + x] !== "water") {
          ctx.fillStyle = "rgba(190, 220, 255, 0.9)";
          ctx.fillRect(px, py, view.cell, 2);
        }
        ctx.globalAlpha = 1;
      }
    }
  }, [view, pack]);

  const apply = (cell: { x: number; y: number }, mode: "place" | "erase") => {
    if (!view) return;
    const block = mode === "place" ? selected : "air";
    const current = view.cells[cell.y * view.cols + cell.x];
    if (current === block) return;
    // 先在本地更新，界面不用等后端。
    const cells = view.cells.slice();
    cells[cell.y * view.cols + cell.x] = block;
    setView({ ...view, cells });
    api.worldSet(cell.x, cell.y, block).catch(() => reload());
  };

  const local = (e: React.PointerEvent<HTMLCanvasElement>) => {
    const r = e.currentTarget.getBoundingClientRect();
    return [e.clientX - r.left, e.clientY - r.top] as const;
  };

  const onDown = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (!view?.build_mode) return;
    const cell = cellAt(view, ...local(e));
    if (!cell) return;
    e.currentTarget.setPointerCapture?.(e.pointerId);
    painting.current = e.button === 2 ? "erase" : "place";
    apply(cell, painting.current);
  };
  const onMove = (e: React.PointerEvent<HTMLCanvasElement>) => {
    if (!view?.build_mode) return;
    const cell = cellAt(view, ...local(e));
    setHover(cell);
    if (cell && painting.current) apply(cell, painting.current);
  };
  const onUp = () => {
    painting.current = null;
  };

  if (!view) return null;
  const build = view.build_mode;

  return (
    <div className={`strip${build ? " building" : ""}`}>
      <canvas
        ref={canvasRef}
        data-testid="strip-canvas"
        onPointerDown={onDown}
        onPointerMove={onMove}
        onPointerUp={onUp}
        onPointerLeave={() => setHover(null)}
        onContextMenu={(e) => e.preventDefault()}
      />
      {build && hover && (
        <div
          className="hover-cell"
          style={{ left: hover.x * view.cell, top: (view.rows - 1 - hover.y) * view.cell, width: view.cell, height: view.cell }}
        />
      )}
      {build && (
        <div className="palette" role="toolbar" aria-label="方块">
          {BLOCKS.map((b) => (
            <button
              key={b}
              className={b === selected ? "on" : ""}
              title={BLOCK_LABEL[b]}
              aria-label={BLOCK_LABEL[b]}
              aria-pressed={b === selected}
              onClick={() => setSelected(b)}
            >
              <BlockIcon id={b} pack={pack[b]} />
            </button>
          ))}
          <span className="hint">左键放置 · 右键拆除 · 按住拖动连续画 · Esc 退出</span>
          <button className="done" onClick={() => api.setBuildMode(false)}>
            完成
          </button>
        </div>
      )}
    </div>
  );
}

function BlockIcon({ id, pack }: { id: BlockId; pack?: HTMLCanvasElement | null }) {
  const ref = useRef<HTMLCanvasElement>(null);
  useEffect(() => {
    const c = ref.current;
    const ctx = c?.getContext("2d");
    if (!c || !ctx) return;
    ctx.imageSmoothingEnabled = false;
    ctx.clearRect(0, 0, c.width, c.height);
    const tex = pack ?? builtinTexture(id);
    if (tex) ctx.drawImage(tex, 0, 0, c.width, c.height);
  }, [id, pack]);
  return <canvas ref={ref} width={20} height={20} />;
}
