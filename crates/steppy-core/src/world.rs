//! 桌面建造带：每块显示器底部一条 12 格高的方块网格。
//!
//! * 只存方块种类（每格 1 字节），一块 1920 宽的屏是 80 × 12 = 960 字节。
//! * 宠物把每列最高的实心方块顶面当地面（见 [`World::heights`] 和 `pet::Screen::terrain`）。
//! * 方块都是原创的种类名；外观由前端绘制（内置像素纹理或用户本机的材质包）。

use base64::Engine;
use serde::{Deserialize, Serialize};

/// 建造带高度（格）。
pub const ROWS: usize = 12;
/// 一格的逻辑像素大小。
pub const CELL: f64 = 24.0;

macro_rules! blocks {
    ($($name:ident = $n:expr, $id:expr, $solid:expr;)*) => {
        /// 方块种类。
        #[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
        #[repr(u8)]
        pub enum Block {
            Air = 0,
            $($name = $n,)*
        }

        impl Block {
            pub const ALL: &'static [Block] = &[$(Block::$name,)*];

            pub fn from_u8(v: u8) -> Block {
                match v {
                    $($n => Block::$name,)*
                    _ => Block::Air,
                }
            }

            pub fn id(self) -> &'static str {
                match self {
                    Block::Air => "air",
                    $(Block::$name => $id,)*
                }
            }

            pub fn from_id(id: &str) -> Option<Block> {
                match id {
                    "air" => Some(Block::Air),
                    $($id => Some(Block::$name),)*
                    _ => None,
                }
            }

            /// 宠物能不能站在上面。
            pub fn solid(self) -> bool {
                match self {
                    Block::Air => false,
                    $(Block::$name => $solid,)*
                }
            }
        }
    };
}

blocks! {
    Grass = 1, "grass", true;
    Dirt = 2, "dirt", true;
    Stone = 3, "stone", true;
    Cobble = 4, "cobble", true;
    Planks = 5, "planks", true;
    Log = 6, "log", true;
    Bricks = 7, "bricks", true;
    Glass = 8, "glass", true;
    Sand = 9, "sand", true;
    Leaves = 10, "leaves", true;
    Bookshelf = 11, "bookshelf", true;
    Lamp = 12, "lamp", true;
    Wool = 13, "wool", true;
    Flower = 14, "flower", false;
    Bed = 15, "bed", true;
    Workbench = 16, "workbench", true;
    Water = 17, "water", false;
}

impl Serialize for Block {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        s.serialize_str(self.id())
    }
}

impl<'de> Deserialize<'de> for Block {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let s = String::deserialize(d)?;
        Block::from_id(&s).ok_or_else(|| serde::de::Error::custom(format!("unknown block {s}")))
    }
}

/// 一块屏的建造带。第 0 行在最下面。
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct World {
    cols: usize,
    cells: Vec<u8>,
}

/// 持久化 / 传给前端的格式：`cells` 是逐行（从下往上）的方块编号，base64 编码。
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct WorldData {
    pub cols: usize,
    pub rows: usize,
    pub cells: String,
}

impl World {
    pub fn new(cols: usize) -> World {
        World {
            cols,
            cells: vec![0; cols * ROWS],
        }
    }

    pub fn cols(&self) -> usize {
        self.cols
    }

    pub fn get(&self, x: usize, y: usize) -> Block {
        if x >= self.cols || y >= ROWS {
            return Block::Air;
        }
        Block::from_u8(self.cells[y * self.cols + x])
    }

    /// 放置或拆除（`Block::Air`）。返回是否有变化。
    pub fn set(&mut self, x: usize, y: usize, b: Block) -> bool {
        if x >= self.cols || y >= ROWS {
            return false;
        }
        let i = y * self.cols + x;
        let changed = self.cells[i] != b as u8;
        self.cells[i] = b as u8;
        changed
    }

    pub fn is_empty(&self) -> bool {
        self.cells.iter().all(|c| *c == 0)
    }

    pub fn clear(&mut self) {
        self.cells.iter_mut().for_each(|c| *c = 0);
    }

    /// 屏幕宽度变了（分辨率 / 缩放改变）：保留左边能放下的部分。
    pub fn resized(&self, cols: usize) -> World {
        let mut w = World::new(cols);
        for y in 0..ROWS {
            for x in 0..cols.min(self.cols) {
                w.cells[y * cols + x] = self.cells[y * self.cols + x];
            }
        }
        w
    }

    /// 每列的地面高度：最高的实心方块的顶面（格）。
    pub fn heights(&self) -> Vec<u8> {
        (0..self.cols)
            .map(|x| {
                (0..ROWS)
                    .rev()
                    .find(|&y| self.get(x, y).solid())
                    .map(|y| (y + 1) as u8)
                    .unwrap_or(0)
            })
            .collect()
    }

    pub fn to_data(&self) -> WorldData {
        WorldData {
            cols: self.cols,
            rows: ROWS,
            cells: base64::engine::general_purpose::STANDARD.encode(&self.cells),
        }
    }

    /// 从持久化格式恢复；未知的方块编号当作空气，尺寸不符时按 `cols` 裁剪 / 补齐。
    pub fn from_data(d: &WorldData) -> Option<World> {
        let bytes = base64::engine::general_purpose::STANDARD
            .decode(&d.cells)
            .ok()?;
        if d.rows == 0 || bytes.len() != d.cols * d.rows {
            return None;
        }
        let mut w = World::new(d.cols);
        for y in 0..d.rows.min(ROWS) {
            for x in 0..d.cols {
                w.cells[y * d.cols + x] = Block::from_u8(bytes[y * d.cols + x]) as u8;
            }
        }
        Some(w)
    }
}

/// 显示器的 key 是“名字@位置”。调整显示器排列、拔插外接屏后位置会变，key 就对不上了。
/// 给每块新 key 的显示器找一份“没人认领”的旧存档接管：先找同名的，再找列数相同的。
///
/// `saved` 是存档里所有非空世界的 (key, 列数)；`known` 是存档里出现过的全部 key（包括清空了的，
/// 清空过的屏不会去接管别人的存档）；`current` 是现在接着的显示器的 (key, 列数)。
/// 返回要改名的 (旧 key, 新 key)。
pub fn adopt_orphans(
    saved: &[(String, usize)],
    known: &[String],
    current: &[(String, usize)],
) -> Vec<(String, String)> {
    let name = |k: &str| k.split('@').next().unwrap_or("").to_string();
    let is_current = |k: &str| current.iter().any(|(c, _)| c == k);
    let mut orphans: Vec<&(String, usize)> = saved.iter().filter(|(k, _)| !is_current(k)).collect();
    let mut moves = Vec::new();
    let newcomers: Vec<&(String, usize)> = current
        .iter()
        .filter(|(k, _)| !saved.iter().any(|(s, _)| s == k) && !known.contains(k))
        .collect();
    // 两轮：先按名字配对，再按列数配对，避免列数相同的别的屏抢走同名屏的存档。
    for by_name in [true, false] {
        for (key, cols) in &newcomers {
            if moves.iter().any(|(_, to): &(String, String)| to == key) {
                continue;
            }
            let pos = orphans.iter().position(|(k, c)| {
                if by_name {
                    key.contains('@') && name(k) == name(key)
                } else {
                    c == cols
                }
            });
            if let Some(i) = pos {
                moves.push((orphans.remove(i).0.clone(), key.clone()));
            }
        }
    }
    moves
}

/// 某块屏能放多少列。
pub fn cols_for(logical_width: f64) -> usize {
    (logical_width / CELL).floor().max(1.0) as usize
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn set_get_and_heights() {
        let mut w = World::new(10);
        assert!(w.is_empty());
        assert!(w.set(2, 0, Block::Grass));
        assert!(!w.set(2, 0, Block::Grass), "重复放置没有变化");
        w.set(2, 1, Block::Planks);
        w.set(5, 4, Block::Bricks); // 悬空的方块也算地面
        w.set(7, 0, Block::Flower); // 花不是实心的
        assert_eq!(w.heights(), vec![0, 0, 2, 0, 0, 5, 0, 0, 0, 0]);
        assert!(!w.set(10, 0, Block::Dirt), "越界不放");
        assert!(!w.set(0, ROWS, Block::Dirt));
        assert_eq!(w.get(99, 99), Block::Air);
        w.set(2, 1, Block::Air);
        assert_eq!(w.heights()[2], 1);
        w.clear();
        assert!(w.is_empty());
    }

    #[test]
    fn roundtrip_and_resize() {
        let mut w = World::new(80);
        for (i, b) in Block::ALL.iter().enumerate() {
            w.set(i, i % ROWS, *b);
        }
        let d = w.to_data();
        assert_eq!(d.rows, ROWS);
        assert_eq!(World::from_data(&d), Some(w.clone()));
        let small = w.resized(5);
        assert_eq!(small.cols(), 5);
        assert_eq!(small.get(4, 4), Block::ALL[4]);
        let big = small.resized(20);
        assert_eq!(big.get(4, 4), Block::ALL[4]);
        assert_eq!(big.get(19, 0), Block::Air);
        // 坏数据不恢复。
        assert!(World::from_data(&WorldData {
            cols: 3,
            rows: ROWS,
            cells: "AAAA".into()
        })
        .is_none());
        assert!(World::from_data(&WorldData {
            cols: 3,
            rows: ROWS,
            cells: "!!".into()
        })
        .is_none());
    }

    #[test]
    fn ids_roundtrip() {
        for b in Block::ALL {
            assert_eq!(Block::from_id(b.id()), Some(*b));
            assert_eq!(Block::from_u8(*b as u8), *b);
            let json = serde_json::to_string(b).unwrap();
            assert_eq!(serde_json::from_str::<Block>(&json).unwrap(), *b);
        }
        assert_eq!(Block::from_u8(250), Block::Air);
        assert_eq!(Block::from_id("nope"), None);
        assert_eq!(cols_for(1920.0), 80);
        assert_eq!(cols_for(1366.0), 56);
    }

    #[test]
    fn adopts_saved_world_after_monitor_rearrange() {
        let saved = vec![
            ("DISPLAY1@0x0".to_string(), 80),
            ("DISPLAY2@1920x0".to_string(), 106),
        ];
        // 外接屏挪到左边：两块屏的位置都变了，按名字各自认回自己的存档。
        let cur = vec![
            ("DISPLAY2@-2560x0".to_string(), 106),
            ("DISPLAY1@0x0".to_string(), 80),
        ];
        assert_eq!(
            adopt_orphans(&saved, &[], &cur),
            vec![(
                "DISPLAY2@1920x0".to_string(),
                "DISPLAY2@-2560x0".to_string()
            )]
        );
        // 名字也变了（比如换了接口）：按列数认领。
        let cur = vec![
            ("DISPLAY3@1920x0".to_string(), 106),
            ("DISPLAY1@0x0".to_string(), 80),
        ];
        assert_eq!(
            adopt_orphans(&saved, &[], &cur),
            vec![("DISPLAY2@1920x0".to_string(), "DISPLAY3@1920x0".to_string())]
        );
        // 拔掉外接屏：笔记本自己的存档不受影响，外接屏的存档留着等它回来。
        let cur = vec![("DISPLAY1@0x0".to_string(), 80)];
        assert!(adopt_orphans(&saved, &[], &cur).is_empty());
        // 没有名字的显示器（key 只有位置）不按名字配对。
        let saved = vec![("0x0".to_string(), 80)];
        let cur = vec![("100x0".to_string(), 50)];
        assert!(adopt_orphans(&saved, &[], &cur).is_empty());
        // 清空过的屏（存档里有它的 key）不接管别人的存档。
        let saved = vec![("A@0x0".to_string(), 80)];
        let cur = vec![("B@1920x0".to_string(), 80)];
        assert!(adopt_orphans(&saved, &["B@1920x0".to_string()], &cur).is_empty());
        // 一份存档只给一块屏。
        let saved = vec![("A@0x0".to_string(), 80)];
        let cur = vec![("B@0x0".to_string(), 80), ("C@1920x0".to_string(), 80)];
        assert_eq!(adopt_orphans(&saved, &[], &cur).len(), 1);
    }

    #[test]
    fn tiny_memory_footprint() {
        // 一块 4K 屏（逻辑宽度 3840）也只有 160 × 12 = 1920 字节。
        let w = World::new(cols_for(3840.0));
        assert_eq!(w.cells.len(), 1920);
        assert!(w.to_data().cells.len() < 3000);
    }
}
