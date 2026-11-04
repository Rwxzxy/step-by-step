import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";
import type { FoundPet, SkinInfo } from "../types";

const calls: { name: string; args: unknown[] }[] = [];
let skins: SkinInfo[] = [];
let found: FoundPet[] = [];
vi.mock("../api", () => ({
  api: new Proxy(
    {},
    {
      get: (_t, name: string) =>
        (...args: unknown[]) => {
          calls.push({ name, args });
          if (name === "listSkins") return Promise.resolve(skins);
          if (name === "discoverSkins") return Promise.resolve(found);
          if (name === "getSkinSheet") return Promise.resolve(null);
          if (name === "importSkinFrom") return Promise.resolve(skins[0]);
          return Promise.resolve();
        },
    },
  ),
  on: () => Promise.resolve(() => undefined),
}));

import { ANIM_LABEL } from "../pet/PetSprite";
import { SkinSprite, skinScale } from "../pet/SkinSprite";
import { FULL_FRAME, STATES, boundsFromAlpha, dataUrlToBlob, stateFor, type LoadedSkin } from "../pet/skin";
import { SkinSection } from "../panel/SkinSection";
import type { Anim } from "../types";

const info = (id: string, name: string): SkinInfo => ({
  id,
  name,
  description: null,
  atlas: { version: 1, rows: 9, width: 1536, height: 1872 },
  file: "sheet.webp",
  source: "/x",
});

const loaded: LoadedSkin = { id: "steve-4", name: "Steve", src: "blob:steve", bounds: { minX: 56, minY: 40, maxX: 136, maxY: 200 } };

beforeEach(() => {
  calls.length = 0;
  skins = [];
  found = [];
});

describe("Codex 宠物格式", () => {
  it("动作表和 Codex 一致：9 行、每行不超过 8 帧", () => {
    const rows = Object.values(STATES).map((s) => s.row);
    expect(rows).toEqual([0, 1, 2, 3, 4, 5, 6, 7, 8]);
    expect(Object.values(STATES).every((s) => s.frames <= 8 && s.ms > 0)).toBe(true);
  });

  it("步步的每个动作都对应到一行，走路按方向选行", () => {
    for (const a of Object.keys(ANIM_LABEL) as Anim[]) expect(STATES[stateFor(a, "right").state]).toBeDefined();
    expect(stateFor("walk", "left").state).toBe("running-left");
    expect(stateFor("walk", "right").state).toBe("running-right");
    expect(stateFor("raise_hand", "right").state).toBe("waving");
    expect(stateFor("anxious", "right").state).toBe("failed");
    expect(stateFor("happy", "right").state).toBe("jumping");
    expect(stateFor("calendar", "right").state).toBe("review");
    expect(stateFor("sleep", "right")).toEqual({ state: "idle", slow: 2.5 });
  });

  it("从待机那一行的像素算出角色外框（所有帧的并集，换算回标准帧坐标）", () => {
    // 缩小一半的图集：一行 8 帧，每帧 96 × 104。
    const w = 8 * 96,
      h = 104;
    const data = new Uint8ClampedArray(w * h * 4);
    const dot = (x: number, y: number) => (data[(y * w + x) * 4 + 3] = 255);
    dot(30, 20); // 第 1 帧
    dot(96 + 60, 90); // 第 2 帧
    dot(7 * 96 + 1, 1); // 第 8 帧：超出待机的 6 帧，不算
    expect(boundsFromAlpha(data, w, h, 6)).toEqual({ minX: 60, minY: 40, maxX: 122, maxY: 182 });
    expect(boundsFromAlpha(new Uint8ClampedArray(w * h * 4), w, h, 6)).toEqual(FULL_FRAME);
  });

  it("data URL 解码成 Blob", async () => {
    const b = dataUrlToBlob("data:image/png;base64,iVBORw==");
    expect(b.type).toBe("image/png");
    expect(b.size).toBe(4);
  });
});

describe("精灵图形象", () => {
  it("按动作选行、按帧数播放，外框就是角色范围", () => {
    const { container } = render(<SkinSprite skin={loaded} anim="walk" facing="left" size={112} />);
    const root = container.querySelector(".skin-sprite") as HTMLElement;
    expect(root.getAttribute("data-state")).toBe("running-left");
    expect(root.getAttribute("aria-label")).toBe("Steve：走路");
    const k = skinScale(loaded, 112);
    expect(k).toBeCloseTo((112 * 0.85) / 160);
    expect(parseFloat(root.style.width)).toBeCloseTo(80 * k);
    expect(parseFloat(root.style.height)).toBeCloseTo(160 * k);
    const sheet = container.querySelector(".skin-sheet") as HTMLElement;
    expect(sheet.style.backgroundImage).toContain("blob:steve");
    expect(parseFloat(sheet.style.backgroundPositionY)).toBeCloseTo(-2 * 208 * k);
    expect(sheet.style.animation).toContain("steps(8)");
    expect(parseFloat(sheet.style.left)).toBeCloseTo(-56 * k);
  });

  it("附加小图标照常显示", () => {
    const icon = (anim: Anim, sel: string) =>
      render(<SkinSprite skin={loaded} anim={anim} facing="right" />).container.querySelector(sel);
    expect(icon("sleep", ".zzz")).not.toBeNull();
    expect(icon("raise_hand", ".exclaim")).not.toBeNull();
    expect(icon("anxious", ".sweat")).not.toBeNull();
    expect(icon("happy", ".sparkles")).not.toBeNull();
    const cal = render(<SkinSprite skin={loaded} anim="calendar" facing="right" day={9} />);
    expect(cal.container.querySelector(".calendar-card")?.textContent).toBe("9");
    // 睡觉时待机动画放慢。
    const sleep = render(<SkinSprite skin={loaded} anim="sleep" facing="right" />);
    expect((sleep.container.querySelector(".skin-sheet") as HTMLElement).style.animation).toContain("2.750s");
  });
});

describe("设置 → 宠物形象", () => {
  it("列出默认和已导入的形象，点击切换、删除", async () => {
    skins = [info("steve-4", "Steve")];
    vi.spyOn(window, "confirm").mockReturnValue(true);
    render(<SkinSection current={null} hue={30} notify={() => undefined} />);
    const steve = await screen.findByRole("radio", { name: /Steve/ });
    expect(screen.getByRole("radio", { name: /步步（默认）/ })).toHaveAttribute("aria-checked", "true");
    await userEvent.click(steve);
    expect(calls.find((c) => c.name === "setPetSkin")?.args).toEqual(["steve-4"]);
    expect(steve).toHaveAttribute("aria-checked", "true");
    expect(screen.getByText("动作预览")).toBeInTheDocument();
    await userEvent.click(screen.getByRole("button", { name: "删除Steve" }));
    expect(calls.find((c) => c.name === "deleteSkin")?.args).toEqual(["steve-4"]);
  });

  it("本机找到的 Codex 宠物可以一键导入，已导入的不再列出", async () => {
    skins = [info("boba", "Boba")];
    found = [
      { path: "/home/me/.codex/pets/boba", id: "boba", name: "Boba", imported: true },
      { path: "/home/me/.codex/pets/steve-4", id: "steve-4", name: "Steve", imported: false },
    ];
    render(<SkinSection current="boba" hue={30} notify={() => undefined} />);
    await screen.findByText("在本机找到的 Codex / Petdex 宠物");
    const items = screen.getAllByRole("listitem");
    expect(items).toHaveLength(1);
    expect(items[0]).toHaveTextContent("Steve");
    await userEvent.click(screen.getByRole("button", { name: "导入并换上" }));
    expect(calls.find((c) => c.name === "importSkinFrom")?.args).toEqual(["/home/me/.codex/pets/steve-4"]);
  });
});
