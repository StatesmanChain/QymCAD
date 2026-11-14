//! The system chooser and bounded decoding keep image loading off the frame thread.
use super::*;
use qymcad_ai::ImageInput;
use std::path::PathBuf;

struct Attached {
    input: ImageInput,
    texture: egui::TextureHandle,
}
#[derive(Default)]
pub(super) struct Gallery {
    items: Vec<Attached>,
    retired: Vec<egui::TextureHandle>,
    chosen: Option<mpsc::Receiver<PathBuf>>,
    decoded: Option<mpsc::Receiver<Result<ImageInput, &'static str>>>,
}
pub(super) enum Activity {
    Idle,
    Generating,
}
pub(crate) fn ai_image_dialog() -> rfd::AsyncFileDialog {
    rfd::AsyncFileDialog::new().set_title(crate::i18n::tr("ai-add-image")).add_filter(crate::i18n::tr("ai-image-formats"), &["png", "jpg", "jpeg", "webp"])
}
pub(crate) fn ai_image_chosen(tx: &mpsc::Sender<PathBuf>, path: PathBuf) {
    let _ = tx.send(path);
}
impl Gallery {
    pub(super) fn is_empty(&self) -> bool {
        self.items.is_empty()
    }
    pub(super) fn loading(&self) -> bool {
        self.chosen.is_some() || self.decoded.is_some()
    }
    pub(super) fn clear(&mut self) {
        self.retired.extend(self.items.drain(..).map(|item| item.texture));
    }
    pub(super) fn inputs(&self) -> Vec<ImageInput> {
        self.items.iter().map(|i| i.input.clone()).collect()
    }
    pub(super) fn poll(&mut self, ctx: &egui::Context) -> Option<&'static str> {
        self.retired.clear();
        if let Some(rx) = &self.chosen {
            match rx.try_recv() {
                Ok(path) => {
                    self.chosen = None;
                    let (tx, rx) = mpsc::channel();
                    self.decoded = Some(rx);
                    let repaint = ctx.clone();
                    let _worker = qymcad_kernel::worker(move || {
                        let _ = tx.send(ImageInput::read(&path));
                        repaint.request_repaint();
                    });
                }
                Err(mpsc::TryRecvError::Disconnected) => self.chosen = None,
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if let Some(rx) = &self.decoded {
            match rx.try_recv() {
                Ok(result) => {
                    self.decoded = None;
                    match result {
                        Ok(input) => {
                            let thumb = input.thumbnail();
                            let pixels = egui::ColorImage::from_rgba_unmultiplied([thumb.width() as usize, thumb.height() as usize], thumb.as_raw());
                            let texture = ctx.load_texture("ai-reference", pixels, egui::TextureOptions::LINEAR);
                            self.items.push(Attached { input, texture });
                        }
                        Err(key) => return Some(key),
                    }
                }
                Err(mpsc::TryRecvError::Disconnected) => {
                    self.decoded = None;
                    return Some("ai-error-image-read");
                }
                Err(mpsc::TryRecvError::Empty) => {}
            }
        }
        if self.loading() {
            ctx.request_repaint_after(std::time::Duration::from_millis(100));
        }
        None
    }
    pub(super) fn draw(&mut self, ui: &mut egui::Ui, wc: &mut qymcad_ui_state::WinCtx, activity: Activity) {
        let enabled = matches!(activity, Activity::Idle) && !self.loading() && !wc.file_ask_open;
        if ui.add_enabled(enabled && self.items.len() < qymcad_ai::MAX_IMAGES, egui::Button::new(crate::i18n::tr("ai-add-image"))).clicked() {
            let (tx, rx) = mpsc::channel();
            self.chosen = Some(rx);
            wc.ask.push(WinAsk::AiImage(tx));
        }
        if self.loading() {
            ui.label(crate::i18n::tr("ai-image-loading"));
        }
        if !self.items.is_empty() {
            ui.small(crate::i18n::tr("ai-vision-hint"));
            let label = crate::i18n::tr1("ai-images-attached", "count", &self.items.len().to_string());
            ui.collapsing(label, |ui| {
                let mut remove = None;
                for (index, item) in self.items.iter().enumerate() {
                    ui.push_id(index, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(item.input.name());
                            if ui.add_enabled(enabled, egui::Button::new(crate::i18n::tr("ai-remove-image"))).clicked() {
                                remove = Some(index);
                            }
                        });
                        ui.add(egui::Image::new((item.texture.id(), item.texture.size_vec2())).max_size(egui::vec2(260.0, 180.0)));
                    });
                }
                if let Some(index) = remove {
                    self.retired.push(self.items.remove(index).texture);
                }
            });
        }
    }
}
