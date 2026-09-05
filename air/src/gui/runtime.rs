//! Egui window lifecycle and command rendering. Keep it separate from the ABI.

use eframe::egui;
use wasmtime::{Engine, Result};

use crate::cmds::GuestEnv;
use crate::component::{Linked, link_all, plain_func};
use crate::gui::UiCommand;
use crate::manifest::Manifest;

pub fn run(engine: &Engine, manifest: &Manifest, base: &std::path::Path) -> Result<()> {
    // A frame loop never ends, so there is nothing to hand it a command
    // line's arguments for; directory and network grants still come from the
    // manifest, exactly as they do for a command.
    let linked = link_all(engine, manifest, base, &GuestEnv::default())?;
    let title = format!("air: {}", manifest.app.path);
    let app = GuiApp::new(linked)?;
    eframe::run_native(
        &title,
        eframe::NativeOptions::default(),
        Box::new(move |_| Ok(Box::new(app))),
    )
    .map_err(|e| wasmtime::Error::msg(format!("native GUI: {e}")))
}

struct GuiApp {
    linked: Linked,
    frame: wasmtime::component::TypedFunc<(), ()>,
}

impl GuiApp {
    /// Resolve the entry point once, at startup. A missing or mistyped
    /// `frame` is a manifest error, and finding it out on the first repaint
    /// would mean an open window printing the same failure forever.
    fn new(mut linked: Linked) -> Result<Self> {
        let frame = plain_func(&mut linked)?;
        Ok(GuiApp { linked, frame })
    }
}

impl eframe::App for GuiApp {
    fn update(&mut self, ctx: &egui::Context, _: &mut eframe::Frame) {
        self.linked.store.data_mut().ui.clear();
        // Nothing here touches the bump heap, unlike a request loop: a
        // `string` parameter travels guest-to-host, so the guest hands over a
        // pointer into its own memory and the host copies out. Only values the
        // host produces need `cabi_realloc`, and this interface produces none.
        if let Err(error) = self.frame.call(&mut self.linked.store, ()) {
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
