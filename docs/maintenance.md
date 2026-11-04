# 维护手册：换宠物形象、改方块材质

这份文档写给以后维护步步的人（包括未来的你自己）。按“改动大小”从小到大排列：能不改代码就不改代码。

- [一、改之前：素材从哪来、能不能放进仓库](#一改之前素材从哪来能不能放进仓库)
- [二、换宠物形象](#二换宠物形象)
  - [1. 只换颜色（不改代码）](#1-只换颜色不改代码)
  - [2. 节日外观（不改代码）](#2-节日外观不改代码)
  - [3. 换成全新形象（改代码）](#3-换成全新形象改代码)
- [三、改方块材质](#三改方块材质)
  - [1. 从材质包导入（不改代码）](#1-从材质包导入不改代码)
  - [2. 修改自带纹理（改代码）](#2-修改自带纹理改代码)
  - [3. 新增一种方块（改代码）](#3-新增一种方块改代码)
- [四、改完之后怎么验证、怎么发版](#四改完之后怎么验证怎么发版)

---

## 一、改之前：素材从哪来、能不能放进仓库

这个仓库是**公开**的，进了仓库或安装包的图片等于对外发布。规则只有两条：

| 素材来源 | 能不能进仓库 / 安装包 |
| --- | --- |
| 自己画的；CC0（公共领域）；明确允许修改和再分发的开源协议（如 CC BY，需在 README 里署名） | 可以 |
| Minecraft 的史蒂夫、苦力怕、原版材质；Petdex / itch.io 上的同人图；任何“不写协议”或“仅个人使用”的素材 | **不可以**，只能自己电脑上用 |

“只能自己用”的素材有两种用法：

- 方块材质：用下面的“从材质包导入”，图片只复制到本机数据目录，不进仓库、不同步给其他电脑。
- 宠物形象：在本机改代码、本机构建安装，**不要提交和推送**这些图片（可以在本地的 `.git/info/exclude` 里加上图片路径，防止误提交）。

找素材可以去 [OpenGameArt](https://opengameart.org/)（可按 CC0 筛选）、[itch.io 免费素材](https://itch.io/game-assets/free)、[Kenney](https://kenney.nl/assets)（全部 CC0）。下载前看清每个页面写的协议。

---

## 二、换宠物形象

### 1. 只换颜色（不改代码）

面板 → 设置 → **宠物颜色** 滑块。每台电脑可以设不同颜色，串门时对方看到的也是你的颜色。

### 2. 节日外观（不改代码）

给某个节日加帽子、灯笼之类的叠加图：把图片放进 `public/themes/`，在 `public/themes/manifest.json` 里登记，重新构建即可。画布尺寸、节日 id 等详见 [docs/themes.md](themes.md)。

### 3. 换成全新形象（改代码）

现在的宠物“步步”是用 SVG 代码画出来的（`src/pet/PetSprite.tsx`），**没有**图片文件可以直接替换，所以换形象一定要改代码。有两条路：

- **A. 改 SVG 画法**：适合在现有小团子的基础上改样子（换耳朵、换小芽、改五官）。
- **B. 改用精灵图**：适合用现成的像素画角色（比如从素材网站下载的史莱姆）。

两条路都要遵守下面的约定，宠物引擎（Rust）只认这些东西。

#### 必须支持的 9 个动作

动作名定义在 `src/types.ts` 的 `Anim` 类型里，由后端决定什么时候播放哪个：

| 动作 | 什么时候出现 | 现在的样子 |
| --- | --- | --- |
| `idle` | 站着不动 | 呼吸、眨眼 |
| `walk` | 沿任务栏走、串门进出屏幕、爬方块 | 身体上下颠、手脚摆动 |
| `sleep` | 很久没动静 / 免打扰时段 | 闭眼，头上 Zzz |
| `happy` | Claude / Codex 任务完成、节日问候 | 双手举高、冒星星 |
| `raise_hand` | Claude 在等你批准 | 举手挥动、头上感叹号 |
| `anxious` | 任务出错（StopFailure 等） | 皱眉、冒汗、发抖 |
| `calendar` | 日程提醒 | 捧着写有今天日期的日历 |
| `drag` | 被鼠标拎起来 | 瞪大眼睛 |
| `fall` | 松手后下落、从方块边缘掉下 | 双手举高、张嘴 |

朝向由 `facing`（`left` / `right`）决定：只需要画**朝右**的样子，朝左时整体水平镜像。所以图里不要放文字。

#### 尺寸和位置

- 宠物窗口是 240 × 210 逻辑像素（`src-tauri/src/pet_rt.rs` 里的 `PET_W` / `PET_H`），多出来的空间给气泡用。
- 宠物图本身默认画成 112 × 112，**底边贴着窗口底边、水平居中**。脚底要画在图的最下面（现在的步步是 120 坐标系里 y≈110），否则会浮空或陷进任务栏。
- 宠物“身体”占窗口宽度的 30%–70%（`crates/steppy-core/src/pet.rs` 里 `Tuning` 的 `body_left` / `body_right`）。这决定了走到屏幕边缘、站上方块、串门出屏时按哪里算。新形象明显更宽或更窄时，改这两个数。
- 鼠标能点到的范围会自动按宠物元素的外框计算（`PetWindow.tsx` 里带 `data-hit` 的元素），不用手动改。

#### A. 改 SVG 画法

1. 打开 `src/pet/PetSprite.tsx`。所有图形都在一个 `viewBox="0 0 120 120"` 的坐标系里。
2. 保留这几个 class，`src/pet/pet.css` 的动画靠它们驱动：
   - `pet-flip`：整体镜像（朝左）；
   - `pet-body`：呼吸、走路颠簸、发抖；
   - `foot-l` / `foot-r`：走路时交替抬脚；
   - `blink`：眨眼；
   - `zzz`、`sparkles`、`exclaim`、`sweat`、`calendar-card`：各动作的附加小图标。
3. 手臂的摆动写在 `Arms` 函数里（用 SVG 的 `animateTransform`，以肩膀为支点），换手臂形状时同时改那里的坐标。
4. 节日叠加图（`HolidayOverlay`）的两个插入点不要删，分别在身体前面和后面。
5. `hue` 参数是“宠物颜色”滑块的值，身体颜色用它算出来；不想支持换色可以忽略它。

#### B. 改用精灵图

**准备图片。** 做一张精灵图（sprite sheet）：所有帧一样大，每个动作占一行，从左往右排。比如：

```
第 0 行 idle       □□□□
第 1 行 walk       □□□□□□□□
第 2 行 sleep      □□□□
第 3 行 happy      □□□□□□
第 4 行 raise_hand □□□□
第 5 行 anxious    □□□□
第 6 行 calendar   □□□□
第 7 行 drag/fall  □□
```

- 背景透明的 PNG，角色朝右，脚贴着每一帧的底边。
- 正方形帧最省事；不是正方形时，把下面代码里的 `height` 和 `backgroundPositionY` 按比例改。
- 现成素材动作不够时可以复用：比如 `calendar` 用 `idle` 那一行，`fall` 和 `drag` 共用一行。

把图片放到 `public/pets/mypet/sheet.png`（`mypet` 换成你的名字）。

**新建组件** `src/pet/PetImage.tsx`：

```tsx
// 用一张精灵图显示宠物（替换 PetSprite）。
import type { CSSProperties } from "react";
import type { Anim, Side } from "../types";
import { HolidayOverlay, type ThemeAsset } from "../themes/holidays";

const SHEET = "/pets/mypet/sheet.png"; // 对应 public/pets/mypet/sheet.png
const COLS = 8; // 精灵图一行最多几帧（决定图片总宽度）

// 每个动作：第几行、几帧、每秒几帧。按你的精灵图改。
const ROWS: Record<Anim, { row: number; frames: number; fps: number }> = {
  idle: { row: 0, frames: 4, fps: 4 },
  walk: { row: 1, frames: 8, fps: 10 },
  sleep: { row: 2, frames: 4, fps: 2 },
  happy: { row: 3, frames: 6, fps: 8 },
  raise_hand: { row: 4, frames: 4, fps: 6 },
  anxious: { row: 5, frames: 4, fps: 8 },
  calendar: { row: 6, frames: 4, fps: 3 },
  drag: { row: 7, frames: 2, fps: 4 },
  fall: { row: 7, frames: 2, fps: 4 },
};

interface Props {
  anim: Anim;
  facing: Side;
  hue?: number;
  size?: number;
  day?: number;
  holiday?: string | null;
  holidayAsset?: ThemeAsset;
}

export function PetImage({ anim, facing, size = 112, holiday, holidayAsset }: Props) {
  const { row, frames, fps } = ROWS[anim];
  const style = {
    width: size,
    height: size,
    backgroundImage: `url(${SHEET})`,
    backgroundSize: `${COLS * size}px auto`,
    backgroundPositionY: `${-row * size}px`,
    "--frames": frames,
    "--size": `${size}px`,
    animation: `pet-frames ${frames / fps}s steps(${frames}) infinite`,
    transform: facing === "left" ? "scaleX(-1)" : undefined,
  } as CSSProperties;
  return (
    <div className="pet-image" data-anim={anim} role="img" aria-label={`宠物：${anim}`} style={style}>
      {holiday && (
        <svg viewBox="0 0 120 120" width={size} height={size} style={{ position: "absolute", inset: 0, overflow: "visible" }}>
          <HolidayOverlay id={holiday} asset={holidayAsset} />
        </svg>
      )}
    </div>
  );
}
```

**在 `src/pet/pet.css` 末尾加上**：

```css
.pet-image {
  position: relative;
  background-repeat: no-repeat;
  image-rendering: pixelated; /* 像素画放大后保持清晰；非像素画删掉这一行 */
}
@keyframes pet-frames {
  to { background-position-x: calc(var(--frames) * var(--size) * -1); }
}
```

**替换使用的地方。** 把下面这些文件里的 `<PetSprite ... />` 换成 `<PetImage ... />`（同时改掉文件顶部的 `import`），属性不用改：

- `src/pet/PetWindow.tsx`：桌面上的宠物和来串门的宠物；
- `src/panel/PanelApp.tsx`：面板左上角的小头像、第一次打开时的欢迎页（两处）；
- `src/panel/SettingsView.tsx`：设置页“宠物颜色”旁边的小预览；
- `src/pet/SpriteGallery.tsx`：浏览器里的预览页。

可以用 `grep -rn "<PetSprite" src` 确认没有漏掉的。

最后删掉或改写 `src/__tests__/sprite.test.tsx` 里只适用于 SVG 版的断言（比如查找 `.zzz`、`.calendar-card` 的那几行）。

这段示例代码已经过类型检查，逐帧切换的 CSS 也在 Chromium 里验证过。精灵图里没有 Zzz、感叹号这类小图标时，可以把 `PetSprite.tsx` 里 `zzz` / `exclaim` / `sweat` / `sparkles` 那几段 SVG 复制到 `PetImage` 的 `<svg>` 里一起画。

---

## 三、改方块材质

方块的种类（草地、木板、砖块……）和外观是分开的：种类存在后端，每格 1 字节；外观由前端画。外观有两个来源，**导入的材质优先，没导入的用自带纹理**。

### 1. 从材质包导入（不改代码）

适合用 Minecraft 资源包、网上下载的材质，或者自己画的 PNG。

**操作：** 面板 → 设置 → 桌面建造 → 材质包 → **从文件夹导入…**，选一个文件夹。导入后马上生效，会提示匹配到几种、哪几种没找到（没找到的继续用自带纹理）。点 **恢复自带纹理** 删除全部导入的材质。

**文件名规则。** 程序会在你选的文件夹里（包括所有子文件夹，最多 8 层）按下表找文件，**大小写不限**，每种方块按顺序用找到的第一个名字：

| 方块 | 步步自己的文件名（优先） | 也认这些名字（常见资源包命名） |
| --- | --- | --- |
| 草地 | `grass.png` | `grass_block_side.png` |
| 泥土 | `dirt.png` | `dirt.png` |
| 石头 | `stone.png` | `stone.png` |
| 碎石 | `cobble.png` | `cobblestone.png` |
| 木板 | `planks.png` | `oak_planks.png` |
| 原木 | `log.png` | `oak_log.png` |
| 砖块 | `bricks.png` | `bricks.png` |
| 玻璃 | `glass.png` | `glass.png` |
| 沙子 | `sand.png` | `sand.png` |
| 树叶 | `leaves.png` | `oak_leaves.png` |
| 书架 | `bookshelf.png` | `bookshelf.png` |
| 台灯 | `lamp.png` | `redstone_lamp_on.png`、`glowstone.png` |
| 羊毛 | `wool.png` | `white_wool.png` |
| 花 | `flower.png` | `poppy.png`、`dandelion.png` |
| 床 | `bed.png` | （没有，资源包里的床不是单张方块图） |
| 工作台 | `workbench.png` | `crafting_table_front.png` |

**图片要求：**

- 必须是 PNG，每张不超过 512 KB；不符合的跳过，继续用自带纹理。
- 正方形，任意分辨率（16×16、32×32、64×64 都行），会缩放到一格 24 像素显示，不做模糊处理，像素画保持清晰。
- 竖长条的动画纹理（比如 16×256）只取最上面一帧。
- 灰色的草地和树叶会自动染成绿色（有些资源包的这两种是灰度图，游戏里按地形着色）。
- 透明部分照样透明（比如树叶的空隙、花的背景）。
- 资源包里同名的文件有好几个时，优先用路径里带 `block` 的那个（方块纹理所在目录）。

**用 Minecraft 自己的材质（只在本机）：** 找到游戏版本文件，比如 Windows 上的 `%APPDATA%\.minecraft\versions\<版本号>\<版本号>.jar`。复制一份出来，把扩展名改成 `.zip` 解压，然后选解压出来的文件夹导入。下载的资源包（`.zip`）也一样，先解压再导入。

**只换一两种方块：** 新建一个文件夹，只放你想换的那几张（比如只放 `planks.png`），导入它即可。导入只覆盖这次匹配到的方块，之前导入的其他方块不受影响。

**导入的文件存在哪：** 数据目录下的 `textures/<方块 id>.png`（数据目录见 README 的[数据存放位置](../README.md#数据存放位置)）。也可以直接把文件按这个名字放进去，然后重启步步。

### 2. 修改自带纹理（改代码）

自带纹理在 `src/world/textures.ts` 的 `pattern()` 函数里，每种方块是一段 16×16 的像素生成代码：

- `fill([...颜色])`：用几种颜色随机铺满；
- `set(x, y, 颜色)`：画单个像素，`(0, 0)` 是左上角，颜色写 `null` 表示透明；
- 随机数有固定种子，所以每次生成的结果都一样。

改完在浏览器里打开 `npm run dev` 的地址，预览页的“方块”一栏会显示所有纹理。这里的修改会进仓库，所以只能用原创的图案。

### 3. 新增一种方块（改代码）

以新增“南瓜”为例，要改 3 个文件，**3 个都要改**。漏了第 2 步 Rust 编译会报错；第 3 步漏了 `BLOCK_LABEL` 类型检查会报错，漏了 `pattern()` 测试会失败；但如果整个第 3 步都没做，不会有任何报错，只是建造工具栏里看不到这种方块。

1. **`crates/steppy-core/src/world.rs`**：在 `blocks!` 列表最后加一行：

   ```rust
   Pumpkin = 17, "pumpkin", true;   // 编号、id、宠物能不能站在上面
   ```

   **编号只能往后加，不能改、不能复用。** 存档里存的就是编号，改了会让已有建筑变成别的方块。不想要某种方块了也不要删它那一行，否则老存档里的这种方块会变成空气。

2. **`src-tauri/src/world_rt.rs`**：在 `candidates()` 里加上导入材质包时认的文件名：

   ```rust
   Block::Pumpkin => &["pumpkin_side.png"],
   ```

3. **`src/world/textures.ts`**：
   - `BLOCKS` 数组里加 `"pumpkin"`，它在建造工具栏里的顺序就是数组顺序；
   - `BLOCK_LABEL` 里加 `pumpkin: "南瓜"`；
   - `pattern()` 里加一个 `case "pumpkin":`，画它的自带纹理。

改完运行下面的测试。`world.test.tsx` 会自动检查每种方块都有 16×16 的纹理、名字和图案。

---

## 四、改完之后怎么验证、怎么发版

```bash
npm run dev
```

用浏览器打开它给的地址，能看到宠物的全部动作、所有方块纹理和节日外观。改形象、改纹理时先在这里看效果。

```bash
npm test
```

```bash
cargo test --workspace
```

```bash
npm run tauri dev
```

最后一条会启动真正的桌面程序，用来检查宠物站在任务栏和方块上的位置对不对、点击和拖动是否正常。完整的手动检查项见 README 的[真机手动验收清单](../README.md#真机手动验收清单)。

确认没问题后提交、推送到 GitHub。CI 会在 Windows、macOS、Linux 上跑全部测试并打出安装包，在 Actions 页面对应运行的 Artifacts 里下载。**再强调一次：只能自己用的素材不要提交。**
