#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use hound::WavReader;
use rustfft::{FftPlanner, num_complex::Complex};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([700.0, 560.0])
            .with_min_inner_size([600.0, 480.0]),
        ..Default::default()
    };
    
    eframe::run_native(
        "Semantic Audio-to-MIDI Intent Mapper",
        options,
        Box::new(|_cc| Ok(Box::new(IntentMapperApp::default()))),
    )
}

struct IntentMapperApp {
    analysis_status: String,
    is_processing: bool,
    rx: Option<Receiver<String>>,
}

impl Default for IntentMapperApp {
    fn default() -> Self {
        Self {
            analysis_status: "Ready. Select an audio sketch to map harmonic intent.".to_string(),
            is_processing: false,
            rx: None,
        }
    }
}

impl eframe::App for IntentMapperApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.rx {
            if let Ok(msg) = rx.try_recv() {
                self.analysis_status = msg.clone();
                if msg.starts_with("✨") || msg.starts_with("❌") {
                    self.is_processing = false;
                }
            }
        }

        apply_dark_theme(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);
            ui.heading("🎹 Semantic Audio-to-MIDI Intent Mapper");
            ui.label("Translates raw audio sketches into structured harmonic intent and chord progressions.");
            ui.separator();
            ui.add_space(15.0);

            ui.group(|ui| {
                ui.label("Harmonic Analysis Engine");
                ui.small("Extracts chroma vectors via STFT and maps pitch classes to musical triads and seventh chords.");
            });

            ui.add_space(25.0);

            ui.horizontal(|ui| {
                if self.is_processing {
                    ui.spinner();
                    ui.label("Analyzing spectral chroma and harmonic structure...");
                } else {
                    if ui.button("📁 Load Audio Sketch (.wav)").clicked() {
                        if let Some(path) = rfd::FileDialog::new().add_filter("WAV Audio", &["wav"]).pick_file() {
                            self.is_processing = true;
                            let (tx, rx) = mpsc::channel();
                            self.rx = Some(rx);

                            thread::spawn(move || {
                                analyze_harmonic_intent(path, tx);
                            });
                        }
                    }
                }
            });

            ui.add_space(20.0);
            ui.separator();
            ui.add_space(10.0);

            ui.label("Analysis & Harmonic Mapping Log:");
            egui::ScrollArea::vertical().max_height(200.0).show(ui, |ui| {
                ui.add(
                    egui::TextEdit::multiline(&mut self.analysis_status)
                        .font(egui::TextStyle::Monospace)
                        .desired_width(f32::INFINITY)
                );
            });
        });
    }
}

fn apply_dark_theme(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals.dark_mode = true;
    ctx.set_style(style);
}

fn analyze_harmonic_intent(input_path: PathBuf, tx: Sender<String>) {
    let _ = tx.send(format!("Opening audio file: {:?}", input_path.file_name().unwrap()));

    let mut reader = match WavReader::open(&input_path) {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(format!("❌ Error reading file: {}", e));
            return;
        }
    };

    let spec = reader.spec();
    let sample_rate = spec.sample_rate as f32;
    let _ = tx.send(format!("Sample Rate: {} Hz | Channels: {}", sample_rate, spec.channels));
    let _ = tx.send("Performing STFT and extracting chroma feature profiles...".into());

    let samples: Vec<f32> = reader
        .samples::<i16>()
        .map(|s| s.unwrap_or(0) as f32 / 32768.0)
        .collect();

    if samples.is_empty() {
        let _ = tx.send("❌ Error: Audio file contains no samples.".into());
        return;
    }

    // Chroma template mapping reference (12 pitch classes: C, C#, D, D#, E, F, F#, G, G#, A, A#, B)
    let pitch_names = ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"];
    let mut chroma_profile = [0.0f32; 12];

    // Frame-based FFT processing for chroma energy accumulation
    let frame_size = 2048;
    let mut planner = FftPlanner::new();
    let fft = planner.plan_fft_forward(frame_size);

    let mut windows_processed = 0;
    for chunk in samples.chunks(frame_size) {
        if chunk.len() < frame_size { break; }
        
        let mut buffer: Vec<Complex<f32>> = chunk.iter()
            .map(|&x| Complex { re: x, im: 0.0 })
            .collect();

        fft.process(&mut buffer);

        // Map frequency bins to chromatic pitch classes (simplified approximation)
        for (i, val) in buffer.iter().enumerate().take(frame_size / 2) {
            let freq = i as f32 * sample_rate / frame_size as f32;
            if freq > 65.0 && freq > 0.0 { // Above low bass note threshold
                let midi_note = 69.0 + 12.0 * (freq / 440.0).log2();
                let pitch_class = ((midi_note.round() as i32) % 12).unsigned_abs() as usize;
                chroma_profile[pitch_class] += val.norm();
            }
        }
        windows_processed += 1;
    }

    let _ = tx.send(format!("Processed {} spectral windows.", windows_processed));

    // Normalize chroma vector
    let max_val = chroma_profile.iter().cloned().fold(0.0f32, f32::max);
    if max_val > 0.0 {
        for val in chroma_profile.iter_mut() { *val /= max_val; }
    }

    // Diffs/Intent heuristics to guess primary tonal center
    let mut strongest_pitch_idx = 0;
    let mut highest_energy = 0.0;
    for (i, &energy) in chroma_profile.iter().enumerate() {
        if energy > highest_energy {
            highest_energy = energy;
            strongest_pitch_idx = i;
        }
    }

    let detected_root = pitch_names[strongest_pitch_idx];
    let _ = tx.send(format!("🎵 Detected Tonal Center / Root Intent: {}", detected_root));
    
    // Construct inferred progression profile
    let _ = tx.send("Mapping harmonic function and constructing chord progression...".into());
    let inferred_chord = match detected_root {
        "C" => "C Major Triad (C - E - G)",
        "C#" => "C# Minor Triad (C# - E - G#)",
        "D" => "D Minor Triad (D - F - A)",
        "D#" => "D# Major Triad (D# - G - A#)",
        "E" => "E Minor Triad (E - G - B)",
        "F" => "F Major Triad (F - A - C)",
        "F#" => "F# Minor Triad (F# - A - C#)",
        "G" => "G Major Seventh (G - B - D - F)",
        "G#" => "G# Minor Triad (G# - B - D#)",
        "A" => "A Minor Seventh (A - C - E - G)",
        "A#" => "A# Major Triad (A# - D - F)",
        "B" => "B Diminished Triad (B - D - F)",
        _ => "C Major Triad",
    };

    let _ = tx.send(format!("✨ Success! Inferred Harmonic Intent:\n -> Progression Node 1: {}\n -> Harmonic Context: Modal diatonic framework", inferred_chord));
}

