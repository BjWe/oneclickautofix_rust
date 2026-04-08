use std::sync::mpsc;
use std::thread;

use eframe::egui;

use crate::command::parse_command;
use crate::config::AppConfig;
use crate::executor::execute;

// ---------------------------------------------------------------------------
// State

#[derive(Clone)]
pub struct StepState {
    pub status:           StepStatus,
    pub logoff_countdown: Option<u64>,
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
            .map(|_| StepState { status: StepStatus::Waiting, logoff_countdown: None })
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
                    .map(|r| parse_command(r))
                    .collect();

                let cmds = match cmds {
                    Ok(c)  => c,
                    Err(e) => {
                        let _ = tx.send(Ev::StepFailed(i, e));
                        ctx.request_repaint();
                        return;
                    }
                };

                let error: Option<String> = if step.parallel {
                    thread::scope(|s| {
                        cmds.iter()
                            .map(|cmd| s.spawn(|| execute(cmd, None)))
                            .collect::<Vec<_>>()
                            .into_iter()
                            .find_map(|h| {
                                h.join()
                                    .unwrap_or_else(|_| Err("Thread-Fehler".to_string()))
                                    .err()
                            })
                    })
                } else {
                    let mut err = None;
                    for cmd in &cmds {
                        let tx2  = tx.clone();
                        let ctx2 = ctx.clone();
                        let countdown = move |n: u64| {
                            let _ = tx2.send(Ev::StepLogoff(i, n));
                            ctx2.request_repaint();
                        };
                        if let Err(e) = execute(cmd, Some(&countdown)) {
                            err = Some(e);
                            break;
                        }
                    }
                    err
                };

                if let Some(e) = error {
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
            ui.add_space(4.0);
            ui.heading(&self.config.title);
            ui.separator();

            if is_welcome {
                // Welcome card
                ui.add_space(8.0);
                egui::Frame::group(ui.style())
                    .inner_margin(egui::Margin::same(14.0))
                    .show(ui, |ui| {
                        ui.vertical_centered(|ui| {
                            ui.label(egui::RichText::new(&self.config.welcome.ask).size(13.0));
                            ui.add_space(14.0);
                            ui.horizontal(|ui| {
                                if ui.button(egui::RichText::new("▶  Start").size(13.0)).clicked() {
                                    do_start = true;
                                }
                                if ui.button("Abbrechen").clicked() {
                                    std::process::exit(0);
                                }
                            });
                        });
                    });
                ui.add_space(14.0);
                ui.separator();
            } else {
                // Progress bar
                ui.add(egui::ProgressBar::new(progress));
                ui.add_space(4.0);
            }

            // Step list
            egui::ScrollArea::vertical().show(ui, |ui| {
                for (step, state) in self.config.steps.iter().zip(self.step_states.iter()) {
                    let running = matches!(state.status, StepStatus::Running);

                    ui.horizontal(|ui| {
                        // Status column
                        ui.set_min_width(22.0);
                        match &state.status {
                            StepStatus::Waiting    => { ui.label(egui::RichText::new("○").color(egui::Color32::DARK_GRAY)); }
                            StepStatus::Running    => { ui.add(egui::Spinner::new().size(16.0)); }
                            StepStatus::Success    => { ui.label(egui::RichText::new("✓").color(egui::Color32::GREEN)); }
                            StepStatus::Error(_)   => { ui.label(egui::RichText::new("✗").color(egui::Color32::RED)); }
                        }

                        ui.vertical(|ui| {
                            ui.label(if running {
                                egui::RichText::new(&step.title).strong()
                            } else {
                                egui::RichText::new(&step.title)
                            });
                            ui.label(
                                egui::RichText::new(&step.description)
                                    .small()
                                    .color(egui::Color32::GRAY),
                            );
                            if let StepStatus::Error(msg) = &state.status {
                                ui.label(egui::RichText::new(msg).small().color(egui::Color32::RED));
                            }
                            if running {
                                if let Some(n) = state.logoff_countdown {
                                    ui.label(
                                        egui::RichText::new(format!("Abmeldung in {n} s …"))
                                            .small()
                                            .color(egui::Color32::from_rgb(220, 130, 0)),
                                    );
                                }
                            }
                        });
                    });
                    ui.add_space(2.0);
                    ui.separator();
                }

                // Completion notice
                if let Some(aborted) = completion {
                    ui.add_space(10.0);
                    if aborted {
                        ui.colored_label(egui::Color32::RED,   "✗  Abgebrochen – ein Schritt ist fehlgeschlagen.");
                    } else {
                        ui.colored_label(egui::Color32::GREEN, "✓  Alle Schritte erfolgreich abgeschlossen.");
                    }
                }
            });

            // Footer: percentage
            if !is_welcome {
                ui.separator();
                ui.vertical_centered(|ui| {
                    ui.label(format!("Fortschritt: {} %", (progress * 100.0).round() as u32));
                });
            }
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
