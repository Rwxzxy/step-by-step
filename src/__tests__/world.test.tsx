import { fireEvent, render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import { BLOCKS, BLOCK_LABEL, TINT, needsTint, pattern } from "../world/textures";
import type { StripView } from "../types";

const calls: { name: string; args: unknown[] }[] = [];
let currentView: StripView;
vi.mock("../api", () => ({
  api: new Proxy(
    {},
    {
      get: (_t, name: string) =>
        (...args: unknown[]) => {
          calls.push({ name, args });
          if (name === "worldView") return Promise.resolve(currentView);
          if (name === "getTextures") return Promise.resolve({});
          return Promise.resolve(true);
        },
    },
  ),
  on: () => Promise.resolve(() => undefined),
}));

import { StripWindow, cellAt } from "../world/StripWindow";

/** jsdom 没有完整的 PointerEvent：用带坐标的 MouseEvent 模拟。 */
function pointer(el: Element, type: "pointerdown" | "pointerup" | "pointermove", init: MouseEventInit = {}) {
  fireEvent(el, new MouseEvent(type, { bubbles: true, cancelable: true, ...init }));
}

function view(build: boolean): StripView {
  const cols = 10;
  const rows = 12;
  const cells = new Array(cols * rows).fill("air");
  cells[0] = "grass";
  return { key: "m", cols, rows, cell: 24, cells, build_mode: build };
}

beforeEach(() => {
  calls.length = 0;
});

describe("方块纹理", () => {
  it("每种方块都有 16×16 的原创像素纹理，且每次生成结果相同", () => {
    for (const b of BLOCKS) {
      const p = pattern(b);
      expect(p).toHaveLength(256);
      expect(p).toEqual(pattern(b));
      expect(p.some((c) => c !== null)).toBe(true);
      expect(BLOCK_LABEL[b]).toBeTruthy();
    }
    // 玻璃中间透明，花大部分透明。
    expect(pattern("glass")[8 * 16 + 12]).toBeNull();
    expect(pattern("flower").filter((c) => c === null).length).toBeGreaterThan(200);
  });

  it("灰度的树叶 / 水纹理需要染色，彩色的不用", () => {
    const grey = new Uint8ClampedArray([100, 100, 100, 255, 120, 120, 120, 255]);
    const green = new Uint8ClampedArray([40, 160, 40, 255, 60, 170, 50, 255]);
    expect(needsTint("leaves", grey)).toBe(true);
    expect(needsTint("leaves", green)).toBe(false);
    expect(needsTint("stone", grey)).toBe(false);
    expect(needsTint("water", grey)).toBe(true);
    expect(TINT.water).toMatch(/^#/);
    expect(BLOCK_LABEL.water).toBe("水");
  });
});

describe("建造带", () => {
  it("鼠标位置换算成格子坐标（y 从下往上）", () => {
    const v = view(true);
    expect(cellAt(v, 0, 12 * 24 - 1)).toEqual({ x: 0, y: 0 });
    expect(cellAt(v, 24 * 3 + 5, 5)).toEqual({ x: 3, y: 11 });
    expect(cellAt(v, 24 * 10, 0)).toBeNull();
    expect(cellAt(v, -1, 0)).toBeNull();
  });

  it("非建造模式下不显示工具栏，点击不会放方块", async () => {
    currentView = view(false);
    render(<StripWindow />);
    const canvas = await screen.findByTestId("strip-canvas");
    expect(screen.queryByRole("toolbar")).toBeNull();
    pointer(canvas, "pointerdown", { clientX: 30, clientY: 200, button: 0 });
    expect(calls.some((c) => c.name === "worldSet")).toBe(false);
  });

  it("建造模式：选方块、左键放置、右键拆除、完成", async () => {
    currentView = view(true);
    render(<StripWindow />);
    const canvas = await screen.findByTestId("strip-canvas");
    expect(screen.getByRole("toolbar", { name: "方块" })).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "砖块" }));
    expect(screen.getByRole("button", { name: "砖块" })).toHaveAttribute("aria-pressed", "true");
    // 第 2 列、从下往上第 1 行。
    pointer(canvas, "pointerdown", { clientX: 2 * 24 + 3, clientY: 12 * 24 - 3, button: 0 });
    pointer(canvas, "pointerup");
    expect(calls.find((c) => c.name === "worldSet")?.args).toEqual([2, 0, "bricks"]);
    // 右键拆掉第 0 列的草地。
    pointer(canvas, "pointerdown", { clientX: 3, clientY: 12 * 24 - 3, button: 2 });
    pointer(canvas, "pointerup");
    expect(calls.filter((c) => c.name === "worldSet").pop()?.args).toEqual([0, 0, "air"]);
    await userEvent.click(screen.getByRole("button", { name: "完成" }));
    expect(calls.some((c) => c.name === "setBuildMode" && c.args[0] === false)).toBe(true);
  });
});
