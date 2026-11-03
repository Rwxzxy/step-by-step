import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { describe, expect, it, vi } from "vitest";
import { PetSprite } from "../pet/PetSprite";
import { DEFAULT_ACCENT, HOLIDAY_IDS, accentOf } from "../themes/holidays";
import { snapshot } from "./fixtures";
import type { Settings } from "../types";

vi.mock("../api", () => ({
  api: {
    listHolidays: () =>
      Promise.resolve([
        { id: "mid_autumn", name: "中秋节", next_date: "2027-09-15" },
        { id: "custom", name: "自定义纪念日", next_date: null },
      ]),
  },
  on: () => Promise.resolve(() => undefined),
}));

import { HolidaySection } from "../panel/SettingsView";

describe("节日主题", () => {
  it("每个节日都有占位图形和强调色", () => {
    for (const id of HOLIDAY_IDS) {
      const { container, unmount } = render(<PetSprite anim="idle" facing="right" hue={30} holiday={id} />);
      const overlay = container.querySelector(".holiday-overlay")!;
      expect(overlay.getAttribute("data-holiday")).toBe(id);
      expect(overlay.getAttribute("data-source")).toBe("placeholder");
      expect(overlay.childElementCount).toBeGreaterThan(0);
      expect(DEFAULT_ACCENT[id]).toMatch(/^#/);
      unmount();
    }
  });

  it("没有节日时不画叠加层", () => {
    const { container } = render(<PetSprite anim="walk" facing="left" hue={30} holiday={null} />);
    expect(container.querySelector(".holiday-overlay")).toBeNull();
  });

  it("登记了正式资源时用图片，可以放在身后", () => {
    const { container } = render(
      <PetSprite anim="idle" facing="right" hue={30} holiday="spring_festival" holidayAsset={{ overlay: "spring_festival/overlay.svg", layer: "back" }} />,
    );
    const overlay = container.querySelector(".holiday-overlay")!;
    expect(overlay.getAttribute("data-source")).toBe("asset");
    expect(overlay.querySelector("image")!.getAttribute("href")).toBe("/themes/spring_festival/overlay.svg");
    // 身后：在脚和身体之前绘制。
    const flip = container.querySelector(".pet-flip")!;
    expect(flip.firstElementChild).toBe(overlay);
  });

  it("manifest 里的强调色优先", () => {
    expect(accentOf("mid_autumn")).toBe(DEFAULT_ACCENT.mid_autumn);
    expect(accentOf("mid_autumn", { version: 1, themes: { mid_autumn: { accent: "#123456" } } })).toBe("#123456");
    expect(accentOf(null)).toBeUndefined();
  });

  it("设置页：开关、预览和自定义纪念日", async () => {
    const settings = {
      holiday_theme: true,
      holiday_preview: null,
      custom_holidays: [{ name: "妈妈生日", month: 2, day: 29, greeting: "" }],
    } as unknown as Settings;
    const save = vi.fn();
    const snap = snapshot({ holiday: { id: "mid_autumn", name: "中秋节", greeting: "中秋快乐", main_day: true, date: "2026-09-25" } });
    render(<HolidaySection settings={settings} snap={snap} save={save} />);
    expect(screen.getByText("现在：中秋节")).toBeInTheDocument();
    await screen.findByRole("option", { name: "中秋节（09月15日）" });
    await userEvent.selectOptions(screen.getByLabelText("预览节日外观"), "mid_autumn");
    expect(save).toHaveBeenLastCalledWith({ holiday_preview: "mid_autumn" });
    await userEvent.click(screen.getByText("节日时自动切换宠物外观，并在当天送上祝福"));
    expect(save).toHaveBeenLastCalledWith({ holiday_theme: false });
    // 2 月可以选到 29 日。
    expect(screen.getByLabelText("日")).toHaveValue("29");
    await userEvent.click(screen.getByText("删除"));
    expect(save).toHaveBeenLastCalledWith({ custom_holidays: [] });
    await userEvent.click(screen.getByText("+ 添加纪念日"));
    expect(save.mock.calls.at(-1)![0].custom_holidays).toHaveLength(2);
  });
});
