// Hide the console window on Windows release builds.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod markdown;
mod store;

use eframe::egui;
use std::sync::Arc;

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("nimblenote")
            .with_app_id("nimblenote-rs")
            .with_inner_size([780.0, 620.0])
            .with_min_inner_size([420.0, 320.0])
            .with_icon(Arc::new(app_icon())),
        persist_window: true,
        ..Default::default()
    };
    eframe::run_native(
        "nimblenote-rs",
        options,
        Box::new(|cc| {
            install_fonts(&cc.egui_ctx);
            Ok(Box::new(app::NimbleApp::new(cc)))
        }),
    )
}

/// Use the native Windows UI fonts when they exist (Segoe UI + Cascadia/Consolas),
/// keeping egui's bundled fonts as fallbacks for emoji and missing glyphs.
fn install_fonts(ctx: &egui::Context) {
    let mut fonts = egui::FontDefinitions::default();
    let font_dirs = font_dirs();
    let load = |names: &[&str]| -> Option<Vec<u8>> {
        for dir in &font_dirs {
            for n in names {
                if let Ok(bytes) = std::fs::read(dir.join(n)) {
                    return Some(bytes);
                }
            }
        }
        None
    };
    if let Some(b) = load(&["segoeui.ttf", "SegoeUI.ttf"]) {
        fonts.font_data.insert("ui".into(), Arc::new(egui::FontData::from_owned(b)));
        fonts.families.entry(egui::FontFamily::Proportional).or_default().insert(0, "ui".into());
    }
    if let Some(b) = load(&["CascadiaMono.ttf", "consola.ttf", "Consolas.ttf"]) {
        fonts.font_data.insert("mono".into(), Arc::new(egui::FontData::from_owned(b)));
        fonts.families.entry(egui::FontFamily::Monospace).or_default().insert(0, "mono".into());
    }
    if let Some(b) = load(&["seguisym.ttf"]) {
        fonts.font_data.insert("sym".into(), Arc::new(egui::FontData::from_owned(b)));
        for fam in [egui::FontFamily::Proportional, egui::FontFamily::Monospace] {
            fonts.families.entry(fam).or_default().push("sym".into());
        }
    }
    ctx.set_fonts(fonts);
}

fn font_dirs() -> Vec<std::path::PathBuf> {
    let mut v = Vec::new();
    if let Some(w) = std::env::var_os("WINDIR") {
        v.push(std::path::PathBuf::from(w).join("Fonts"));
    }
    v.push("C:\\Windows\\Fonts".into());
    v
}

/// A small rounded green square with three "text lines", drawn in code so the
/// binary needs no image files.
fn app_icon() -> egui::IconData {
    let s = 64u32;
    let mut rgba = vec![0u8; (s * s * 4) as usize];
    let r = 14.0f32;
    for y in 0..s {
        for x in 0..s {
            let (fx, fy) = (x as f32 + 0.5, y as f32 + 0.5);
            // rounded-rect coverage
            let cx = fx.clamp(r, s as f32 - r);
            let cy = fy.clamp(r, s as f32 - r);
            let d = ((fx - cx).powi(2) + (fy - cy).powi(2)).sqrt();
            let a = (r - d + 0.5).clamp(0.0, 1.0);
            if a <= 0.0 {
                continue;
            }
            let mut c = [0x1F, 0xA3, 0x6B];
            // three white lines
            let line = |y0: f32, x1: f32| fy >= y0 && fy < y0 + 6.0 && fx >= 16.0 && fx < x1;
            if line(18.0, 48.0) || line(29.0, 48.0) || line(40.0, 38.0) {
                c = [0xFF, 0xFF, 0xFF];
            }
            let i = ((y * s + x) * 4) as usize;
            rgba[i..i + 4].copy_from_slice(&[c[0], c[1], c[2], (a * 255.0) as u8]);
        }
    }
    egui::IconData { rgba, width: s, height: s }
}
