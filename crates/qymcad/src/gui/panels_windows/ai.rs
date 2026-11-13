//! A checked preview is inserted as one undo step, with no executable AI output.
use super::*;
use qymcad_ai::{Connection, Exchange, Proposal};
use qymcad_core::model::Project;
use std::sync::{Arc, Mutex, mpsc};
mod preview;
struct Answer {
    request: String,
    proposal: Proposal,
    part: Option<Arc<Project>>,
}
#[derive(Default)]
struct Chat {
    connection: Connection,
    prompt: String,
    history: Vec<Exchange>,
    answer: Option<Answer>,
    pending: Option<mpsc::Receiver<Result<Answer, &'static str>>>,
    notice: Option<Notice>,
}
fn state(ctx: &egui::Context) -> Arc<Mutex<Chat>> {
    ctx.data_mut(|d| d.get_temp_mut_or_insert_with(egui::Id::new("ai_chat"), || Arc::new(Mutex::new(Chat::default()))).clone())
}
fn checked(proposal: Proposal) -> Result<Answer, &'static str> {
    let part = if let Some(plan) = &proposal.plan {
        let mut built = plan.build()?;
        let _gate = qymcad_kernel::kernel_gate();
        let kernel = qymcad_kernel::OcctKernel::default();
        let _ = built.project.regenerate(&kernel);
        if !built.project.regen_errors.is_empty() {
            return Err("ai-error-geometry");
        }
        let shapes = kernel.shapes.borrow();
        let shape = shapes.get(&built.body).ok_or("ai-error-geometry")?;
        if !shape.is_valid() || shape.solid_count() != 1 || !shape.volume().is_finite() || shape.volume() <= 1e-9 {
            return Err("ai-error-geometry");
        }
        Some(Arc::new(built.project))
    } else {
        None
    };
    Ok(Answer { request: String::new(), proposal, part })
}
fn receive(chat: &mut Chat) {
    let Some(rx) = &chat.pending else { return };
    match rx.try_recv() {
        Ok(result) => {
            chat.pending = None;
            match result {
                Ok(answer) => {
                    chat.history.push(Exchange { prompt: answer.request.clone(), reply: answer.proposal.clone() });
                    if chat.history.len() > 6 {
                        chat.history.remove(0);
                    }
                    chat.answer = Some(answer);
                    chat.notice = None;
                }
                Err(key) => chat.notice = Some(Notice::refusal(crate::i18n::tr(key))),
            }
        }
        Err(mpsc::TryRecvError::Disconnected) => {
            chat.pending = None;
            chat.notice = Some(Notice::error(crate::i18n::tr("ai-error-worker")));
        }
        Err(mpsc::TryRecvError::Empty) => {}
    }
}
pub(super) fn window(wc: &mut qymcad_ui_state::WinCtx, ctx: &egui::Context) {
    if !wc.win.is(WinKind::Ai) {
        return;
    }
    let state = state(ctx);
    let Ok(mut chat) = state.lock() else { return };
    receive(&mut chat);
    let mut open = true;
    egui::Window::new(crate::i18n::tr("ai-title")).id(egui::Id::new("win_ai")).open(&mut open).default_width(500.0).show(ctx, |ui| {
        egui::ScrollArea::vertical().max_height(700.0).show(ui, |ui| {
            ui.label(crate::i18n::tr("ai-intro"));
            ui.collapsing(crate::i18n::tr("ai-settings"), |ui| {
                ui.label(crate::i18n::tr("ai-endpoint"));
                ui.add(egui::TextEdit::singleline(&mut chat.connection.endpoint).desired_width(f32::INFINITY).hint_text("https://server.example/v1/chat/completions"));
                ui.label(crate::i18n::tr("ai-model"));
                ui.add(egui::TextEdit::singleline(&mut chat.connection.model).desired_width(f32::INFINITY));
                ui.label(crate::i18n::tr("ai-key"));
                ui.add(egui::TextEdit::singleline(&mut chat.connection.key).password(true).desired_width(f32::INFINITY));
                ui.small(crate::i18n::tr("ai-key-session"));
                if chat.connection.endpoint.starts_with("http://") {
                    qymcad_ui_state::say(ui, &wc.scheme.pal, &Notice::warning(crate::i18n::tr("ai-http")), NoticeSize::Small);
                }
            });
            if !chat.history.is_empty() {
                ui.collapsing(crate::i18n::tr("ai-history"), |ui| {
                    for turn in &chat.history {
                        ui.strong(&turn.prompt);
                        ui.label(&turn.reply.message);
                        ui.separator();
                    }
                });
            }
            ui.separator();
            ui.label(crate::i18n::tr("ai-prompt"));
            ui.add(egui::TextEdit::multiline(&mut chat.prompt).desired_rows(4).desired_width(f32::INFINITY).char_limit(12000).hint_text(crate::i18n::tr("ai-example")));
            ui.small(crate::i18n::tr("ai-sent"));
            ui.horizontal(|ui| {
                let busy = chat.pending.is_some();
                if ui.add_enabled(!busy && !chat.prompt.trim().is_empty(), egui::Button::new(crate::i18n::tr("ai-send"))).clicked() {
                    let connection = chat.connection.clone();
                    let prompt = chat.prompt.clone();
                    let history = chat.history.clone();
                    let (tx, rx) = mpsc::channel();
                    chat.pending = Some(rx);
                    chat.answer = None;
                    chat.notice = None;
                    let repaint = ctx.clone();
                    let _worker = qymcad_kernel::worker(move || {
                        let result = qymcad_ai::ask(&connection, &prompt, &history).and_then(checked).map(|mut a| {
                            a.request = prompt;
                            a
                        });
                        let _ = tx.send(result);
                        repaint.request_repaint();
                    });
                }
                if busy {
                    ui.spinner();
                    ui.label(crate::i18n::tr("ai-wait"));
                    if ui.button(crate::i18n::tr("ai-cancel")).clicked() {
                        chat.pending = None;
                        chat.notice = Some(Notice::info(crate::i18n::tr("ai-cancelled")));
                    }
                    ctx.request_repaint_after(std::time::Duration::from_millis(200));
                }
                if !busy && ui.button(crate::i18n::tr("ai-clear")).clicked() {
                    chat.history.clear();
                    chat.answer = None;
                    chat.notice = None;
                    chat.prompt.clear();
                }
            });
            if let Some(n) = &chat.notice {
                qymcad_ui_state::say(ui, &wc.scheme.pal, n, NoticeSize::Line);
            }
            if let Some(answer) = &chat.answer {
                ui.separator();
                ui.label(&answer.proposal.message);
                if let Some(part) = &answer.part {
                    preview::draw(ui, part);
                    ui.small(crate::i18n::tr("ai-variant"));
                    let editing = wc.edits.open.is_some() || wc.sketch_ses.editing.is_some();
                    if ui.add_enabled(!editing, egui::Button::new(crate::i18n::tr("ai-apply"))).clicked() {
                        wc.ask.push(WinAsk::AiPart(part.clone()));
                    }
                    if editing {
                        ui.small(crate::i18n::tr("ai-finish-edit"));
                    }
                }
                if let Some(plan) = &answer.proposal.plan {
                    ui.collapsing(crate::i18n::tr("ai-operations"), |ui| {
                        for op in &plan.operations {
                            operation_row(ui, op);
                        }
                    });
                }
            }
        });
    });
    wc.win.set(WinKind::Ai, open);
}
pub(crate) fn apply_ai_part(pc: &mut qymcad_ui_state::PartCtx, part: &Project) -> Option<Id> {
    if pc.edits.open.is_some() || pc.sketch_ses.editing.is_some() {
        return None;
    }
    qymcad_ui_state::begin_edit(pc.edits, pc.project, crate::i18n::tr("ai-undo"));
    let root = pc.project.ensure_root();
    let Some(id) = pc.project.graft(part, root, &crate::i18n::name) else {
        qymcad_ui_state::abort_edit(&mut pc.rebuild());
        *pc.status = Notice::refusal(crate::i18n::tr("ai-error-insert"));
        return None;
    };
    qymcad_part::resync_after_topology_change(pc);
    qymcad_ui_state::commit_edit(&mut pc.rebuild());
    *pc.mode_3d = true;
    pc.win.close(WinKind::Start);
    *pc.status = Notice::info(crate::i18n::tr("ai-inserted"));
    Some(id)
}
fn operation_row(ui: &mut egui::Ui, op: &qymcad_core::ai::Operation) {
    use qymcad_core::ai::{Action, Boolean};
    let key = match &op.action {
        Action::Box { .. } => "ai-op-box",
        Action::Cylinder { .. } => "ai-op-cylinder",
        Action::Sphere { .. } => "ai-op-sphere",
        Action::Move { .. } => "ai-op-move",
        Action::Fillet { .. } => "ai-op-fillet",
        Action::Boolean { operation, .. } => match operation {
            Boolean::Cut => "ai-op-cut",
            Boolean::Union => "ai-op-union",
            Boolean::Intersect => "ai-op-intersect",
        },
    };
    let details = match &op.action {
        Action::Box { size } => format!("{} x {} x {}", size.x, size.y, size.z),
        Action::Cylinder { radius, height } => format!("R={radius}; H={height}"),
        Action::Sphere { radius } | Action::Fillet { radius, .. } => format!("R={radius}"),
        Action::Move { source, offset } => format!("{source}: {}, {}, {}", offset.x, offset.y, offset.z),
        Action::Boolean { base, tool, .. } => format!("{base}, {tool}"),
    };
    ui.label(format!("{}: {} ({details})", op.id, crate::i18n::tr(key)));
}
