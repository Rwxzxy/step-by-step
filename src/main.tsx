import React from "react";
import ReactDOM from "react-dom/client";
import { getCurrentWebviewWindow } from "@tauri-apps/api/webviewWindow";
import { PetWindow } from "./pet/PetWindow";
import { PanelApp } from "./panel/PanelApp";
import { SpriteGallery } from "./pet/SpriteGallery";

function pick() {
  // 不在 Tauri 里（普通浏览器打开 npm run dev）：显示动作预览。
  if (!("__TAURI_INTERNALS__" in window)) return <SpriteGallery />;
  // 同一份前端按窗口 label 渲染不同界面：pet / guest-* 是宠物窗口，panel 是面板。
  const label = getCurrentWebviewWindow().label;
  return label === "pet" || label.startsWith("guest-") ? <PetWindow /> : <PanelApp />;
}

ReactDOM.createRoot(document.getElementById("root")!).render(<React.StrictMode>{pick()}</React.StrictMode>);
