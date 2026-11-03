// 宠物窗口：渲染宠物和气泡，处理点击 / 拖拽，并把可点击区域告诉后端（其他区域鼠标穿透）。
import { useCallback, useEffect, useLayoutEffect, useRef, useState } from "react";
import { api, on } from "../api";
import type { ActiveHoliday, Bubble, FramePayload, WindowInfo } from "../types";
import { accentOf, useManifest } from "../themes/holidays";
import { PetSprite } from "./PetSprite";
import "./pet.css";

const DRAG_THRESHOLD = 4;

export function PetWindow() {
  const [info, setInfo] = useState<WindowInfo | null>(null);
  const [frame, setFrame] = useState<FramePayload>({ anim: "idle", facing: "right", visible: true });
  const [bubbles, setBubbles] = useState<Bubble[]>([]);
  const [dragging, setDragging] = useState(false);
  const [holiday, setHoliday] = useState<ActiveHoliday | null>(null);
  const manifest = useManifest();
  const stageRef = useRef<HTMLDivElement>(null);
  const press = useRef<{ x: number; y: number; dragging: boolean } | null>(null);

  const loadInfo = useCallback(() => {
    api.petWindowInfo().then((i) => {
      setInfo(i);
      setHoliday(i.holiday);
      if (i.frame) setFrame(i.frame);
    });
  }, []);

  useEffect(() => {
    document.documentElement.classList.add("pet-window");
    loadInfo();
    const unsub = [
      on<FramePayload>("pet://frame", setFrame),
      on<Bubble>("pet://bubble", (b) => setBubbles((list) => [...list.filter((x) => x.id !== b.id), b].slice(-3))),
      on<null>("pet://appearance", () => loadInfo()),
      on<ActiveHoliday | null>("steppy://theme", setHoliday),
    ];
    return () => {
      unsub.forEach((p) => p.then((f) => f()));
    };
  }, [loadInfo]);

  const current = bubbles[bubbles.length - 1];

  // 气泡到时自动消失。
  useEffect(() => {
    if (!current) return;
    const t = window.setTimeout(() => setBubbles((l) => l.filter((b) => b.id !== current.id)), current.ttl_ms);
    return () => window.clearTimeout(t);
  }, [current]);

  // 把宠物和气泡的矩形报给后端，用来决定什么时候接收鼠标。
  useLayoutEffect(() => {
    const report = () => {
      const stage = stageRef.current;
      if (!stage) return;
      const rects = [...stage.querySelectorAll<HTMLElement | SVGElement>("[data-hit]")].map((el) => {
        const r = el.getBoundingClientRect();
        return { x: r.left, y: r.top, w: r.width, h: r.height };
      });
      api.petHitRegions(rects).catch(() => undefined);
    };
    report();
    const t = window.setTimeout(report, 300);
    window.addEventListener("resize", report);
    return () => {
      window.clearTimeout(t);
      window.removeEventListener("resize", report);
    };
  }, [current, info, frame.anim]);

  const onPointerDown = (e: React.PointerEvent) => {
    if (e.button !== 0) return;
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
    press.current = { x: e.screenX, y: e.screenY, dragging: false };
  };
  const onPointerMove = (e: React.PointerEvent) => {
    const p = press.current;
    if (!p || p.dragging) return;
    if (Math.abs(e.screenX - p.x) + Math.abs(e.screenY - p.y) > DRAG_THRESHOLD) {
      p.dragging = true;
      setDragging(true);
      api.petDragStart();
    }
  };
  const onPointerUp = (e: React.PointerEvent) => {
    const p = press.current;
    press.current = null;
    try {
      (e.currentTarget as HTMLElement).releasePointerCapture(e.pointerId);
    } catch {
      /* 已释放 */
    }
    if (!p) return;
    if (p.dragging) {
      setDragging(false);
      api.petDragEnd();
    } else {
      api.petClicked();
    }
  };

  const dismiss = (id: string) => setBubbles((l) => l.filter((b) => b.id !== id));
  const guest = info?.guest ?? false;

  if (!frame.visible) return <div className="pet-stage" ref={stageRef} />;

  return (
    <div className="pet-stage" ref={stageRef}>
      {current && (
        <div
          className={`bubble kind-${current.kind}${guest ? " has-tag" : ""}`}
          style={current.kind === "holiday" ? ({ "--accent": accentOf(holiday?.id, manifest) } as React.CSSProperties) : undefined}
          data-hit
          role="status"
          onClick={() => current.actions.length === 0 && dismiss(current.id)}
        >
          <div>{current.text}</div>
          {current.actions.length > 0 && (
            <div className="bubble-actions">
              {current.actions.map((a) => (
                <button
                  key={a.id}
                  onClick={(ev) => {
                    ev.stopPropagation();
                    api.bubbleAction(current.id, a.id);
                    dismiss(current.id);
                  }}
                >
                  {a.label}
                </button>
              ))}
            </div>
          )}
        </div>
      )}
      {guest && info && <div className="name-tag">{info.owner_name}的步步</div>}
      <div
        className={`pet-holder${dragging ? " dragging" : ""}`}
        data-hit
        onPointerDown={onPointerDown}
        onPointerMove={onPointerMove}
        onPointerUp={onPointerUp}
        onContextMenu={(e) => e.preventDefault()}
        title={guest && info ? `${info.owner_name}的步步（来自「${info.machine_name}」）` : "点我打开面板，按住可以拖动"}
      >
        <PetSprite
          anim={frame.anim}
          facing={frame.facing}
          hue={info?.hue ?? 32}
          holiday={holiday?.id}
          holidayAsset={holiday ? manifest.themes[holiday.id] : undefined}
        />
      </div>
    </div>
  );
}
