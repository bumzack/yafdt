use crate::model::SharedState;
use crate::move_files::move_non_kept;
use eframe::egui;
use std::{fs, path::PathBuf};

pub fn run_gui(shared: SharedState) {
    let opts = eframe::NativeOptions::default();
    let _ = eframe::run_native(
        "Duplicate Finder",
        opts,
        Box::new(|_cc| Ok(Box::new(GuiApp { shared }))),
    );
}

struct GuiApp {
    shared: SharedState,
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        let mut s = self.shared.lock().unwrap();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Duplicate Finder");
            ui.separator();
            ui.label(format!("Root  : {}", s.root.display()));
            ui.label(format!("Target: {}", s.target.display()));

            let kept = s.folders.iter().filter(|f| f.keep).count();
            let total_files: usize = s.folders.iter().map(|f| f.files.len()).sum();
            ui.add_space(4.0);
            ui.label(format!(
                "{} folder(s) with duplicates \u{2022} {} file(s) \u{2022} {} folder(s) kept",
                s.folders.len(),
                total_files,
                kept
            ));

            ui.add_space(6.0);
            ui.horizontal(|ui| {
                if ui.button("Keep all folders").clicked() {
                    for f in s.folders.iter_mut() {
                        f.keep = true;
                    }
                }
                if ui.button("Un-keep all folders").clicked() {
                    for f in s.folders.iter_mut() {
                        f.keep = false;
                    }
                }
                let move_enabled = kept > 0 || s.folders.is_empty();
                let clicked = ui
                    .add_enabled(move_enabled, egui::Button::new("Move non-kept"))
                    .clicked();
                if clicked {
                    let root = s.root.clone();
                    let target = s.target.clone();
                    let (moved, errors) = move_non_kept(&mut s.folders, &root, &target);
                    println!("moved {} files, {} error(s)", moved, errors.len());
                    for e in &errors {
                        println!("  err: {}", e);
                    }
                }
            });

            if kept == 0 && !s.folders.is_empty() {
                ui.colored_label(
                    egui::Color32::RED,
                    "No folder is marked to keep \u{2014} move blocked.",
                );
            }

            ui.separator();
            ui.label("Folders containing duplicates (check to keep in place):");

            egui::ScrollArea::vertical().show(ui, |ui| {
                let mut toggles: Vec<(usize, bool)> = Vec::new();
                for (i, f) in s.folders.iter_mut().enumerate().take(5_000) {
                    let mut keep = f.keep;
                    let header = format!(
                        "{}  [{} file(s), {} B]",
                        f.folder,
                        f.files.len(),
                        f.total_size
                    );
                    ui.collapsing(header, |ui| {
                        if ui.checkbox(&mut keep, "Keep this folder + subfolders").changed() {
                            toggles.push((i, keep));
                        }
                        ui.separator();
                        for fe in &f.files {
                            ui.label(format!("{}  [{} B]", fe.path, fe.size));
                        }
                    });
                }
                // Apply toggles with a recursive cascade: keeping a folder
                // also keeps all of its subfolders.
                for (i, keep) in toggles {
                    let Some(target) = s.folders.get(i) else { continue };
                    let target_canon = fs::canonicalize(&target.folder)
                        .unwrap_or_else(|_| PathBuf::from(&target.folder));
                    for f in s.folders.iter_mut() {
                        let f_canon = fs::canonicalize(&f.folder)
                            .unwrap_or_else(|_| PathBuf::from(&f.folder));
                        if f_canon == target_canon || f_canon.starts_with(&target_canon) {
                            f.keep = keep;
                        }
                    }
                }
            });
        });

        ctx.request_repaint();
    }
}
