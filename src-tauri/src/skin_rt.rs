//! 导入的宠物形象（Codex 宠物格式，见 `steppy_core::skin`）。
//!
//! 导入时把精灵图复制到本机数据目录 `pets/<id>/`，只在这台电脑上用：不打包、不同步、不上传。
//! 来串门的别人家宠物仍然显示成默认的步步（用对方设置的颜色）。

use std::path::{Path, PathBuf};

use base64::Engine;
use serde::{Deserialize, Serialize};
use steppy_core::skin::{self, Atlas, ImageFormat};

use crate::storage;

fn skins_dir() -> PathBuf {
    storage::data_dir().join("pets")
}

/// 每个已导入形象的说明文件（`pets/<id>/skin.json`）。
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SkinInfo {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    pub atlas: Atlas,
    /// 精灵图文件名（`sheet.webp` / `sheet.png`）。
    pub file: String,
    /// 从哪个文件夹导入的（仅供显示）。
    #[serde(default)]
    pub source: String,
}

/// 在一个文件夹里找精灵图：先找 `spritesheet.webp` / `spritesheet.png`，
/// 再找文件夹里唯一一张尺寸符合图集规格的 PNG / WebP（方便自己画的图不用改名）。
fn find_sheet(folder: &Path) -> Result<(PathBuf, Vec<u8>, ImageFormat, Atlas), String> {
    let read_ok = |p: &Path| -> Option<(Vec<u8>, ImageFormat, Atlas)> {
        let meta = std::fs::metadata(p).ok()?;
        if !meta.is_file() || meta.len() as usize > skin::MAX_SHEET_BYTES {
            return None;
        }
        let bytes = std::fs::read(p).ok()?;
        let (fmt, w, h) = skin::image_size(&bytes)?;
        let atlas = skin::detect_atlas(w, h)?;
        Some((bytes, fmt, atlas))
    };
    for name in ["spritesheet.webp", "spritesheet.png"] {
        let p = folder.join(name);
        if p.exists() {
            let meta = std::fs::metadata(&p).map_err(|e| e.to_string())?;
            if meta.len() as usize > skin::MAX_SHEET_BYTES {
                return Err(format!("{name} 太大了（上限 16 MB）"));
            }
            let bytes = std::fs::read(&p).map_err(|e| e.to_string())?;
            let (fmt, w, h) =
                skin::image_size(&bytes).ok_or(format!("{name} 不是有效的 PNG / WebP 图片"))?;
            let atlas = skin::detect_atlas(w, h).ok_or(format!(
                "{name} 的尺寸是 {w} × {h}，不是 Codex 宠物图集（8 列 × 9 行或 11 行，每格 192 × 208，可等比缩放）"
            ))?;
            return Ok((p, bytes, fmt, atlas));
        }
    }
    let mut found = Vec::new();
    if let Ok(rd) = std::fs::read_dir(folder) {
        for e in rd.flatten() {
            let p = e.path();
            let ext = p
                .extension()
                .and_then(|x| x.to_str())
                .unwrap_or("")
                .to_ascii_lowercase();
            if ext == "png" || ext == "webp" {
                if let Some((bytes, fmt, atlas)) = read_ok(&p) {
                    found.push((p, bytes, fmt, atlas));
                }
            }
        }
    }
    match found.len() {
        1 => Ok(found.pop().unwrap()),
        0 => Err("这个文件夹里没有找到精灵图（spritesheet.webp / spritesheet.png，8 列 × 9 行、每格 192 × 208）".into()),
        _ => Err("这个文件夹里有好几张符合规格的图片，请把要用的那张改名为 spritesheet.png".into()),
    }
}

/// 从文件夹导入。已经导入过同一个 id 的会被更新。
pub fn import(folder: &Path) -> Result<SkinInfo, String> {
    if !folder.is_dir() {
        return Err("请选择一个文件夹（压缩包请先解压）".into());
    }
    let meta = match std::fs::read(folder.join("pet.json")) {
        Ok(b) if b.len() <= skin::MAX_META_BYTES => {
            skin::parse_pet_json(&String::from_utf8_lossy(&b))?
        }
        Ok(_) => return Err("pet.json 太大了".into()),
        Err(_) => skin::PetMeta::default(),
    };
    let (src, bytes, fmt, atlas) = find_sheet(folder)?;
    let folder_name = folder
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("pet")
        .to_string();
    let id = skin::slugify(meta.id.as_deref().unwrap_or(&folder_name));
    let name = meta
        .name
        .clone()
        .unwrap_or_else(|| folder_name.chars().take(40).collect());
    let dir = skins_dir().join(&id);
    // 先写到临时目录再改名，导入一半失败不会留下坏掉的形象。
    let tmp = skins_dir().join(format!(".{id}.tmp"));
    let _ = std::fs::remove_dir_all(&tmp);
    std::fs::create_dir_all(&tmp).map_err(|e| e.to_string())?;
    let file = format!("sheet.{}", fmt.ext());
    let info = SkinInfo {
        id: id.clone(),
        name,
        description: meta.description,
        atlas,
        file: file.clone(),
        source: src.parent().unwrap_or(folder).display().to_string(),
    };
    let write = || -> std::io::Result<()> {
        std::fs::write(tmp.join(&file), &bytes)?;
        std::fs::write(tmp.join("skin.json"), serde_json::to_vec_pretty(&info)?)?;
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::rename(&tmp, &dir)
    };
    if let Err(e) = write() {
        let _ = std::fs::remove_dir_all(&tmp);
        return Err(format!("保存形象失败：{e}"));
    }
    log::info!("导入形象 {id}（{}，v{}）", info.name, atlas.version);
    Ok(info)
}

pub fn get(id: &str) -> Option<SkinInfo> {
    if !skin::valid_id(id) {
        return None;
    }
    let text = std::fs::read_to_string(skins_dir().join(id).join("skin.json")).ok()?;
    let info: SkinInfo = serde_json::from_str(&text).ok()?;
    (info.id == id && skins_dir().join(id).join(&info.file).is_file()).then_some(info)
}

pub fn list() -> Vec<SkinInfo> {
    let mut out: Vec<SkinInfo> = std::fs::read_dir(skins_dir())
        .map(|rd| {
            rd.flatten()
                .filter_map(|e| e.file_name().to_str().and_then(get))
                .collect()
        })
        .unwrap_or_default();
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

pub fn delete(id: &str) -> Result<(), String> {
    if !skin::valid_id(id) {
        return Err("无效的形象".into());
    }
    std::fs::remove_dir_all(skins_dir().join(id)).map_err(|e| e.to_string())
}

/// 前端绘制用：形象信息 + 精灵图 data URL。
#[derive(Debug, Clone, Serialize)]
pub struct SkinSheet {
    #[serde(flatten)]
    pub info: SkinInfo,
    pub url: String,
}

pub fn sheet(id: &str) -> Option<SkinSheet> {
    let info = get(id)?;
    let bytes = std::fs::read(skins_dir().join(id).join(&info.file)).ok()?;
    let (fmt, _, _) = skin::image_size(&bytes)?;
    let url = format!(
        "data:{};base64,{}",
        fmt.mime(),
        base64::engine::general_purpose::STANDARD.encode(bytes)
    );
    Some(SkinSheet { info, url })
}

/// 本机已经装好的 Codex / Petdex 宠物（`~/.codex/pets/*`、`~/.petdex/pets/*`），可以一键导入。
#[derive(Debug, Clone, Serialize, PartialEq)]
pub struct FoundPet {
    pub path: String,
    pub id: String,
    pub name: String,
    pub imported: bool,
}

pub fn discover() -> Vec<FoundPet> {
    let Some(home) = dirs::home_dir() else {
        return Vec::new();
    };
    discover_in(&[
        home.join(".codex").join("pets"),
        home.join(".petdex").join("pets"),
    ])
}

fn discover_in(roots: &[PathBuf]) -> Vec<FoundPet> {
    let mut out: Vec<FoundPet> = Vec::new();
    for root in roots {
        let Ok(rd) = std::fs::read_dir(root) else {
            continue;
        };
        for e in rd.flatten() {
            let p = e.path();
            if !p.is_dir() || !p.join("pet.json").is_file() {
                continue;
            }
            if !p.join("spritesheet.webp").is_file() && !p.join("spritesheet.png").is_file() {
                continue;
            }
            let folder = p
                .file_name()
                .and_then(|n| n.to_str())
                .unwrap_or("pet")
                .to_string();
            let meta = std::fs::read_to_string(p.join("pet.json"))
                .ok()
                .filter(|t| t.len() <= skin::MAX_META_BYTES)
                .and_then(|t| skin::parse_pet_json(&t).ok())
                .unwrap_or_default();
            let id = skin::slugify(meta.id.as_deref().unwrap_or(&folder));
            if out.iter().any(|f| f.id == id) {
                continue;
            }
            out.push(FoundPet {
                path: p.display().to_string(),
                imported: get(&id).is_some(),
                name: meta.name.unwrap_or(folder),
                id,
            });
        }
    }
    out.sort_by_key(|a| a.name.to_lowercase());
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let mut b = b"\x89PNG\r\n\x1a\n\0\0\0\x0dIHDR".to_vec();
        b.extend(w.to_be_bytes());
        b.extend(h.to_be_bytes());
        b.extend([8, 6, 0, 0, 0]);
        b
    }

    #[test]
    fn finds_sheet_by_name_or_by_size() {
        let dir = tempfile::tempdir().unwrap();
        let d = dir.path();
        assert!(find_sheet(d).unwrap_err().contains("没有找到精灵图"));
        std::fs::write(d.join("notes.png"), png(64, 64)).unwrap();
        std::fs::write(d.join("my-cat.png"), png(768, 936)).unwrap();
        let (p, _, fmt, atlas) = find_sheet(d).unwrap();
        assert!(p.ends_with("my-cat.png"));
        assert_eq!((fmt, atlas.version), (ImageFormat::Png, 1));
        std::fs::write(d.join("other.png"), png(1536, 2288)).unwrap();
        assert!(find_sheet(d).unwrap_err().contains("好几张"));
        // 有 spritesheet.png 时只认它，尺寸不对要说清楚。
        std::fs::write(d.join("spritesheet.png"), png(100, 100)).unwrap();
        assert!(find_sheet(d).unwrap_err().contains("100 × 100"));
        std::fs::write(d.join("spritesheet.png"), png(1536, 2288)).unwrap();
        assert_eq!(find_sheet(d).unwrap().3.version, 2);
    }

    #[test]
    fn discovers_installed_codex_pets() {
        let dir = tempfile::tempdir().unwrap();
        let codex = dir.path().join("codex");
        let petdex = dir.path().join("petdex");
        for (root, slug, name) in [
            (&codex, "boba", "Boba"),
            (&petdex, "boba", "Boba"),
            (&petdex, "steve-4", "Steve"),
        ] {
            let p = root.join(slug);
            std::fs::create_dir_all(&p).unwrap();
            std::fs::write(
                p.join("pet.json"),
                format!(r#"{{"id":"{slug}","displayName":"{name}"}}"#),
            )
            .unwrap();
            std::fs::write(p.join("spritesheet.webp"), b"RIFF").unwrap();
        }
        // 缺精灵图的文件夹不算。
        std::fs::create_dir_all(codex.join("broken")).unwrap();
        std::fs::write(codex.join("broken/pet.json"), "{}").unwrap();
        let found = discover_in(&[codex, petdex, dir.path().join("missing")]);
        let names: Vec<_> = found
            .iter()
            .map(|f| (f.id.as_str(), f.name.as_str()))
            .collect();
        assert_eq!(
            names,
            vec![("boba", "Boba"), ("steve-4", "Steve")],
            "同一个宠物只列一次"
        );
    }
}
