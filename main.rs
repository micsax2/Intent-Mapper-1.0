// Hides the console/terminal window on Windows
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use hound::{SampleFormat, WavReader, WavSpec, WavWriter};
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;

fn main() -> Result<(), eframe::Error> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([680.0, 680.0])
            .with_min_inner_size([580.0, 560.0]),
        ..Default::default()
    };

    eframe::run_native(
        "Suno AI Stem Cleaner & WAV-to-MIDI Converter",
        options,
        Box::new(|_cc| Ok(Box::new(SunoCleanerApp::default()))),
    )
}

#[derive(Clone, Copy)]
struct PipelineConfig {
    export_clean_wav: bool,
    export_midi: bool,
    click_sensitivity: f32,
    lpf_cutoff: f32,
    midi_gate_db: f32,
    midi_clarity: f32,
    min_note_ms: f32,
}

struct SunoCleanerApp {
    config: PipelineConfig,
    status_message: String,
    is_processing: bool,
    rx: Option<Receiver<String>>,
}

impl Default for SunoCleanerApp {
    fn default() -> Self {
        Self {
            config: PipelineConfig {
                export_clean_wav: true,
                export_midi: true,
                click_sensitivity: 0.04,
                lpf_cutoff: 16500.0,
                midi_gate_db: -42.0,
                midi_clarity: 0.80,
                min_note_ms: 45.0,
            },
            status_message: "Ready. Select a WAV stem to clean and/or convert to MIDI.".to_string(),
            is_processing: false,
            rx: None,
        }
    }
}

impl eframe::App for SunoCleanerApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if let Some(rx) = &self.rx {
            while let Ok(msg) = rx.try_recv() {
                if !self.status_message.is_empty() {
                    self.status_message.push('\n');
                }
                self.status_message.push_str(&msg);
                if msg.starts_with("✨") || msg.starts_with("❌") {
                    self.is_processing = false;
                }
            }
        }

        if self.is_processing {
            ctx.request_repaint();
        }

        egio_styling(ctx);

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.add_space(10.0);
            ui.heading("🎵 Suno AI Stem Cleaner & WAV-to-MIDI Converter");
            ui.label("Open-source DSP pipeline for cleaning stems and extracting MIDI notes.");
            ui.separator();
            ui.add_space(8.0);

            ui.horizontal(|ui| {
                ui.label("Output Modes:");
                ui.checkbox(&mut self.config.export_clean_wav, "Save Cleaned WAV");
                ui.add_space(10.0);
                ui.checkbox(&mut self.config.export_midi, "Extract & Save MIDI (.mid)");
            });

            ui.add_space(8.0);

            ui.group(|ui| {
                ui.label("1. DSP Audio Cleaning Parameters");
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label("Click Sensitivity:");
                    ui.add(
                        egui::Slider::new(&mut self.config.click_sensitivity, 0.01..=0.10)
                            .text("Threshold"),
                    );
                });
                ui.small("Lower threshold catches more subtle isolated digital clicks/pops.");

                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label("High-Frequency Silk LPF:");
                    ui.add(
                        egui::Slider::new(&mut self.config.lpf_cutoff, 10000.0..=20000.0)
                            .text("Hz"),
                    );
                });
                ui.small("2nd-order Butterworth LPF smooths harsh Nyquist glare and vocoder fizz.");
            });

            ui.add_space(10.0);

            ui.group(|ui| {
                ui.set_enabled(self.config.export_midi);
                ui.label("2. WAV-to-MIDI Pitch Extraction Parameters");
                ui.add_space(4.0);

                ui.horizontal(|ui| {
                    ui.label("Silence Gate:");
                    ui.add(
                        egui::Slider::new(&mut self.config.midi_gate_db, -65.0..=-15.0).text("dB"),
                    );
                });
                ui.small("Ignores background noise below this volume level.");

                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label("Pitch Confidence:");
                    ui.add(
                        egui::Slider::new(&mut self.config.midi_clarity, 0.50..=0.95)
                            .text("Clarity"),
                    );
                });
                ui.small("Higher values reject breathiness/percussive noise and keep only clear pitched notes.");

                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label("Min Note Length:");
                    ui.add(
                        egui::Slider::new(&mut self.config.min_note_ms, 15.0..=200.0).text("ms"),
                    );
                });
                ui.small("Filters out brief pitch glitches shorter than this duration.");
            });

            ui.add_space(14.0);

            ui.horizontal(|ui| {
                let can_run = self.config.export_clean_wav || self.config.export_midi;
                if self.is_processing {
                    ui.spinner();
                    ui.label("Processing audio & MIDI pipeline...");
                } else if ui
                    .add_enabled(
                        can_run,
                        egui::Button::new("📁 Select WAV File & Process"),
                    )
                    .clicked()
                {
                    if let Some(path) = rfd::FileDialog::new()
                        .add_filter("WAV Audio", &["wav"])
                        .pick_file()
                    {
                        self.is_processing = true;
                        self.status_message.clear();
                        let cfg = self.config;

                        let (tx, rx) = mpsc::channel();
                        self.rx = Some(rx);

                        thread::spawn(move || {
                            run_dsp_pipeline(path, cfg, tx);
                        });
                    }
                }
            });

            ui.add_space(12.0);
            ui.separator();
            ui.add_space(6.0);

            ui.label("Execution Log:");
            egui::ScrollArea::vertical()
                .max_height(160.0)
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut self.status_message)
                            .font(egui::TextStyle::Monospace)
                            .desired_width(f32::INFINITY),
                    );
                });
        });
    }
}

fn egio_styling(ctx: &egui::Context) {
    let mut style = (*ctx.style()).clone();
    style.visuals.dark_mode = true;
    ctx.set_style(style);
}

/// 2nd-order Butterworth Low-Pass Biquad Filter (Direct Form II Transposed)
struct BiquadLpf {
    b0: f32,
    b1: f32,
    b2: f32,
    a1: f32,
    a2: f32,
    z1: f32,
    z2: f32,
}

impl BiquadLpf {
    fn new(cutoff_hz: f32, sample_rate: f32) -> Self {
        let fc = cutoff_hz.clamp(20.0, sample_rate * 0.49);
        let omega = 2.0 * std::f32::consts::PI * fc / sample_rate;
        let sin_w = omega.sin();
        let cos_w = omega.cos();
        let alpha = sin_w / std::f32::consts::SQRT_2;

        let a0 = 1.0 + alpha;
        let b0 = ((1.0 - cos_w) * 0.5) / a0;
        let b1 = (1.0 - cos_w) / a0;
        let b2 = ((1.0 - cos_w) * 0.5) / a0;
        let a1 = (-2.0 * cos_w) / a0;
        let a2 = (1.0 - alpha) / a0;

        Self {
            b0,
            b1,
            b2,
            a1,
            a2,
            z1: 0.0,
            z2: 0.0,
        }
    }

    #[inline]
    fn process(&mut self, x: f32) -> f32 {
        let y = self.b0 * x + self.z1;
        self.z1 = self.b1 * x - self.a1 * y + self.z2;
        self.z2 = self.b2 * x - self.a2 * y;
        y
    }
}

/// Detects isolated 1-2 sample digital clicks using 2nd-order difference (curvature)
fn declick_channel(channel: &mut [f32], sensitivity: f32) -> usize {
    if channel.len() < 8 {
        return 0;
    }

    let orig = channel.to_vec();
    let mut click_count = 0;

    for i in 3..orig.len() - 3 {
        let predicted = 0.5 * (orig[i - 1] + orig[i + 1]);
        let impulse_error = (orig[i] - predicted).abs();

        let local_activity = 0.25
            * ((orig[i - 1] - orig[i - 2]).abs()
                + (orig[i - 2] - orig[i - 3]).abs()
                + (orig[i + 2] - orig[i + 1]).abs()
                + (orig[i + 3] - orig[i + 2]).abs());

        let neighbor_continuity = (orig[i + 1] - orig[i - 1]).abs();

        if impulse_error > sensitivity
            && impulse_error > local_activity * 4.5
            && neighbor_continuity < impulse_error * 1.2
        {
            channel[i] =
                (-orig[i - 2] + 4.0 * orig[i - 1] + 4.0 * orig[i + 1] - orig[i + 2]) / 6.0;
            click_count += 1;
        }
    }

    click_count
}

#[derive(Debug, Clone, Copy)]
struct MidiNoteEvent {
    note: u8,
    velocity: u8,
    start_sample: usize,
    end_sample: usize,
}

/// YIN pitch detection (Cumulative Mean Normalized Difference Function)
/// Returns `(midi_note_number, rms_amplitude)` if a clear pitch is present in the frame.
fn detect_pitch_yin(
    frame: &[f32],
    sample_rate: f32,
    gate_linear: f32,
    clarity_threshold: f32,
) -> Option<(u8, f32)> {
    let n = frame.len();
    let rms = (frame.iter().map(|&x| x * x).sum::<f32>() / n as f32).sqrt();
    if rms < gate_linear {
        return None;
    }

    // Search range: ~55 Hz (A1) to ~1760 Hz (A6)
    let min_tau = (sample_rate / 1760.0).floor() as usize;
    let max_tau = ((sample_rate / 55.0).ceil() as usize).min(n / 2);
    if min_tau >= max_tau {
        return None;
    }

    let mut diff = vec![0.0f32; max_tau + 1];
    for tau in 1..=max_tau {
        let mut sum = 0.0f32;
        for i in 0..(n - max_tau) {
            let d = frame[i] - frame[i + tau];
            sum += d * d;
        }
        diff[tau] = sum;
    }

    // Cumulative Mean Normalized Difference Function (CMNDF)
    let mut cmndf = vec![1.0f32; max_tau + 1];
    let mut running_sum = 0.0f32;
    for tau in 1..=max_tau {
        running_sum += diff[tau];
        if running_sum > 1e-9 {
            cmndf[tau] = diff[tau] * (tau as f32) / running_sum;
        }
    }

    // Maximum allowed aperiodicity (inverse of clarity)
    let max_aperiodicity = (1.0 - clarity_threshold).clamp(0.05, 0.50);

    // Find the first local minimum below the threshold to prevent octave errors
    let mut best_tau = None;
    let mut tau = min_tau;
    while tau < max_tau {
        if cmndf[tau] < max_aperiodicity {
            while tau + 1 < max_tau && cmndf[tau + 1] < cmndf[tau] {
                tau += 1;
            }
            best_tau = Some(tau);
            break;
        }
        tau += 1;
    }

    // Fallback to global minimum if it still satisfies clarity
    let tau_idx = best_tau.or_else(|| {
        (min_tau..=max_tau)
            .min_by(|&a, &b| cmndf[a].partial_cmp(&cmndf[b]).unwrap())
            .filter(|&t| cmndf[t] < max_aperiodicity * 1.15)
    })?;

    // Parabolic interpolation for sub-sample frequency accuracy
    let refined_tau = if tau_idx > 1 && tau_idx < max_tau {
        let y0 = cmndf[tau_idx - 1];
        let y1 = cmndf[tau_idx];
        let y2 = cmndf[tau_idx + 1];
        let denom = 2.0 * (2.0 * y1 - y2 - y0);
        if denom.abs() > 1e-6 {
            tau_idx as f32 + (y2 - y0) / denom
        } else {
            tau_idx as f32
        }
    } else {
        tau_idx as f32
    };

    let freq = sample_rate / refined_tau;
    let midi_float = 69.0 + 12.0 * (freq / 440.0).log2();
    let midi_note = midi_float.round() as i32;

    if (21..=108).contains(&midi_note) {
        Some((midi_note as u8, rms))
    } else {
        None
    }
}

/// Converts a mono audio signal into discrete MIDI note events
fn extract_midi_notes(
    mono: &[f32],
    sample_rate: f32,
    gate_db: f32,
    clarity: f32,
    min_note_ms: f32,
) -> Vec<MidiNoteEvent> {
    let window_size = ((sample_rate * 0.046) as usize).max(512); // ~46ms analysis window
    let hop_size = (sample_rate * 0.010).round().max(1.0) as usize; // ~10ms hop resolution

    if mono.len() < window_size {
        return Vec::new();
    }

    let gate_linear = 10.0f32.powf(gate_db / 20.0);
    let num_hops = (mono.len() - window_size) / hop_size + 1;

    let mut frame_pitches: Vec<Option<(u8, f32)>> = Vec::with_capacity(num_hops);
    for h in 0..num_hops {
        let start = h * hop_size;
        let frame = &mono[start..start + window_size];
        frame_pitches.push(detect_pitch_yin(frame, sample_rate, gate_linear, clarity));
    }

    // 3-frame median smoothing to bridge 1-frame dropouts or jitter
    let mut smoothed = frame_pitches.clone();
    for i in 1..frame_pitches.len().saturating_sub(1) {
        match (frame_pitches[i - 1], frame_pitches[i], frame_pitches[i + 1]) {
            (Some((n_prev, r_prev)), _, Some((n_next, r_next))) if n_prev == n_next => {
                smoothed[i] = Some((n_prev, 0.5 * (r_prev + r_next)));
            }
            _ => {}
        }
    }

    // Group contiguous frames of the same MIDI pitch into note events
    let min_samples = ((min_note_ms / 1000.0) * sample_rate) as usize;
    let mut notes = Vec::new();
    let mut active: Option<(u8, usize, f32)> = None; // (note, start_hop, peak_rms)

    for (h, &detected) in smoothed.iter().enumerate() {
        match (active, detected) {
            (None, Some((note, rms))) => {
                active = Some((note, h, rms));
            }
            (Some((curr_note, start_h, max_rms)), Some((note, rms))) => {
                if note == curr_note {
                    active = Some((curr_note, start_h, max_rms.max(rms)));
                } else {
                    let start_sample = start_h * hop_size;
                    let end_sample = h * hop_size;
                    if end_sample - start_sample >= min_samples {
                        notes.push(MidiNoteEvent {
                            note: curr_note,
                            velocity: rms_to_velocity(max_rms),
                            start_sample,
                            end_sample,
                        });
                    }
                    active = Some((note, h, rms));
                }
            }
            (Some((curr_note, start_h, max_rms)), None) => {
                let start_sample = start_h * hop_size;
                let end_sample = h * hop_size;
                if end_sample - start_sample >= min_samples {
                    notes.push(MidiNoteEvent {
                        note: curr_note,
                        velocity: rms_to_velocity(max_rms),
                        start_sample,
                        end_sample,
                    });
                }
                active = None;
            }
            (None, None) => {}
        }
    }

    if let Some((curr_note, start_h, max_rms)) = active {
        let start_sample = start_h * hop_size;
        let end_sample = smoothed.len() * hop_size;
        if end_sample - start_sample >= min_samples {
            notes.push(MidiNoteEvent {
                note: curr_note,
                velocity: rms_to_velocity(max_rms),
                start_sample,
                end_sample,
            });
        }
    }

    notes
}

fn rms_to_velocity(rms: f32) -> u8 {
    let db = 20.0 * rms.max(1e-5).log10();
    // Map [-45 dB .. 0 dB] -> [25 .. 127]
    let norm = ((db + 45.0) / 45.0).clamp(0.0, 1.0);
    (25.0 + norm * 102.0).round().clamp(1.0, 127.0) as u8
}

/// Encodes a standard MIDI Variable-Length Quantity (VLQ)
fn push_vlq(buf: &mut Vec<u8>, mut value: u32) {
    let mut bytes = [0u8; 5];
    let mut count = 1;
    bytes[0] = (value & 0x7F) as u8;
    value >>= 7;
    while value > 0 {
        bytes[count] = ((value & 0x7F) as u8) | 0x80;
        value >>= 7;
        count += 1;
    }
    for i in (0..count).rev() {
        buf.push(bytes[i]);
    }
}

/// Writes a Standard MIDI File (Format 0, 120 BPM, 480 PPQ) with zero external crates
fn write_midi_file(path: &Path, notes: &[MidiNoteEvent], sample_rate: f32) -> std::io::Result<()> {
    const TICKS_PER_QUARTER: u16 = 480;
    // At 120 BPM (500,000 microseconds per quarter note), 1 second = 2 beats = 960 ticks
    let ticks_per_sec = (TICKS_PER_QUARTER as f64) * 2.0;

    let mut track_data: Vec<u8> = Vec::new();

    // Tempo Meta Event: 120 BPM (500,000 us/quarter note = 0x07A120)
    push_vlq(&mut track_data, 0);
    track_data.extend_from_slice(&[0xFF, 0x51, 0x03, 0x07, 0xA1, 0x20]);

    // Program Change: Channel 0 -> Acoustic Grand Piano (0)
    push_vlq(&mut track_data, 0);
    track_data.extend_from_slice(&[0xC0, 0x00]);

    // Convert sample positions to MIDI ticks
    let mut last_tick: u32 = 0;
    for event in notes {
        let start_tick =
            ((event.start_sample as f64 / sample_rate as f64) * ticks_per_sec).round() as u32;
        let end_tick = ((event.end_sample as f64 / sample_rate as f64) * ticks_per_sec)
            .round()
            .max((start_tick + 1) as f64) as u32;

        // Note On (0x90)
        let delta_on = start_tick.saturating_sub(last_tick);
        push_vlq(&mut track_data, delta_on);
        track_data.extend_from_slice(&[0x90, event.note, event.velocity]);

        // Note Off (0x80)
        let delta_off = end_tick.saturating_sub(start_tick);
        push_vlq(&mut track_data, delta_off);
        track_data.extend_from_slice(&[0x80, event.note, 0x40]);

        last_tick = end_tick;
    }

    // End of Track Meta Event
    push_vlq(&mut track_data, 0);
    track_data.extend_from_slice(&[0xFF, 0x2F, 0x00]);

    let mut file_bytes: Vec<u8> = Vec::with_capacity(22 + track_data.len());
    // Header Chunk: MThd, length = 6, format = 0, tracks = 1, division = 480 PPQ
    file_bytes.extend_from_slice(b"MThd");
    file_bytes.extend_from_slice(&6u32.to_be_bytes());
    file_bytes.extend_from_slice(&0u16.to_be_bytes());
    file_bytes.extend_from_slice(&1u16.to_be_bytes());
    file_bytes.extend_from_slice(&TICKS_PER_QUARTER.to_be_bytes());

    // Track Chunk: MTrk
    file_bytes.extend_from_slice(b"MTrk");
    file_bytes.extend_from_slice(&(track_data.len() as u32).to_be_bytes());
    file_bytes.extend_from_slice(&track_data);

    let mut file = File::create(path)?;
    file.write_all(&file_bytes)?;
    Ok(())
}

fn run_dsp_pipeline(input_path: PathBuf, cfg: PipelineConfig, tx: Sender<String>) {
    let file_name = input_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("input.wav");
    let _ = tx.send(format!("Loading audio file: {}", file_name));

    let mut reader = match WavReader::open(&input_path) {
        Ok(r) => r,
        Err(e) => {
            let _ = tx.send(format!("❌ Error opening file: {}", e));
            return;
        }
    };

    let spec: WavSpec = reader.spec();
    let sample_rate = spec.sample_rate as f32;
    let channels = spec.channels as usize;

    if channels == 0 {
        let _ = tx.send("❌ Invalid channel count (0).".into());
        return;
    }

    let _ = tx.send(format!(
        "Sample Rate: {} Hz | Channels: {} | Format: {:?} {}-bit",
        spec.sample_rate, channels, spec.sample_format, spec.bits_per_sample
    ));

    let interleaved: Result<Vec<f32>, _> = match (spec.sample_format, spec.bits_per_sample) {
        (SampleFormat::Int, 16) => reader
            .samples::<i16>()
            .map(|s| s.map(|v| v as f32 / 32_768.0))
            .collect(),
        (SampleFormat::Int, 24) => reader
            .samples::<i32>()
            .map(|s| s.map(|v| v as f32 / 8_388_608.0))
            .collect(),
        (SampleFormat::Int, 32) => reader
            .samples::<i32>()
            .map(|s| s.map(|v| v as f32 / 2_147_483_648.0))
            .collect(),
        (SampleFormat::Float, 32) => reader.samples::<f32>().collect(),
        _ => {
            let _ = tx.send(format!(
                "❌ Unsupported format: {:?} {}-bit",
                spec.sample_format, spec.bits_per_sample
            ));
            return;
        }
    };

    let data = match interleaved {
        Ok(d) => d,
        Err(e) => {
            let _ = tx.send(format!("❌ Error reading samples: {}", e));
            return;
        }
    };

    let num_frames = data.len() / channels;

    // De-interleave into separate per-channel buffers
    let mut channel_buffers: Vec<Vec<f32>> = vec![Vec::with_capacity(num_frames); channels];
    for (idx, &sample) in data.iter().enumerate() {
        channel_buffers[idx % channels].push(sample);
    }

    let _ = tx.send("Stage 1: De-clicking transient spikes per channel...".into());
    let mut total_clicks = 0;
    for ch_buf in channel_buffers.iter_mut() {
        total_clicks += declick_channel(ch_buf, cfg.click_sensitivity);
    }
    let _ = tx.send(format!(" -> Repaired {} click anomalies.", total_clicks));

    let _ = tx.send(format!(
        "Stage 2: Applying 2nd-order Butterworth LPF ({:.0} Hz)...",
        cfg.lpf_cutoff
    ));
    for ch_buf in channel_buffers.iter_mut() {
        let mut lpf = BiquadLpf::new(cfg.lpf_cutoff, sample_rate);
        for sample in ch_buf.iter_mut() {
            *sample = lpf.process(*sample);
        }
    }

    let stem = input_path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("output");

    // Optional WAV export
    if cfg.export_clean_wav {
        let output_wav = input_path.with_file_name(format!("{}_cleaned.wav", stem));
        let mut writer = match WavWriter::create(&output_wav, spec) {
            Ok(w) => w,
            Err(e) => {
                let _ = tx.send(format!("❌ Error creating output WAV: {}", e));
                return;
            }
        };

        for frame in 0..num_frames {
            for ch in 0..channels {
                let s = channel_buffers[ch][frame].clamp(-1.0, 1.0);
                let write_res = match (spec.sample_format, spec.bits_per_sample) {
                    (SampleFormat::Int, 16) => writer.write_sample((s * 32_767.0).round() as i16),
                    (SampleFormat::Int, 24) => {
                        writer.write_sample((s * 8_388_607.0).round() as i32)
                    }
                    (SampleFormat::Int, 32) => {
                        writer.write_sample((s as f64 * 2_147_483_647.0).round() as i32)
                    }
                    (SampleFormat::Float, 32) => writer.write_sample(s),
                    _ => unreachable!(),
                };
                if let Err(e) = write_res {
                    let _ = tx.send(format!("❌ Error writing sample: {}", e));
                    return;
                }
            }
        }

        if let Err(e) = writer.finalize() {
            let _ = tx.send(format!("❌ Error saving WAV file: {}", e));
            return;
        }
        let _ = tx.send(format!(" -> Saved cleaned WAV: {}", output_wav.display()));
    }

    // MIDI pitch transcription & export
    if cfg.export_midi {
        let _ = tx.send("Stage 3: Transcribing audio pitches to MIDI (YIN algorithm)...".into());

        // Mix cleaned channels down to mono for pitch detection
        let mut mono = vec![0.0f32; num_frames];
        let inv_ch = 1.0 / (channels as f32);
        for ch_buf in &channel_buffers {
            for (i, &s) in ch_buf.iter().enumerate() {
                mono[i] += s * inv_ch;
            }
        }

        let midi_notes = extract_midi_notes(
            &mono,
            sample_rate,
            cfg.midi_gate_db,
            cfg.midi_clarity,
            cfg.min_note_ms,
        );
        let _ = tx.send(format!(" -> Extracted {} MIDI notes.", midi_notes.len()));

        let output_mid = input_path.with_file_name(format!("{}.mid", stem));
        if let Err(e) = write_midi_file(&output_mid, &midi_notes, sample_rate) {
            let _ = tx.send(format!("❌ Error writing MIDI file: {}", e));
            return;
        }
        let _ = tx.send(format!(" -> Saved MIDI file: {}", output_mid.display()));
    }

    let _ = tx.send("✨ Processing complete!".into());
}
