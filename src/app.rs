use std::sync::mpsc;
use std::thread;

use eframe::egui;

use crate::command::parse_command;
use crate::config::{AppConfig, OnError};
use crate::executor::execute;

// ---------------------------------------------------------------------------
// State

#[derive(Clone)]
pub struct StepState {
    pub status:           StepStatus,
    pub logoff_countdown: Option<u64>,
    pub warnings:         Vec<String>,
}

#[derive(Clone, PartialEq)]
pub enum StepStatus {
    Waiting,
    Running,
    Success,
    Error(String),
}

enum Phase {
    Welcome,
    Running,
    Complete { aborted: bool },
}

enum Ev {
    StepStarted(usize),
    StepLogoff(usize, u64),
    StepDone(usize),
    StepFailed(usize, String),
    StepWarning(usize, String),
    AllDone,
}

// ---------------------------------------------------------------------------
// App

pub struct OneClickApp {
    config:      AppConfig,
    step_states: Vec<StepState>,
    phase:       Phase,
    rx:          Option<mpsc::Receiver<Ev>>,
}

impl OneClickApp {
    pub fn new(config: AppConfig) -> Self {
        let step_states = config.steps.iter()
            .map(|_| StepState { status: StepStatus::Waiting, logoff_countdown: None, warnings: Vec::new() })
            .collect();
        Self { config, step_states, phase: Phase::Welcome, rx: None }
    }

    fn start(&mut self, ctx: egui::Context) {
        let (tx, rx) = mpsc::channel::<Ev>();
        self.rx    = Some(rx);
        self.phase = Phase::Running;

        let steps = self.config.steps.clone();

        thread::spawn(move || {
            for (i, step) in steps.iter().enumerate() {
                let _ = tx.send(Ev::StepStarted(i));
                ctx.request_repaint();

                let cmds: Result<Vec<_>, _> = step.run.iter()
                    .map(|entry| {
                        parse_command(&entry.command_string())
                            .map(|cmd| (cmd, entry.onerror()))
                    })
                    .collect();

                let cmds = match cmds {
                    Ok(c)  => c,
                    Err(e) => {
                        let _ = tx.send(Ev::StepFailed(i, e));
                        ctx.request_repaint();
                        return;
                    }
                };

                let hard_error: Option<String> = if step.parallel {
                    let results: Vec<(Result<(), String>, OnError)> = thread::scope(|s| {
                        let handles: Vec<_> = cmds.iter()
                            .map(|(cmd, _)| s.spawn(|| execute(cmd, None)))
                            .collect();
                        handles.into_iter()
                            .zip(cmds.iter().map(|(_, oe)| oe.clone()))
                            .map(|(h, oe)| (
                                h.join().unwrap_or_else(|_| Err("Thread-Fehler".to_string())),
                                oe,
                            ))
                            .collect()
                    });
                    let mut err = None;
                    for (result, onerror) in results {
                        match (result, onerror) {
                            (Err(e), OnError::Stop) => { err = Some(e); }
                            (Err(e), OnError::ContinueMessage) => {
                                let _ = tx.send(Ev::StepWarning(i, e));
                                ctx.request_repaint();
                            }
                            (Err(_), OnError::ContinueSilent) => {}
                            (Ok(()), _) => {}
                        }
                    }
                    err
                } else {
                    let mut err = None;
                    for (cmd, onerror) in &cmds {
                        let tx2  = tx.clone();
                        let ctx2 = ctx.clone();
                        let countdown = move |n: u64| {
                            let _ = tx2.send(Ev::StepLogoff(i, n));
                            ctx2.request_repaint();
                        };
                        match execute(cmd, Some(&countdown)) {
                            Ok(()) => {}
                            Err(e) => match onerror {
                                OnError::Stop => { err = Some(e); break; }
                                OnError::ContinueMessage => {
                                    let _ = tx.send(Ev::StepWarning(i, e));
                                    ctx.request_repaint();
                                }
                                OnError::ContinueSilent => {}
                            }
                        }
                    }
                    err
                };

                if let Some(e) = hard_error {
                    let _ = tx.send(Ev::StepFailed(i, e));
                    ctx.request_repaint();
                    return;
                }

                let _ = tx.send(Ev::StepDone(i));
                ctx.request_repaint();
            }

            let _ = tx.send(Ev::AllDone);
            ctx.request_repaint();
        });
    }

    fn poll(&mut self) {
        let Some(rx) = &self.rx else { return };
        while let Ok(ev) = rx.try_recv() {
            match ev {
                Ev::StepStarted(i) => {
                    self.step_states[i].status           = StepStatus::Running;
                    self.step_states[i].logoff_countdown = None;
                }
                Ev::StepLogoff(i, n) => {
                    self.step_states[i].logoff_countdown = Some(n);
                }
                Ev::StepDone(i) => {
                    self.step_states[i].status           = StepStatus::Success;
                    self.step_states[i].logoff_countdown = None;
                }
                Ev::StepFailed(i, msg) => {
                    self.step_states[i].status = StepStatus::Error(msg);
                    self.phase = Phase::Complete { aborted: true };
                }
                Ev::StepWarning(i, msg) => {
                    self.step_states[i].warnings.push(msg);
                }
                Ev::AllDone => {
                    self.phase = Phase::Complete { aborted: false };
                }
            }
        }
    }

    fn progress(&self) -> f32 {
        let done = self.step_states.iter()
            .filter(|s| matches!(s.status, StepStatus::Success | StepStatus::Error(_)))
            .count();
        done as f32 / self.step_states.len().max(1) as f32
    }
}

impl eframe::App for OneClickApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.poll();

        // Snapshot values that don't need self-borrow inside the closure
        let progress   = self.progress();
        let is_welcome = matches!(self.phase, Phase::Welcome);
        let completion = match &self.phase {
            Phase::Complete { aborted } => Some(*aborted),
            _                           => None,
        };

        let mut do_start = false;

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(8.0);
            ui.heading(egui::RichText::new(&self.config.title).size(18.0));
            ui.add_space(4.0);
            ui.separator();
            ui.add_space(6.0);

            if is_welcome {
                // Welcome card
                egui::Frame::group(ui.style())
                    .inner_margin(egui::Margin::same(20.0))
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new(&self.config.welcome.ask).size(13.0));
                            ui.add_space(16.0);
                            ui.horizontal(|ui| {
                                if ui.add_sized([96.0, 28.0], egui::Button::new(
                                    egui::RichText::new("▶  Start").size(13.0)
                                )).clicked() {
                                    do_start = true;
                                }
                                ui.add_space(6.0);
                                if ui.add_sized([96.0, 28.0], egui::Button::new(
                                    egui::RichText::new("Abbrechen").size(13.0)
                                )).clicked() {
                                    std::process::exit(0);
                                }
                            });
                        });
                    });
                ui.add_space(14.0);
                ui.separator();
            } else {
                // Progress bar with embedded percentage
                ui.add(
                    egui::ProgressBar::new(progress)
                        .text(format!("{} %", (progress * 100.0).round() as u32)),
                );
                ui.add_space(6.0);
            }

            // Step list
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (step, state) in self.config.steps.iter().zip(self.step_states.iter()) {
                    let running = matches!(state.status, StepStatus::Running);
                    let done    = matches!(state.status, StepStatus::Success);

                    ui.add_space(6.0);
                    ui.horizontal(|ui| {
                        ui.set_min_height(30.0);

                        // Status icon
                        ui.add_space(2.0);
                        ui.set_min_width(26.0);
                        match &state.status {
                            StepStatus::Waiting  => { ui.label(egui::RichText::new("○").size(16.0).color(egui::Color32::from_gray(110))); }
                            StepStatus::Running  => { ui.add(egui::Spinner::new().size(16.0)); }
                            StepStatus::Success  => { ui.label(egui::RichText::new("✓").size(16.0).color(egui::Color32::from_rgb(80, 200, 100))); }
                            StepStatus::Error(_) => { ui.label(egui::RichText::new("✗").size(16.0).color(egui::Color32::from_rgb(220, 70, 70))); }
                        }
                        ui.add_space(4.0);

                        ui.vertical(|ui| {
                            let title_color = if done {
                                egui::Color32::from_gray(140)
                            } else {
                                ui.visuals().text_color()
                            };
                            let desc_gray = if done { 100u8 } else { 150u8 };

                            let title_text = egui::RichText::new(&step.title)
                                .size(13.5)
                                .color(title_color);
                            ui.label(if running { title_text.strong() } else { title_text });
                            ui.label(
                                egui::RichText::new(&step.description)
                                    .size(11.5)
                                    .color(egui::Color32::from_gray(desc_gray)),
                            );
                            if let StepStatus::Error(msg) = &state.status {
                                ui.add_space(2.0);
                                ui.label(
                                    egui::RichText::new(msg)
                                        .size(11.5)
                                        .color(egui::Color32::from_rgb(220, 70, 70)),
                                );
                            }
                            for warn in &state.warnings {
                                ui.add_space(2.0);
                                ui.label(
                                    egui::RichText::new(warn)
                                        .size(11.5)
                                        .color(egui::Color32::from_rgb(220, 150, 0)),
                                );
                            }
                            if running {
                                if let Some(n) = state.logoff_countdown {
                                    ui.add_space(2.0);
                                    ui.label(
                                        egui::RichText::new(format!("Abmeldung in {n} s …"))
                                            .size(11.5)
                                            .color(egui::Color32::from_rgb(220, 150, 0)),
                                    );
                                }
                            }
                        });
                    });
                    ui.add_space(4.0);
                    ui.separator();
                }

                // Completion banner
                if let Some(aborted) = completion {
                    ui.add_space(12.0);
                    let (fg, bg, text) = if aborted {
                        (
                            egui::Color32::from_rgb(220, 70, 70),
                            egui::Color32::from_rgba_unmultiplied(220, 70, 70, 18),
                            "✗  Abgebrochen – ein Schritt ist fehlgeschlagen.",
                        )
                    } else {
                        (
                            egui::Color32::from_rgb(80, 200, 100),
                            egui::Color32::from_rgba_unmultiplied(80, 200, 100, 18),
                            "✓  Alle Schritte erfolgreich abgeschlossen.",
                        )
                    };
                    egui::Frame::none()
                        .fill(bg)
                        .inner_margin(egui::Margin::same(10.0))
                        .rounding(egui::Rounding::same(4.0))
                        .show(ui, |ui| {
                            ui.label(egui::RichText::new(text).size(13.0).color(fg).strong());
                        });
                }
            });
        });

        if do_start {
            self.start(ctx.clone());
        }
    }
}

// ---------------------------------------------------------------------------
// Error screen (config load failure)

pub struct ErrorApp { pub message: String }

impl eframe::App for ErrorApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical_centered(|ui| {
                ui.add_space(40.0);
                ui.heading(egui::RichText::new("Konfigurationsfehler").color(egui::Color32::RED));
                ui.add_space(16.0);
                ui.label(&self.message);
            });
        });
    }
}
