//! 桌面建造带：每块显示器底部的方块世界、建造模式、本机材质包。
//!
//! 内存策略：只有某块屏上已经有方块、或者打开了建造模式时，才为它创建一个透明长条窗口；
//! 没有方块时窗口会被销毁，不建造就完全不占内存。方块本身每格 1 字节。

use std::collections::{BTreeMap, HashMap};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;

use base64::Engine;
use serde::Serialize;
use steppy_core::world::{Block, World, WorldData, CELL, ROWS};

use crate::storage;

static WORLDS: Mutex<Option<BTreeMap<String, World>>> = Mutex::new(None);
/// 窗口 label → 显示器 key。
static STRIPS: Mutex<Option<HashMap<String, String>>> = Mutex::new(None);
static BUILD_MODE: AtomicBool = AtomicBool::new(false);
/// 方块或建造模式每变一次加一，宠物引擎据此刷新地形和窗口。
static VERSION: AtomicU64 = AtomicU64::new(1);

fn with_worlds<T>(f: impl FnOnce(&mut BTreeMap<String, World>) -> T) -> T {
    let mut g = WORLDS.lock().unwrap();
    let w = g.get_or_insert_with(|| {
        let saved: BTreeMap<String, WorldData> =
            storage::read_json(&storage::path("world.json")).unwrap_or_default();
        saved
            .iter()
            .filter_map(|(k, d)| World::from_data(d).map(|w| (k.clone(), w)))
            .collect()
    });
    f(w)
}

fn save(worlds: &BTreeMap<String, World>) {
    let data: BTreeMap<String, WorldData> = worlds
        .iter()
        .filter(|(_, w)| !w.is_empty())
        .map(|(k, w)| (k.clone(), w.to_data()))
        .collect();
    if let Err(e) = storage::write_json(&storage::path("world.json"), &data, false) {
        log::error!("保存建造带失败：{e}");
    }
}

pub fn version() -> u64 {
    VERSION.load(Ordering::Acquire)
}

fn bump() {
    VERSION.fetch_add(1, Ordering::AcqRel);
}

pub fn build_mode() -> bool {
    BUILD_MODE.load(Ordering::Acquire)
}

pub fn set_build_mode(on: bool) {
    BUILD_MODE.store(on, Ordering::Release);
    bump();
}

/// 某块显示器的世界（列数按当前屏宽调整）。
pub fn world_for(key: &str, cols: usize) -> World {
    with_worlds(|ws| {
        let w = ws
            .entry(key.to_string())
            .or_insert_with(|| World::new(cols));
        if w.cols() != cols {
            *w = w.resized(cols);
        }
        w.clone()
    })
}

/// 这块屏需不需要建造带窗口。
pub fn needs_strip(key: &str) -> bool {
    build_mode() || with_worlds(|ws| ws.get(key).map(|w| !w.is_empty()).unwrap_or(false))
}

pub fn register_strip(label: &str, key: &str) {
    STRIPS
        .lock()
        .unwrap()
        .get_or_insert_with(HashMap::new)
        .insert(label.to_string(), key.to_string());
}

pub fn unregister_strip(label: &str) {
    if let Some(m) = STRIPS.lock().unwrap().as_mut() {
        m.remove(label);
    }
}

fn key_of(label: &str) -> Option<String> {
    STRIPS
        .lock()
        .unwrap()
        .as_ref()
        .and_then(|m| m.get(label).cloned())
}

/// 前端绘制用的数据。
#[derive(Debug, Clone, Serialize)]
pub struct StripView {
    pub key: String,
    pub cols: usize,
    pub rows: usize,
    pub cell: f64,
    /// 从下往上逐行的方块 id。
    pub cells: Vec<&'static str>,
    pub build_mode: bool,
}

pub fn view(label: &str) -> Option<StripView> {
    let key = key_of(label)?;
    let w = with_worlds(|ws| ws.get(&key).cloned())?;
    let mut cells = Vec::with_capacity(w.cols() * ROWS);
    for y in 0..ROWS {
        for x in 0..w.cols() {
            cells.push(w.get(x, y).id());
        }
    }
    Some(StripView {
        key,
        cols: w.cols(),
        rows: ROWS,
        cell: CELL,
        cells,
        build_mode: build_mode(),
    })
}

/// 放置 / 拆除。`block` 为 "air" 表示拆除。
pub fn set_block(label: &str, x: usize, y: usize, block: &str) -> anyhow::Result<bool> {
    let key = key_of(label).ok_or_else(|| anyhow::anyhow!("未知的建造带窗口"))?;
    let b = Block::from_id(block).ok_or_else(|| anyhow::anyhow!("未知的方块：{block}"))?;
    let changed = with_worlds(|ws| {
        let w = ws.get_mut(&key)?;
        let changed = w.set(x, y, b);
        if changed {
            save(ws);
        }
        Some(changed)
    })
    .ok_or_else(|| anyhow::anyhow!("这块屏还没有建造带"))?;
    if changed {
        bump();
    }
    Ok(changed)
}

/// 清空某块屏（或全部）。
pub fn clear(label: Option<&str>) {
    let key = label.and_then(key_of);
    with_worlds(|ws| {
        for (k, w) in ws.iter_mut() {
            if key.as_deref().map(|x| x == k).unwrap_or(true) {
                w.clear();
            }
        }
        save(ws);
    });
    bump();
}

pub fn block_count() -> usize {
    with_worlds(|ws| {
        ws.values()
            .map(|w| {
                (0..ROWS)
                    .flat_map(|y| (0..w.cols()).map(move |x| (x, y)))
                    .filter(|&(x, y)| w.get(x, y) != Block::Air)
                    .count()
            })
            .sum()
    })
}

// ─────────────────────────── 本机材质包 ───────────────────────────
//
// 用户可以从自己电脑上的材质包文件夹导入纹理（比如自己游戏里的资源包）。文件只复制到本机的
// 数据目录，不会打包进步步、不会同步到其他电脑、也不会上传到任何地方。

fn texture_dir() -> PathBuf {
    storage::data_dir().join("textures")
}

/// 每种方块按顺序尝试的文件名：先找步步自己的命名（`<方块 id>.png`），再找常见资源包的命名。
fn candidates(b: Block) -> Vec<String> {
    let mut v = vec![format!("{}.png", b.id())];
    let extra: &[&str] = match b {
        Block::Grass => &["grass_block_side.png"],
        Block::Dirt => &["dirt.png"],
        Block::Stone => &["stone.png"],
        Block::Cobble => &["cobblestone.png"],
        Block::Planks => &["oak_planks.png"],
        Block::Log => &["oak_log.png"],
        Block::Bricks => &["bricks.png"],
        Block::Glass => &["glass.png"],
        Block::Sand => &["sand.png"],
        Block::Leaves => &["oak_leaves.png"],
        Block::Bookshelf => &["bookshelf.png"],
        Block::Lamp => &["redstone_lamp_on.png", "glowstone.png"],
        Block::Wool => &["white_wool.png"],
        Block::Flower => &["poppy.png", "dandelion.png"],
        Block::Workbench => &["crafting_table_front.png"],
        Block::Bed | Block::Air => &[],
    };
    v.extend(extra.iter().map(|s| s.to_string()));
    v
}

fn walk(dir: &Path, depth: usize, out: &mut HashMap<String, PathBuf>) {
    if depth > 8 || out.len() > 20_000 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk(&p, depth + 1, out);
        } else if let Some(name) = p.file_name().and_then(|n| n.to_str()) {
            let lower = name.to_ascii_lowercase();
            if lower.ends_with(".png") {
                // 同名文件优先取路径里带 "block" 的（资源包里方块纹理所在的目录）。
                let prefer = p.to_string_lossy().contains("block");
                if prefer || !out.contains_key(&lower) {
                    out.insert(lower, p);
                }
            }
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct ImportResult {
    pub matched: Vec<String>,
    pub missing: Vec<String>,
}

/// 从文件夹导入。只接受 PNG、每张不超过 512 KB。
pub fn import_pack(folder: &Path) -> anyhow::Result<ImportResult> {
    let mut files = HashMap::new();
    walk(folder, 0, &mut files);
    let dir = texture_dir();
    std::fs::create_dir_all(&dir)?;
    let mut matched = Vec::new();
    let mut missing = Vec::new();
    for b in Block::ALL {
        let found = candidates(*b)
            .into_iter()
            .find_map(|c| files.get(&c).cloned());
        match found {
            Some(src) => {
                let bytes = std::fs::read(&src)?;
                if bytes.len() > 512 * 1024 || !bytes.starts_with(b"\x89PNG") {
                    missing.push(b.id().to_string());
                    continue;
                }
                std::fs::write(dir.join(format!("{}.png", b.id())), bytes)?;
                matched.push(b.id().to_string());
            }
            None => missing.push(b.id().to_string()),
        }
    }
    bump();
    Ok(ImportResult { matched, missing })
}

pub fn clear_pack() {
    let _ = std::fs::remove_dir_all(texture_dir());
    bump();
}

/// 已导入的纹理：方块 id → data URL。
pub fn textures() -> HashMap<String, String> {
    let mut out = HashMap::new();
    for b in Block::ALL {
        let p = texture_dir().join(format!("{}.png", b.id()));
        if let Ok(bytes) = std::fs::read(&p) {
            out.insert(
                b.id().to_string(),
                format!(
                    "data:image/png;base64,{}",
                    base64::engine::general_purpose::STANDARD.encode(bytes)
                ),
            );
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn candidate_names() {
        assert_eq!(
            candidates(Block::Planks),
            vec!["planks.png".to_string(), "oak_planks.png".to_string()]
        );
        assert_eq!(candidates(Block::Bed), vec!["bed.png".to_string()]);
    }

    #[test]
    fn walks_nested_resource_pack_layout() {
        let dir = std::env::temp_dir().join(format!("steppy-pack-{}", std::process::id()));
        let block_dir = dir.join("assets/x/textures/block");
        std::fs::create_dir_all(&block_dir).unwrap();
        std::fs::create_dir_all(dir.join("assets/x/textures/item")).unwrap();
        std::fs::write(block_dir.join("Oak_Planks.png"), b"\x89PNG....").unwrap();
        std::fs::write(
            dir.join("assets/x/textures/item/oak_planks.png"),
            b"\x89PNG....",
        )
        .unwrap();
        let mut files = HashMap::new();
        walk(&dir, 0, &mut files);
        let p = files.get("oak_planks.png").unwrap();
        assert!(p.to_string_lossy().contains("block"), "{}", p.display());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
