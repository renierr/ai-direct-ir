//! Egui window lifecycle and command rendering. Keep it separate from ABI calls.

use eframe::egui;
use wasmtime::{Engine, Result};

use crate::host::UiCommand;
use crate::link::link_all;
use crate::manifest::Manifest;

pub fn run(engine: Engine, manifest: Manifest, base: std::path::PathBuf) -> Result<()> {
    let linked = link_all(&engine, &manifest, &base)?;
    let title = format!("air: {}", manifest.app.path);
    let entry = manifest.app.run;
    eframe::run_native(
        &title,
        eframe::NativeOptions::default(),
        Box::new(move |_| Ok(Box::new(GuiApp { linked, entry }))),
    )
    .map_err(|e| wasmtime::Error::msg(format!("native GUI: {e}")))
}

struct GuiApp {
    linked: crate::link::Linked,
    entry: String,
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.linked.store.data_mut().ui.clear();
        let result = self
            .linked
            .app_inst
            .get_func(&mut self.linked.store, &self.entry)
            .ok_or_else(|| wasmtime::Error::msg(format!("app has no func {}", self.entry)))
            .and_then(|func| func.call(&mut self.linked.store, &[], &mut []));
        if let Err(error) = result {
            eprintln!("gui app: {error}");
        }
        let commands = std::mem::take(&mut self.linked.store.data_mut().ui);
        let mut clicked = std::collections::HashSet::new();
        egui::CentralPanel::default().show(ctx, |ui| {
            for command in commands {
                match command {
                    UiCommand::Label(text) => {
                        ui.label(text);
                    }
                    UiCommand::Button(text) => {
                        if ui.button(&text).clicked() {
                            clicked.insert(text);
                        }
                    }
                }
            }
        });
        self.linked.store.data_mut().ui_clicked = clicked;
        ctx.request_repaint();
    }
}
