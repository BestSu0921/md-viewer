#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use serde::Serialize;
use std::fs;
use std::path::{Path, PathBuf};
use tauri::{Emitter, Manager};

const SKIP_DIRS: &[&str] = &[
    ".git", ".svn", ".hg", ".idea", ".vscode", ".github", ".gitlab",
    "node_modules", "target", "dist", "build", "out", ".next", ".nuxt",
    "__pycache__", "venv", ".venv", ".gradle", ".cargo", ".cache",
    "$recycle.bin", "system volume information",
];
const MAX_NODES: usize = 8000;
const MAX_DEPTH: u32 = 15;

#[derive(Serialize)]
struct TreeNode {
    name: String,
    path: String,
    dir: bool,
    children: Vec<TreeNode>,
}

struct Budget(usize);

fn skip_dir(name: &str) -> bool {
    let lower = name.to_lowercase();
    name.starts_with('.') && lower != ".obsidian" || SKIP_DIRS.contains(&lower.as_str())
}

fn build_tree(dir: &Path, depth: u32, budget: &mut Budget) -> TreeNode {
    let node = TreeNode {
        name: dir
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| dir.to_string_lossy().into_owned()),
        path: dir.to_string_lossy().into_owned(),
        dir: true,
        children: Vec::new(),
    };
    if depth >= MAX_DEPTH || budget.0 == 0 {
        return node;
    }
    let mut entries: Vec<(bool, String, PathBuf)> = Vec::new();
    if let Ok(rd) = fs::read_dir(dir) {
        for e in rd.flatten() {
            let p = e.path();
            let name = e.file_name().to_string_lossy().into_owned();
            let is_dir = p.is_dir();
            if is_dir && skip_dir(&name) {
                continue;
            }
            if !is_dir && !is_markdown(&name) {
                continue;
            }
            entries.push((is_dir, name, p));
        }
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0).then_with(|| a.1.to_lowercase().cmp(&b.1.to_lowercase())));
    let mut children = Vec::new();
    for (_, _, p) in entries {
        if budget.0 == 0 {
            break;
        }
        budget.0 -= 1;
        if p.is_dir() {
            children.push(build_tree(&p, depth + 1, budget));
        } else {
            children.push(TreeNode {
                name: p.file_name().map(|s| s.to_string_lossy().into_owned()).unwrap_or_default(),
                path: p.to_string_lossy().into_owned(),
                dir: false,
                children: Vec::new(),
            });
        }
    }
    TreeNode { children, ..node }
}

fn is_markdown(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.ends_with(".md") || lower.ends_with(".markdown")
}

#[tauri::command]
fn list_tree(root: String) -> Result<TreeNode, String> {
    let p = PathBuf::from(&root);
    if !p.is_dir() {
        return Err("不是有效的文件夹".into());
    }
    let mut budget = Budget(MAX_NODES);
    Ok(build_tree(&p, 0, &mut budget))
}

#[tauri::command]
fn read_text_file(path: String) -> Result<String, String> {
    let bytes = fs::read(&path).map_err(|e| format!("读取失败：{e}"))?;
    let bytes = if bytes.starts_with(&[0xEF, 0xBB, 0xBF]) {
        bytes[3..].to_vec()
    } else {
        bytes
    };
    match String::from_utf8(bytes.clone()) {
        Ok(s) => Ok(s),
        Err(_) => {
            let (decoded, _, had_errors) = encoding_rs::GBK.decode(&bytes);
            if had_errors {
                Err("文件既不是 UTF-8 也无法按 GBK 解码".into())
            } else {
                Ok(decoded.into_owned())
            }
        }
    }
}

#[tauri::command]
fn write_text_file(path: String, content: String) -> Result<(), String> {
    fs::write(&path, content).map_err(|e| format!("保存失败：{e}"))
}

// canonicalize 在 Windows 会给出 \\?\C:\... 扩展前缀，剥掉以免和对话框路径对不上
fn canon(p: &Path) -> PathBuf {
    let c = fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
    let s = c.to_string_lossy();
    if let Some(rest) = s.strip_prefix(r"\\?\") {
        if rest.len() > 1 && rest.as_bytes()[1] == b':' {
            return PathBuf::from(rest);
        }
    }
    c
}

#[tauri::command]
fn startup_file() -> Option<String> {
    std::env::args()
        .nth(1)
        .filter(|p| Path::new(p).is_file())
        .map(|p| canon(Path::new(&p)).to_string_lossy().into_owned())
}

fn main() {
    tauri::Builder::default()
        .plugin(tauri_plugin_single_instance::init(|app, argv, _| {
            if let Some(p) = argv.iter().skip(1).find(|p| Path::new(p).is_file()) {
                let _ = app.emit("open-path", canon(Path::new(p)).to_string_lossy().into_owned());
            }
            if let Some(w) = app.get_webview_window("main") {
                let _ = w.unminimize();
                let _ = w.set_focus();
            }
        }))
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_opener::init())
        .invoke_handler(tauri::generate_handler![
            list_tree,
            read_text_file,
            write_text_file,
            startup_file
        ])
        .run(tauri::generate_context!())
        .expect("error while running md-viewer");
}
