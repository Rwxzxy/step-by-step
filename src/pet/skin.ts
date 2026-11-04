// 导入的宠物形象（Codex 宠物格式）：动作表、动作对应关系、精灵图加载和测量。
import { useEffect, useState } from "react";
import { api, on } from "../api";
import type { Anim, Side, SkinSheet } from "../types";

export const COLUMNS = 8;
export const FRAME_W = 192;
export const FRAME_H = 208;

export type SkinState = "idle" | "running-right" | "running-left" | "waving" | "jumping" | "failed" | "waiting" | "running" | "review";

/** Codex 宠物格式每一行的动作（和 crates/steppy-core/src/skin.rs 的 STATES 一致）。 */
export const STATES: Record<SkinState, { row: number; frames: number; ms: number }> = {
  idle: { row: 0, frames: 6, ms: 1100 },
  "running-right": { row: 1, frames: 8, ms: 1060 },
  "running-left": { row: 2, frames: 8, ms: 1060 },
  waving: { row: 3, frames: 4, ms: 700 },
  jumping: { row: 4, frames: 5, ms: 840 },
  failed: { row: 5, frames: 8, ms: 1220 },
  waiting: { row: 6, frames: 6, ms: 1010 },
  running: { row: 7, frames: 6, ms: 820 },
  review: { row: 8, frames: 6, ms: 1030 },
};

/** 步步的动作用精灵图的哪一行播放，以及放慢几倍。 */
export function stateFor(anim: Anim, facing: Side): { state: SkinState; slow: number } {
  switch (anim) {
    case "walk":
      return { state: facing === "left" ? "running-left" : "running-right", slow: 1 };
    case "sleep":
      return { state: "idle", slow: 2.5 };
    case "happy":
    case "fall":
      return { state: "jumping", slow: 1 };
    case "raise_hand":
    case "drag":
      return { state: "waving", slow: 1 };
    case "anxious":
      return { state: "failed", slow: 1 };
    case "calendar":
      return { state: "review", slow: 1 };
    default:
      return { state: "idle", slow: 1 };
  }
}

/** 角色在帧里实际占的范围（标准帧坐标，192 × 208）。 */
export interface Bounds {
  minX: number;
  minY: number;
  maxX: number;
  maxY: number;
}

export const FULL_FRAME: Bounds = { minX: 0, minY: 0, maxX: FRAME_W, maxY: FRAME_H };

/**
 * 从一整行帧的像素（RGBA，宽 = frames × 192，高 = 208）里找出角色的外框：所有帧不透明像素的并集。
 * 用待机那一行算，决定缩放、点击范围和脚底位置。
 */
export function boundsFromAlpha(data: Uint8ClampedArray, width: number, height: number, frames: number): Bounds {
  let minX = Infinity,
    minY = Infinity,
    maxX = -Infinity,
    maxY = -Infinity;
  const fw = width / COLUMNS;
  for (let y = 0; y < height; y++) {
    for (let x = 0; x < Math.min(width, fw * frames); x++) {
      if (data[(y * width + x) * 4 + 3] > 24) {
        const fx = x % fw;
        if (fx < minX) minX = fx;
        if (fx > maxX) maxX = fx;
        if (y < minY) minY = y;
        if (y > maxY) maxY = y;
      }
    }
  }
  if (!isFinite(minX)) return FULL_FRAME;
  // 换算回标准帧坐标（图集可能整体缩放过）。
  const k = FRAME_W / fw;
  return { minX: minX * k, minY: minY * k, maxX: (maxX + 1) * k, maxY: (maxY + 1) * k };
}

export interface LoadedSkin {
  id: string;
  name: string;
  /** blob: 地址（比 data URL 短，方便多个元素共用同一张图）。 */
  src: string;
  bounds: Bounds;
}

const cache = new Map<string, Promise<LoadedSkin | null>>();

function measure(img: HTMLImageElement, rows: number): Bounds {
  try {
    const rowH = img.naturalHeight / rows;
    const c = document.createElement("canvas");
    c.width = img.naturalWidth;
    c.height = Math.round(rowH);
    const ctx = c.getContext("2d");
    if (!ctx) return FULL_FRAME;
    ctx.drawImage(img, 0, 0, img.naturalWidth, rowH, 0, 0, c.width, c.height);
    const b = boundsFromAlpha(ctx.getImageData(0, 0, c.width, c.height).data, c.width, c.height, STATES.idle.frames);
    c.width = c.height = 0; // 立刻释放画布内存
    return b;
  } catch {
    return FULL_FRAME;
  }
}

/** data URL → Blob（不用 fetch，免得受内容安全策略的 connect-src 限制）。 */
export function dataUrlToBlob(url: string): Blob {
  const comma = url.indexOf(",");
  const mime = url.slice(5, url.indexOf(";")) || "application/octet-stream";
  const bin = atob(url.slice(comma + 1));
  const bytes = new Uint8Array(bin.length);
  for (let i = 0; i < bin.length; i++) bytes[i] = bin.charCodeAt(i);
  return new Blob([bytes], { type: mime });
}

async function load(id: string): Promise<LoadedSkin | null> {
  const sheet: SkinSheet | null = await api.getSkinSheet(id);
  if (!sheet) return null;
  const src = URL.createObjectURL(dataUrlToBlob(sheet.url));
  const img = new Image();
  img.src = src;
  await img.decode();
  return { id, name: sheet.name, src, bounds: measure(img, sheet.atlas.rows) };
}

/** 加载某个导入的形象（同一个窗口里只加载一次）。 */
export function loadSkin(id: string): Promise<LoadedSkin | null> {
  let p = cache.get(id);
  if (!p) {
    p = load(id).catch(() => null);
    cache.set(id, p);
  }
  return p;
}

/** 导入 / 删除形象后丢掉缓存（重新导入同名形象时图可能变了）。 */
export function forgetSkins() {
  for (const p of cache.values()) p.then((s) => s && URL.revokeObjectURL(s.src));
  cache.clear();
}

/** 形象有变化时（steppy://skin 事件）加一，用作 useSkin 的 version。 */
export function useSkinVersion(): number {
  const [v, setV] = useState(0);
  useEffect(() => {
    const un = on<null>("steppy://skin", () => {
      forgetSkins();
      setV((x) => x + 1);
    });
    return () => {
      un.then((f) => f());
    };
  }, []);
  return v;
}

export function useSkin(id: string | null | undefined, version = 0): LoadedSkin | null {
  const [skin, setSkin] = useState<LoadedSkin | null>(null);
  useEffect(() => {
    let alive = true;
    if (!id) {
      setSkin(null);
      return;
    }
    loadSkin(id).then((s) => alive && setSkin(s));
    return () => {
      alive = false;
    };
  }, [id, version]);
  return skin;
}
