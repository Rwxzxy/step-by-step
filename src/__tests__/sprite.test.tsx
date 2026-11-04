import { render } from "@testing-library/react";
import { describe, expect, it } from "vitest";
import { ANIM_LABEL, PetSprite } from "../pet/PetSprite";
import type { Anim } from "../types";

describe("宠物精灵", () => {
  it("每个动作都能渲染，并带有对应的元素", () => {
    for (const anim of Object.keys(ANIM_LABEL) as Anim[]) {
      const { container, unmount } = render(<PetSprite anim={anim} facing="left" hue={200} />);
      const svg = container.querySelector("svg")!;
      expect(svg.getAttribute("data-anim")).toBe(anim);
      expect(svg.classList.contains("facing-left")).toBe(true);
      expect(svg.getAttribute("aria-label")).toContain(ANIM_LABEL[anim]);
      unmount();
    }
    const cal = render(<PetSprite anim="calendar" facing="right" hue={0} day={17} />);
    expect(cal.container.querySelector(".calendar-card")?.textContent).toContain("17");
    cal.unmount();
    expect(render(<PetSprite anim="sleep" facing="right" hue={0} />).container.querySelector(".zzz")).not.toBeNull();
    expect(render(<PetSprite anim="raise_hand" facing="right" hue={0} />).container.querySelector(".exclaim")).not.toBeNull();
    expect(render(<PetSprite anim="anxious" facing="right" hue={0} />).container.querySelector(".sweat")).not.toBeNull();
    expect(render(<PetSprite anim="happy" facing="right" hue={0} />).container.querySelector(".sparkles")).not.toBeNull();
  });
});
