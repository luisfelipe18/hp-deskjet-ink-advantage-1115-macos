mod cups;
mod ledm;
mod pcl;
mod usb;

use clap::Parser;
use cups::{
    CupsQueueInfo, TcpPrintSession, clear_cups_queue, detect_router_print_server,
    ensure_ppd_file_in_workspace, install_cups_driver_macos, install_router_cups_driver_macos,
    list_cups_queues, print_file_via_cups_queue, probe_tcp_print_server, send_raw_pcl_over_tcp,
};
use eframe::egui;
use ledm::{
    MaintenanceJob, PrinterTelemetry, cancel_active_jobs, query_printer_telemetry,
    start_calibration_session, submit_manual_alignment_values, trigger_maintenance_job,
};
use pcl::{
    ColorMode, PaperSize, PclJobConfig, PrintQuality, encode_pdf_page_to_pcl3gui_mode10,
    encode_rgba_to_pcl3gui_mode10, get_pdf_page_count, is_pdf_file, load_file_as_page_images,
    parse_page_range, render_rust_test_page,
};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::thread;
use usb::HpUsbConnection;

#[derive(Parser, Debug)]
#[command(
    name = "hp-deskjet-1115",
    version = "0.1.0",
    about = "Utilidad de control, mantenimiento e impresión para HP DeskJet Ink Advantage 1115"
)]
struct CliArgs {
    /// Mostrar estado completo, niveles de tinta y contadores por terminal
    #[arg(long)]
    status: bool,

    /// Ejecutar limpieza de cabezales (1 = Estándar, 2 = Profunda, 3 = Rodillos, 4 = Manchas)
    #[arg(long)]
    clean: Option<u8>,

    /// Imprimir página de diagnóstico interna de la impresora
    #[arg(long)]
    test_page: bool,

    /// Imprimir página de prueba de color y alineación
    #[arg(long)]
    color_test_page: bool,

    /// Iniciar alineación de cartuchos e imprimir hoja de patrones
    #[arg(long)]
    align: bool,

    /// Guardar valores de alineación manual (formato: A,B,C,D por ejemplo 5,5,5,5)
    #[arg(long)]
    align_values: Option<String>,

    /// Imprimir un archivo PDF, PNG o JPEG
    #[arg(long)]
    print: Option<PathBuf>,

    /// Rango de páginas a imprimir (ej. 1-16 o 1,3,5-8)
    #[arg(long)]
    pages: Option<String>,

    /// Usar modo escala de grises (solo cartucho negro)
    #[arg(long)]
    grayscale: bool,

    /// Imprimir negro compuesto usando solo el cartucho Tricolor (CMY) sin tinta negra
    #[arg(long)]
    cmy_black: bool,

    /// Instalar el filtro y PPD en las colas de impresión del sistema
    #[arg(long)]
    install_cups: bool,

    /// Inspeccionar respuestas XML internas
    #[arg(long)]
    inspect_ledm: bool,

    /// Cancelar trabajos atascados en la memoria de la impresora y reiniciar el canal USB
    #[arg(long)]
    cancel_jobs: bool,

    /// Enviar archivo raw PCL3GUI directamente por USB
    #[arg(long)]
    send_raw: Option<PathBuf>,

    /// Imprimir por red TCP (puerto 9100 del router, ej. 192.168.1.1:9100)
    #[arg(long)]
    net: Option<String>,
}

fn main() {
    let args = CliArgs::parse();

    let has_cli_action = args.status
        || args.clean.is_some()
        || args.test_page
        || args.color_test_page
        || args.align
        || args.align_values.is_some()
        || args.print.is_some()
        || args.install_cups
        || args.inspect_ledm
        || args.cancel_jobs
        || args.send_raw.is_some();

    if has_cli_action {
        if let Err(e) = run_cli(args) {
            eprintln!("Error: {e}");
            std::process::exit(1);
        }
        return;
    }

    if let Ok(cwd) = std::env::current_dir() {
        let _ = ensure_ppd_file_in_workspace(&cwd);
    }

    let native_options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("HP DeskJet Ink Advantage 1115")
            .with_inner_size([980.0, 720.0])
            .with_min_inner_size([640.0, 480.0]),
        ..Default::default()
    };

    if let Err(e) = eframe::run_native(
        "HP DeskJet Ink Advantage 1115",
        native_options,
        Box::new(|cc| Ok(Box::new(HpControlCenterApp::new(cc)))),
    ) {
        eprintln!("Error al iniciar la interfaz gráfica: {e}");
    }
}

fn resolve_cli_color_mode(args: &CliArgs) -> ColorMode {
    match (args.cmy_black, args.grayscale) {
        (true, true) => ColorMode::GrayscaleCMY,
        (true, false) => ColorMode::CompositeCMY,
        (false, true) => ColorMode::GrayscaleK,
        (false, false) => ColorMode::ColorKCMY,
    }
}

fn run_cli(args: CliArgs) -> Result<(), String> {
    let cli_color_mode = resolve_cli_color_mode(&args);
    if args.install_cups {
        let cwd = std::env::current_dir().map_err(|e| e.to_string())?;
        if let Some(net) = args.net.as_deref() {
            let msg = install_router_cups_driver_macos(&cwd, net)?;
            println!("{msg}");
            return Ok(());
        }
        let conn = HpUsbConnection::open()?;
        let msg = install_cups_driver_macos(&cwd, &conn.info.serial_number)?;
        println!("{msg}");
        return Ok(());
    }

    if let Some(net_addr) = args.net.clone() {
        if args.status {
            let reachable = probe_tcp_print_server(&net_addr);
            println!("Router / Red ({net_addr}): {}", if reachable { "DETECTADA (Puerto 9100 abierto)" } else { "No responde" });
        }
        if args.color_test_page {
            let cfg = PclJobConfig {
                color_mode: cli_color_mode,
                quality: PrintQuality::Normal300,
                paper_size: PaperSize::A4,
                use_pcl3gui_header: true,
                copies: 1,
            };
            let img = render_rust_test_page("RED-9100", "Router", &cfg);
            let pcl_bytes = encode_rgba_to_pcl3gui_mode10(&img, &cfg)?;
            let sent = send_raw_pcl_over_tcp(&net_addr, &pcl_bytes)?;
            println!("Página de prueba enviada por red a {net_addr} ({sent} bytes).");
        }
        if let Some(path) = args.print {
            let cfg = PclJobConfig {
                color_mode: cli_color_mode,
                quality: PrintQuality::Normal300,
                paper_size: PaperSize::A4,
                use_pcl3gui_header: true,
                copies: 1,
            };
            let range_spec = args.pages.as_deref().unwrap_or("");
            if is_pdf_file(&path) {
                let total = get_pdf_page_count(&path)?;
                let page_nums = parse_page_range(range_spec, total)?;
                let mut tcp = TcpPrintSession::connect(&net_addr)?;
                let mut sent = 0usize;
                for (idx, pnum) in page_nums.iter().enumerate() {
                    println!(
                        "Procesando y enviando página {} ({} de {}) a {}...",
                        pnum,
                        idx + 1,
                        page_nums.len(),
                        net_addr
                    );
                    let pcl_bytes = encode_pdf_page_to_pcl3gui_mode10(&path, *pnum, &cfg)?;
                    sent += tcp.write_all(&pcl_bytes)?;
                }
                println!(
                    "Documento '{}' ({} páginas, {} bytes) enviado a {net_addr}.",
                    path.display(),
                    page_nums.len(),
                    sent
                );
            } else {
                let pages = load_file_as_page_images(&path, cfg.quality.dpi())?;
                let mut tcp = TcpPrintSession::connect(&net_addr)?;
                let mut sent = 0usize;
                for page in &pages {
                    let pcl_bytes = encode_rgba_to_pcl3gui_mode10(page, &cfg)?;
                    sent += tcp.write_all(&pcl_bytes)?;
                }
                println!(
                    "Documento '{}' ({} páginas, {} bytes) enviado a {net_addr}.",
                    path.display(),
                    pages.len(),
                    sent
                );
            }
        }
        return Ok(());
    }

    let conn = match HpUsbConnection::open() {
        Ok(c) => c,
        Err(usb_err) => {
            if args.status {
                if let Some(router) = detect_router_print_server("192.168.1.1:9100") {
                    println!("USB: no conectada.");
                    println!("Red / Router detectado en {router} (impresión disponible por puerto 9100; niveles de tinta y limpieza requieren USB).");
                    return Ok(());
                }
            }
            return Err(usb_err);
        }
    };

    if args.cancel_jobs {
        let msg = cancel_active_jobs(&conn)?;
        println!("{msg}");
    }

    if let Some(raw_path) = args.send_raw {
        let data = std::fs::read(&raw_path)
            .map_err(|e| format!("No se pudo leer '{}': {e}", raw_path.display()))?;
        let sent = conn.send_raw_print_data(&data)?;
        println!("Enviados {sent} bytes desde '{}'.", raw_path.display());
    }

    if args.inspect_ledm {
        for ep in [
            "/Jobs/JobList",
            "/DevMgmt/ProductStatusDyn.xml",
        ] {
            println!("=== GET {ep} ===");
            match conn.ledm_request("GET", ep, None) {
                Ok((code, body)) => println!("HTTP {code}:\n{body}\n"),
                Err(e) => println!("Error: {e}\n"),
            }
        }
    }

    if args.status {
        let t = query_printer_telemetry(&conn);
        println!("================================================================");
        println!("  {} ({})", t.make_and_model, t.product_number);
        println!("================================================================");
        println!("  Serie             : {}", t.serial_number);
        println!(
            "  Firmware          : {} ({})",
            t.firmware_revision, t.firmware_date
        );
        println!("  Estado            : {:?}", t.status_categories);
        println!("  Alertas           : {:?}", t.alerts);
        println!("  Calibración       : {}", t.calibration_state);
        println!(
            "  Páginas Impresas  : {} (Atascos: {}, Fallos toma: {})",
            t.total_impressions, t.jam_events, t.mispick_events
        );
        println!("----------------------------------------------------------------");
        println!("  NIVELES DE TINTA:");
        for c in &t.cartridges {
            if c.measured_state.eq_ignore_ascii_case("unknown")
                || c.state.to_ascii_lowercase().contains("refilled")
            {
                println!(
                    "  • {:<34} [recargado: nivel físico no medible]  (Estado: {}, Marca: {})",
                    c.name, c.state, c.brand
                );
            } else {
                let bar_len = (c.level_percent as usize) / 5;
                let bar = format!(
                    "{}{}",
                    "█".repeat(bar_len),
                    "░".repeat(20usize.saturating_sub(bar_len))
                );
                println!(
                    "  • {:<34} [{}] {:>3}%  (Estado: {}, Marca: {})",
                    c.name, bar, c.level_percent, c.measured_state, c.brand
                );
            }
        }
        println!("================================================================");
    }

    if let Some(level) = args.clean {
        let job = match level {
            1 => MaintenanceJob::CleanLevel1,
            2 => MaintenanceJob::CleanLevel2,
            3 => MaintenanceJob::PaperFeedCleaning,
            _ => MaintenanceJob::RibSmearCleaning,
        };
        let msg = trigger_maintenance_job(&conn, job)?;
        println!("{msg}");
    }

    if args.test_page {
        let msg = trigger_maintenance_job(&conn, MaintenanceJob::DiagnosticsPage)?;
        println!("{msg}");
    }

    if args.align {
        let msg = start_calibration_session(&conn)?;
        println!("{msg}");
    }

    if let Some(vals) = args.align_values {
        let parts: Vec<u32> = vals
            .split(',')
            .filter_map(|s| s.trim().parse::<u32>().ok())
            .collect();
        if parts.len() != 4 {
            return Err(
                "Formato inválido para --align-values. Usa A,B,C,D (ej. 5,5,5,5)".to_string(),
            );
        }
        let msg = submit_manual_alignment_values(&conn, parts[0], parts[1], parts[2], parts[3])?;
        println!("{msg}");
    }

    if args.color_test_page {
        let cfg = PclJobConfig {
            color_mode: cli_color_mode,
            quality: PrintQuality::Normal300,
            paper_size: PaperSize::A4,
            use_pcl3gui_header: true,
            copies: 1,
        };
        let t = query_printer_telemetry(&conn);
        let img = render_rust_test_page(&t.serial_number, &t.firmware_revision, &cfg);
        let pcl_bytes = encode_rgba_to_pcl3gui_mode10(&img, &cfg)?;
        let sent = conn.send_raw_print_data(&pcl_bytes)?;
        println!("Página de prueba enviada correctamente ({sent} bytes).");
    }

    if let Some(path) = args.print {
        let cfg = PclJobConfig {
            color_mode: cli_color_mode,
            quality: PrintQuality::Normal300,
            paper_size: PaperSize::A4,
            use_pcl3gui_header: true,
            copies: 1,
        };
        let range_spec = args.pages.as_deref().unwrap_or("");
        if is_pdf_file(&path) {
            let total = get_pdf_page_count(&path)?;
            let page_nums = parse_page_range(range_spec, total)?;
            let session = conn.begin_print_session()?;
            let mut sent = 0usize;

            for (idx, pnum) in page_nums.iter().enumerate() {
                println!(
                    "Procesando y enviando página {} ({} de {})...",
                    pnum,
                    idx + 1,
                    page_nums.len()
                );
                let pcl_bytes = encode_pdf_page_to_pcl3gui_mode10(&path, *pnum, &cfg)?;
                sent += session.write_all(&pcl_bytes)?;
            }

            println!(
                "Documento '{}' ({} páginas, {} bytes) enviado correctamente.",
                path.display(),
                page_nums.len(),
                sent
            );
        } else {
            let pages = load_file_as_page_images(&path, cfg.quality.dpi())?;
            let session = conn.begin_print_session()?;
            let mut sent = 0usize;
            for page in &pages {
                let pcl_bytes = encode_rgba_to_pcl3gui_mode10(page, &cfg)?;
                sent += session.write_all(&pcl_bytes)?;
            }
            println!(
                "Documento '{}' ({} páginas, {} bytes) enviado a la impresora.",
                path.display(),
                pages.len(),
                sent
            );
        }
    }

    Ok(())
}

// ============================================================================
// GUI Application
// ============================================================================

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActiveTab {
    DashboardInk,
    MaintenanceCleaning,
    CartridgeAlignment,
    TestPageAndPrint,
    MacOsCupsDriver,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum PrintTargetMode {
    UsbDirect,
    NetworkTcp9100,
    CupsQueue,
}

struct MaintenanceProgress {
    job: MaintenanceJob,
    started_at: std::time::Instant,
    fraction: f32,
    stage_text: String,
}

enum WorkerEvent {
    TelemetryUpdated(Result<PrinterTelemetry, String>, Option<String>),
    ActionCompleted(Result<String, String>),
    ProgressUpdate(String),
    MaintenanceProgressUpdate(MaintenanceJob, f32, String),
    FilePicked(String),
    CupsQueuesUpdated(Vec<CupsQueueInfo>),
}

struct HpControlCenterApp {
    active_tab: ActiveTab,
    telemetry: PrinterTelemetry,
    router_detected: Option<String>,
    cups_queues: Vec<CupsQueueInfo>,
    busy: bool,
    status_banner: String,
    is_error_banner: bool,
    maintenance_progress: Option<MaintenanceProgress>,
    last_maintenance_result: Option<(String, bool)>,
    logs: Vec<String>,

    // Alignment tab state
    align_a: u32,
    align_b: u32,
    align_c: u32,
    align_d: u32,

    // Print tab state
    print_target: PrintTargetMode,
    network_addr: String,
    selected_cups_queue: String,
    print_color_mode: ColorMode,
    print_quality: PrintQuality,
    print_paper_size: PaperSize,
    print_use_pcl3gui_hdr: bool,
    print_copies: u16,
    file_path_input: String,
    page_range_input: String,

    auto_poll: bool,
    last_poll: std::time::Instant,

    tx: Sender<WorkerEvent>,
    rx: Receiver<WorkerEvent>,
}

impl HpControlCenterApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        let mut visuals = egui::Visuals::dark();
        visuals.panel_fill = egui::Color32::from_rgb(16, 20, 28);
        visuals.window_fill = egui::Color32::from_rgb(22, 27, 36);
        visuals.widgets.noninteractive.bg_fill = egui::Color32::from_rgb(26, 32, 44);
        cc.egui_ctx.set_visuals(visuals);

        let queues = list_cups_queues();
        let default_queue = queues
            .first()
            .map(|q| q.name.clone())
            .unwrap_or_else(|| "HP_DeskJet_1115".to_string());

        let (tx, rx) = mpsc::channel();
        let mut app = Self {
            active_tab: ActiveTab::DashboardInk,
            telemetry: PrinterTelemetry::default(),
            router_detected: None,
            cups_queues: queues,
            busy: false,
            status_banner: "Conectando con la impresora...".to_string(),
            is_error_banner: false,
            maintenance_progress: None,
            last_maintenance_result: None,
            logs: Vec::new(),
            align_a: 5,
            align_b: 5,
            align_c: 5,
            align_d: 5,
            print_target: PrintTargetMode::UsbDirect,
            network_addr: "192.168.1.1:9100".to_string(),
            selected_cups_queue: default_queue,
            print_color_mode: ColorMode::ColorKCMY,
            print_quality: PrintQuality::Normal300,
            print_paper_size: PaperSize::A4,
            print_use_pcl3gui_hdr: true,
            print_copies: 1,
            file_path_input: String::new(),
            page_range_input: String::new(),
            auto_poll: true,
            last_poll: std::time::Instant::now(),
            tx,
            rx,
        };

        app.refresh_telemetry();
        app
    }

    fn log(&mut self, msg: impl Into<String>) {
        let m = msg.into();
        self.logs.push(m);
        if self.logs.len() > 80 {
            self.logs.remove(0);
        }
    }

    fn refresh_telemetry(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.last_poll = std::time::Instant::now();
        self.is_error_banner = false;
        self.status_banner = "Buscando impresora por USB y en la red...".to_string();
        let preferred_net = self.network_addr.clone();
        let tx = self.tx.clone();
        thread::spawn(move || {
            let res = match HpUsbConnection::open() {
                Ok(conn) => (Ok(query_printer_telemetry(&conn)), None),
                Err(usb_err) => {
                    let router = detect_router_print_server(&preferred_net);
                    (Err(usb_err), router)
                }
            };
            let _ = tx.send(WorkerEvent::TelemetryUpdated(res.0, res.1));
            let _ = tx.send(WorkerEvent::CupsQueuesUpdated(list_cups_queues()));
        });
    }

    fn run_cancel_jobs(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.last_poll = std::time::Instant::now();
        self.is_error_banner = false;
        self.status_banner = "Cancelando trabajos activos en la impresora...".to_string();
        self.log("Cancelando trabajos activos...");

        let tx = self.tx.clone();
        thread::spawn(move || {
            let res = HpUsbConnection::open().and_then(|conn| {
                let msg = cancel_active_jobs(&conn)?;
                thread::sleep(std::time::Duration::from_millis(800));
                let _ = tx.send(WorkerEvent::TelemetryUpdated(
                    Ok(query_printer_telemetry(&conn)),
                    None,
                ));
                Ok(msg)
            });
            let _ = tx.send(WorkerEvent::ActionCompleted(res));
        });
    }

    fn run_maintenance(&mut self, job: MaintenanceJob) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.last_poll = std::time::Instant::now();
        self.is_error_banner = false;
        self.last_maintenance_result = None;
        self.maintenance_progress = Some(MaintenanceProgress {
            job,
            started_at: std::time::Instant::now(),
            fraction: 0.08,
            stage_text: "Conectando por USB y enviando orden...".to_string(),
        });
        self.status_banner = format!("Ejecutando '{}'...", job.display_name());
        self.log(format!("Iniciando: {}", job.display_name()));

        let tx = self.tx.clone();
        thread::spawn(move || {
            let res = HpUsbConnection::open().and_then(|conn| {
                let _ = tx.send(WorkerEvent::MaintenanceProgressUpdate(
                    job,
                    0.18,
                    "Enviando solicitud a la impresora...".to_string(),
                ));
                let _accepted = trigger_maintenance_job(&conn, job)?;
                let started_exec = std::time::Instant::now();
                let est_secs = estimated_maintenance_secs(job);
                let _ = tx.send(WorkerEvent::MaintenanceProgressUpdate(
                    job,
                    0.28,
                    "Orden aceptada. Esperando que la impresora inicie la rutina...".to_string(),
                ));

                let mut saw_processing = false;
                let max_wait = std::time::Duration::from_secs(90);
                while started_exec.elapsed() < max_wait {
                    thread::sleep(std::time::Duration::from_millis(1200));
                    let t = query_printer_telemetry(&conn);
                    let is_processing = t
                        .status_categories
                        .iter()
                        .any(|s| s.eq_ignore_ascii_case("processing"));
                    let active_alerts: Vec<&String> = t
                        .alerts
                        .iter()
                        .filter(|a| {
                            !a.eq_ignore_ascii_case("inPowerSave")
                                && !a.to_ascii_lowercase().contains("calibration")
                        })
                        .collect();
                    let _ = tx.send(WorkerEvent::TelemetryUpdated(Ok(t.clone()), None));

                    if is_processing {
                        saw_processing = true;
                        let frac = (0.30
                            + 0.65 * (started_exec.elapsed().as_secs_f32() / est_secs).min(1.0))
                        .min(0.95);
                        let stage = if !active_alerts.is_empty() {
                            format!(
                                "Ejecutando rutina (atención: {})...",
                                active_alerts
                                    .iter()
                                    .map(|s| s.as_str())
                                    .collect::<Vec<_>>()
                                    .join(", ")
                            )
                        } else {
                            "La impresora está ejecutando la rutina...".to_string()
                        };
                        let _ = tx.send(WorkerEvent::MaintenanceProgressUpdate(job, frac, stage));
                    } else if saw_processing
                        || started_exec.elapsed() >= std::time::Duration::from_secs(8)
                    {
                        let _ = tx.send(WorkerEvent::MaintenanceProgressUpdate(
                            job,
                            1.0,
                            "Rutina completada.".to_string(),
                        ));
                        return Ok(format!("{}: completado.", job.display_name()));
                    } else {
                        let frac = (0.28
                            + 0.12 * (started_exec.elapsed().as_secs_f32() / 8.0))
                            .min(0.40);
                        let _ = tx.send(WorkerEvent::MaintenanceProgressUpdate(
                            job,
                            frac,
                            "Iniciando cabezales y mecanismos en la impresora...".to_string(),
                        ));
                    }
                }

                Ok(format!(
                    "{}: enviado (verifica si la impresora terminó la hoja).",
                    job.display_name()
                ))
            });
            let _ = tx.send(WorkerEvent::ActionCompleted(res));
        });
    }

    fn run_start_alignment(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.last_poll = std::time::Instant::now();
        self.is_error_banner = false;
        self.status_banner = "Imprimiendo hoja de alineación...".to_string();
        self.log("Solicitando hoja de alineación de cartuchos...");

        let tx = self.tx.clone();
        thread::spawn(move || {
            let res = HpUsbConnection::open().and_then(|conn| {
                let msg = start_calibration_session(&conn)?;
                thread::sleep(std::time::Duration::from_millis(1200));
                let _ = tx.send(WorkerEvent::TelemetryUpdated(
                    Ok(query_printer_telemetry(&conn)),
                    None,
                ));
                Ok(msg)
            });
            let _ = tx.send(WorkerEvent::ActionCompleted(res));
        });
    }

    fn run_submit_alignment(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.last_poll = std::time::Instant::now();
        self.is_error_banner = false;
        let (a, b, c, d) = (self.align_a, self.align_b, self.align_c, self.align_d);
        self.status_banner =
            format!("Guardando valores de alineación (A={a}, B={b}, C={c}, D={d})...");
        self.log(format!(
            "Aplicando alineación A={a}, B={b}, C={c}, D={d}"
        ));

        let tx = self.tx.clone();
        thread::spawn(move || {
            let res = HpUsbConnection::open().and_then(|conn| {
                let msg = submit_manual_alignment_values(&conn, a, b, c, d)?;
                thread::sleep(std::time::Duration::from_millis(800));
                let _ = tx.send(WorkerEvent::TelemetryUpdated(
                    Ok(query_printer_telemetry(&conn)),
                    None,
                ));
                Ok(msg)
            });
            let _ = tx.send(WorkerEvent::ActionCompleted(res));
        });
    }

    fn run_color_test_page(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.last_poll = std::time::Instant::now();
        self.is_error_banner = false;
        let cfg = PclJobConfig {
            color_mode: self.print_color_mode,
            quality: self.print_quality,
            paper_size: self.print_paper_size,
            use_pcl3gui_header: self.print_use_pcl3gui_hdr,
            copies: self.print_copies,
        };
        let fw = self.telemetry.firmware_revision.clone();
        let sn = self.telemetry.serial_number.clone();
        let target_mode = self.print_target;
        let net_addr = self.network_addr.trim().to_string();
        let queue_name = self.selected_cups_queue.clone();

        self.status_banner = "Enviando página de prueba a la impresora...".to_string();
        self.log("Generando página de prueba de color y alineación...");

        let tx = self.tx.clone();
        thread::spawn(move || {
            let img = render_rust_test_page(&sn, &fw, &cfg);
            let res = match target_mode {
                PrintTargetMode::UsbDirect => encode_rgba_to_pcl3gui_mode10(&img, &cfg)
                    .and_then(|pcl_data| {
                        let conn = HpUsbConnection::open()?;
                        let bytes = conn.send_raw_print_data(&pcl_data)?;
                        thread::sleep(std::time::Duration::from_millis(1200));
                            let _ = tx.send(WorkerEvent::TelemetryUpdated(
                                Ok(query_printer_telemetry(&conn)),
                                None,
                            ));
                        Ok(format!(
                            "Página de prueba enviada por USB ({bytes} bytes)."
                        ))
                    }),
                PrintTargetMode::NetworkTcp9100 => encode_rgba_to_pcl3gui_mode10(&img, &cfg)
                    .and_then(|pcl_data| {
                        send_raw_pcl_over_tcp(&net_addr, &pcl_data).map(|bytes| {
                            format!(
                                "Página de prueba enviada por red a {net_addr} ({bytes} bytes)."
                            )
                        })
                    }),
                PrintTargetMode::CupsQueue => {
                    let tmp_png = std::env::temp_dir().join("hp1115_test_page.png");
                    img.save(&tmp_png)
                        .map_err(|e| format!("No se pudo guardar imagen temporal: {e}"))
                        .and_then(|_| print_file_via_cups_queue(&queue_name, &tmp_png, None))
                }
            };
            let _ = tx.send(WorkerEvent::ActionCompleted(res));
        });
    }

    fn run_browse_file(&mut self) {
        let tx = self.tx.clone();
        thread::spawn(move || {
            let out = std::process::Command::new("osascript")
                .args([
                    "-e",
                    "POSIX path of (choose file with prompt \"Selecciona el documento PDF o imagen a imprimir\")",
                ])
                .output();
            if let Ok(o) = out {
                if o.status.success() {
                    let p = String::from_utf8_lossy(&o.stdout).trim().to_string();
                    if !p.is_empty() {
                        let _ = tx.send(WorkerEvent::FilePicked(p));
                    }
                }
            }
        });
    }

    fn run_print_file(&mut self) {
        let path_str = self.file_path_input.trim().to_string();
        if path_str.is_empty() {
            self.is_error_banner = true;
            self.status_banner =
                "Ingresa o selecciona la ruta de un archivo PDF, PNG o JPEG primero.".to_string();
            return;
        }
        if self.busy {
            return;
        }
        self.busy = true;
        self.last_poll = std::time::Instant::now();
        self.is_error_banner = false;
        let cfg = PclJobConfig {
            color_mode: self.print_color_mode,
            quality: self.print_quality,
            paper_size: self.print_paper_size,
            use_pcl3gui_header: self.print_use_pcl3gui_hdr,
            copies: self.print_copies,
        };
        let target_mode = self.print_target;
        let net_addr = self.network_addr.trim().to_string();
        let queue_name = self.selected_cups_queue.clone();
        let page_range = self.page_range_input.trim().to_string();

        self.status_banner = format!("Preparando impresión de '{path_str}'...");
        self.log(format!(
            "Imprimiendo archivo: {path_str} (páginas: {})",
            if page_range.is_empty() {
                "todas"
            } else {
                &page_range
            }
        ));

        let tx = self.tx.clone();
        thread::spawn(move || {
            let path = PathBuf::from(&path_str);
            let copies = cfg.copies.max(1);
            let res = match target_mode {
                PrintTargetMode::CupsQueue => {
                    print_file_via_cups_queue(&queue_name, &path, Some(&page_range))
                }
                PrintTargetMode::UsbDirect => {
                    if is_pdf_file(&path) {
                        get_pdf_page_count(&path).and_then(|total| {
                            let page_nums = parse_page_range(&page_range, total)?;
                            let conn = HpUsbConnection::open()?;
                            let session = conn.begin_print_session()?;
                            let mut sent = 0usize;

                            for copy in 0..copies {
                                for (idx, pnum) in page_nums.iter().enumerate() {
                                    let _ = tx.send(WorkerEvent::ProgressUpdate(format!(
                                        "Imprimiendo página {} ({} de {}){}",
                                        pnum,
                                        idx + 1,
                                        page_nums.len(),
                                        if copies > 1 {
                                            format!(" — Copia {}/{}", copy + 1, copies)
                                        } else {
                                            String::new()
                                        }
                                    )));
                                    let pcl_bytes =
                                        encode_pdf_page_to_pcl3gui_mode10(&path, *pnum, &cfg)?;
                                    sent += session.write_all(&pcl_bytes)?;
                                }
                            }
                            drop(session);

                            thread::sleep(std::time::Duration::from_millis(1200));
                            let _ = tx.send(WorkerEvent::TelemetryUpdated(
                                Ok(query_printer_telemetry(&conn)),
                                None,
                            ));
                            Ok(format!(
                                "Se enviaron {} páginas de '{}' por USB ({sent} bytes).",
                                page_nums.len(),
                                path.display()
                            ))
                        })
                    } else {
                        load_file_as_page_images(&path, cfg.quality.dpi()).and_then(|pages| {
                            let conn = HpUsbConnection::open()?;
                            let session = conn.begin_print_session()?;
                            let mut sent = 0usize;
                            for _ in 0..copies {
                                for p in &pages {
                                    let pcl_bytes = encode_rgba_to_pcl3gui_mode10(p, &cfg)?;
                                    sent += session.write_all(&pcl_bytes)?;
                                }
                            }
                            drop(session);

                            thread::sleep(std::time::Duration::from_millis(1200));
                            let _ = tx.send(WorkerEvent::TelemetryUpdated(
                                Ok(query_printer_telemetry(&conn)),
                                None,
                            ));
                            Ok(format!(
                                "Imagen '{}' enviada por USB ({sent} bytes).",
                                path.display()
                            ))
                        })
                    }
                }
                PrintTargetMode::NetworkTcp9100 => {
                    if is_pdf_file(&path) {
                        get_pdf_page_count(&path).and_then(|total| {
                            let page_nums = parse_page_range(&page_range, total)?;
                            let mut tcp = TcpPrintSession::connect(&net_addr)?;
                            let mut sent = 0usize;
                            for copy in 0..copies {
                                for (idx, pnum) in page_nums.iter().enumerate() {
                                    let _ = tx.send(WorkerEvent::ProgressUpdate(format!(
                                        "Enviando página {} ({} de {}){} a {}...",
                                        pnum,
                                        idx + 1,
                                        page_nums.len(),
                                        if copies > 1 {
                                            format!(" — Copia {}/{}", copy + 1, copies)
                                        } else {
                                            String::new()
                                        },
                                        net_addr
                                    )));
                                    let pcl_bytes =
                                        encode_pdf_page_to_pcl3gui_mode10(&path, *pnum, &cfg)?;
                                    sent += tcp.write_all(&pcl_bytes)?;
                                }
                            }
                            Ok(format!(
                                "Se enviaron {} páginas de '{}' a {net_addr} ({sent} bytes).",
                                page_nums.len(),
                                path.display()
                            ))
                        })
                    } else {
                        load_file_as_page_images(&path, cfg.quality.dpi()).and_then(|pages| {
                            let mut tcp = TcpPrintSession::connect(&net_addr)?;
                            let mut sent = 0usize;
                            for _ in 0..copies {
                                for p in &pages {
                                    let pcl_bytes = encode_rgba_to_pcl3gui_mode10(p, &cfg)?;
                                    sent += tcp.write_all(&pcl_bytes)?;
                                }
                            }
                            Ok(format!(
                                "Imagen '{}' enviada a {net_addr} ({sent} bytes).",
                                path.display()
                            ))
                        })
                    }
                }
            };
            let _ = tx.send(WorkerEvent::ActionCompleted(res));
        });
    }

    fn run_install_cups(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.is_error_banner = false;
        self.status_banner = "Instalando driver en el sistema...".to_string();
        let serial_opt =
            if self.telemetry.serial_number != "-" && !self.telemetry.serial_number.is_empty() {
                Some(self.telemetry.serial_number.clone())
            } else {
                None
            };

        let tx = self.tx.clone();
        thread::spawn(move || {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let serial_res = match serial_opt {
                Some(s) => Ok(s),
                None => HpUsbConnection::open().map(|c| c.info.serial_number.clone()),
            };
            let res = serial_res.and_then(|serial| install_cups_driver_macos(&cwd, &serial));
            let _ = tx.send(WorkerEvent::ActionCompleted(res));
            let _ = tx.send(WorkerEvent::CupsQueuesUpdated(list_cups_queues()));
        });
    }
    fn run_install_router_cups(&mut self) {
        if self.busy {
            return;
        }
        self.busy = true;
        self.is_error_banner = false;
        let addr = self.network_addr.trim().to_string();
        self.status_banner = format!("Configurando cola CUPS en macOS hacia {addr}...");

        let tx = self.tx.clone();
        thread::spawn(move || {
            let cwd = std::env::current_dir().unwrap_or_else(|_| PathBuf::from("."));
            let res = install_router_cups_driver_macos(&cwd, &addr);
            let _ = tx.send(WorkerEvent::ActionCompleted(res));
            let _ = tx.send(WorkerEvent::CupsQueuesUpdated(list_cups_queues()));
        });
    }
}

impl eframe::App for HpControlCenterApp {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        while let Ok(ev) = self.rx.try_recv() {
            match ev {
                WorkerEvent::TelemetryUpdated(Ok(t), _) => {
                    self.last_poll = std::time::Instant::now();
                    self.is_error_banner = false;
                    self.router_detected = None;
                    if self.print_target == PrintTargetMode::NetworkTcp9100 {
                        self.print_target = PrintTargetMode::UsbDirect;
                    }
                    if self.maintenance_progress.is_none() {
                        self.busy = false;
                        self.status_banner =
                            format!("Impresora conectada por USB (Serie: {})", t.serial_number);
                    }
                    self.telemetry = t;
                }
                WorkerEvent::TelemetryUpdated(Err(e), router_opt) => {
                    if self.maintenance_progress.is_none() {
                        self.busy = false;
                    }
                    self.last_poll = std::time::Instant::now();
                    self.telemetry.connected = false;
                    self.router_detected = router_opt.clone();
                    if let Some(addr) = router_opt {
                        self.network_addr = addr.clone();
                        if self.print_target == PrintTargetMode::UsbDirect {
                            self.print_target = PrintTargetMode::NetworkTcp9100;
                        }
                        self.is_error_banner = false;
                        self.status_banner = format!(
                            "Impresora detectada en el router ({addr}). Impresión lista; niveles de tinta y limpieza requieren USB."
                        );
                    } else if self.maintenance_progress.is_none() {
                        self.is_error_banner = true;
                        self.status_banner = e;
                    }
                }
                WorkerEvent::ActionCompleted(Ok(msg)) => {
                    self.busy = false;
                    self.last_poll = std::time::Instant::now();
                    self.is_error_banner = false;
                    if self.maintenance_progress.take().is_some() {
                        self.last_maintenance_result = Some((msg.clone(), false));
                    }
                    self.status_banner = msg.clone();
                    self.log(msg);
                }
                WorkerEvent::ActionCompleted(Err(e)) => {
                    self.busy = false;
                    self.last_poll = std::time::Instant::now();
                    self.is_error_banner = true;
                    if self.maintenance_progress.take().is_some() {
                        self.last_maintenance_result = Some((e.clone(), true));
                    }
                    self.status_banner = e.clone();
                    self.log(format!("Error: {e}"));
                }
                WorkerEvent::ProgressUpdate(msg) => {
                    self.is_error_banner = false;
                    self.status_banner = msg;
                }
                WorkerEvent::MaintenanceProgressUpdate(job, fraction, stage_text) => {
                    self.is_error_banner = false;
                    self.status_banner = format!("{}: {}", job.display_name(), stage_text);
                    if let Some(mp) = &mut self.maintenance_progress {
                        mp.job = job;
                        mp.fraction = fraction.clamp(0.0, 1.0);
                        mp.stage_text = stage_text;
                    } else {
                        self.maintenance_progress = Some(MaintenanceProgress {
                            job,
                            started_at: std::time::Instant::now(),
                            fraction: fraction.clamp(0.0, 1.0),
                            stage_text,
                        });
                    }
                }
                WorkerEvent::FilePicked(path) => {
                    self.file_path_input = path;
                }
                WorkerEvent::CupsQueuesUpdated(q) => {
                    self.cups_queues = q;
                }
            }
        }

        let poll_interval = if self.telemetry.connected { 5 } else { 25 };
        if self.auto_poll
            && !self.busy
            && self.last_poll.elapsed() >= std::time::Duration::from_secs(poll_interval)
        {
            self.refresh_telemetry();
        }
        let repaint_delay = if self.busy || self.maintenance_progress.is_some() {
            std::time::Duration::from_millis(250)
        } else {
            std::time::Duration::from_secs(2)
        };
        ui.ctx().request_repaint_after(repaint_delay);

        // Top Header Panel
        egui::Panel::top("top_header")
            .frame(
                egui::Frame::NONE
                    .fill(egui::Color32::from_rgb(10, 52, 96))
                    .inner_margin(egui::Margin::symmetric(18, 12)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(
                        egui::RichText::new("🖨️ HP DeskJet Ink Advantage 1115")
                            .size(19.0)
                            .strong()
                            .color(egui::Color32::WHITE),
                    );
                    ui.add_space(12.0);

                    draw_usb_indicator(
                        ui,
                        self.telemetry.connected,
                        &self.telemetry.serial_number,
                    );
                    ui.add_space(4.0);
                    draw_wifi_indicator(ui, self.router_detected.as_deref());

                    ui.add_space(8.0);
                    let btn = ui.add_enabled(
                        !self.busy,
                        egui::Button::new(egui::RichText::new("🔄 Actualizar").strong())
                            .min_size(egui::vec2(110.0, 28.0)),
                    );
                    if btn.clicked() {
                        self.refresh_telemetry();
                    }

                    if self
                        .telemetry
                        .status_categories
                        .iter()
                        .any(|s| s.eq_ignore_ascii_case("processing"))
                    {
                        ui.add_space(6.0);
                        let cancel_btn = ui.add_enabled(
                            !self.busy,
                            egui::Button::new(
                                egui::RichText::new("⏹ Cancelar trabajo")
                                    .color(egui::Color32::WHITE)
                                    .strong(),
                            )
                            .fill(egui::Color32::from_rgb(185, 28, 28))
                            .min_size(egui::vec2(130.0, 28.0)),
                        );
                        if cancel_btn.clicked() {
                            self.run_cancel_jobs();
                        }
                    }

                    ui.add_space(6.0);
                    ui.checkbox(&mut self.auto_poll, "Auto-actualizar");
                });

                ui.add_space(4.0);
                let subline = if self.telemetry.connected {
                    let st = if self.telemetry.status_categories.is_empty() {
                        "Lista".to_string()
                    } else {
                        self.telemetry.status_categories.join(", ")
                    };
                    format!(
                        "Serie: {}  •  Estado: {}  •  Páginas impresas: {}",
                        self.telemetry.serial_number, st, self.telemetry.total_impressions
                    )
                } else if let Some(addr) = &self.router_detected {
                    format!(
                        "Servidor de impresión del router activo en {addr}  •  Listo para imprimir desde la pestaña Imprimir"
                    )
                } else {
                    "No se detectó la impresora por USB ni en el puerto 9100 del router".to_string()
                };
                ui.label(
                    egui::RichText::new(subline)
                        .size(12.0)
                        .color(egui::Color32::from_rgb(200, 226, 255)),
                );
            });

        // Navigation Bar
        egui::Panel::top("nav_tabs")
            .frame(
                egui::Frame::NONE
                    .fill(egui::Color32::from_rgb(22, 28, 40))
                    .inner_margin(egui::Margin::symmetric(18, 8)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(8.0, 6.0);
                    let tabs = [
                        (ActiveTab::DashboardInk, "💧 Tinta y Estado"),
                        (ActiveTab::MaintenanceCleaning, "🧹 Mantenimiento"),
                        (ActiveTab::CartridgeAlignment, "🎯 Alineación"),
                        (ActiveTab::TestPageAndPrint, "📄 Imprimir"),
                        (ActiveTab::MacOsCupsDriver, "🌐 Conexión y Colas"),
                    ];
                    for (tab, label) in tabs {
                        let selected = self.active_tab == tab;
                        let fill = if selected {
                            egui::Color32::from_rgb(2, 132, 199)
                        } else {
                            egui::Color32::from_rgb(34, 43, 58)
                        };
                        let btn = egui::Button::new(
                            egui::RichText::new(label)
                                .size(13.5)
                                .strong()
                                .color(egui::Color32::WHITE),
                        )
                        .fill(fill)
                        .corner_radius(6.0)
                        .min_size(egui::vec2(140.0, 32.0));
                        if ui.add(btn).clicked() {
                            self.active_tab = tab;
                        }
                    }
                });
            });

        // Persistent Global Hardware Alert Banner (only shown when an actual hardware problem exists)
        let has_cartridge_error = self
            .telemetry
            .alerts
            .iter()
            .chain(self.telemetry.status_categories.iter())
            .any(|s| {
                let l = s.to_ascii_lowercase();
                l.contains("cartridge") || l.contains("consumable") || l.contains("failure")
            });
        let has_carriage_jam = self
            .telemetry
            .alerts
            .iter()
            .chain(self.telemetry.status_categories.iter())
            .any(|s| {
                let l = s.to_ascii_lowercase();
                l.contains("jam") || l.contains("carriage")
            });
        let has_paper_out = self
            .telemetry
            .alerts
            .iter()
            .chain(self.telemetry.status_categories.iter())
            .any(|s| {
                let l = s.to_ascii_lowercase();
                l.contains("paper") || l.contains("media")
            });
        let has_door_open = self
            .telemetry
            .alerts
            .iter()
            .chain(self.telemetry.status_categories.iter())
            .any(|s| {
                let l = s.to_ascii_lowercase();
                l.contains("door") || l.contains("cover")
            });

        if has_cartridge_error || has_carriage_jam || has_paper_out || has_door_open {
            egui::Panel::top("global_hardware_alert_banner")
                .frame(
                    egui::Frame::NONE
                        .fill(egui::Color32::from_rgb(153, 27, 27))
                        .inner_margin(egui::Margin::symmetric(18, 10)),
                )
                .show_inside(ui, |ui| {
                    let (title, detail) = if has_cartridge_error {
                        (
                            "🚨 Error de cartucho detectado",
                            "Uno de los cartuchos presenta fallo o mal contacto. Verifica que estén bien encajados en sus casquillos.",
                        )
                    } else if has_carriage_jam {
                        (
                            "🚨 Obstrucción o atasco del carro de cartuchos",
                            "Revisa que no haya papel ni objetos bloqueando el recorrido de los cartuchos y presiona el botón Reanudar.",
                        )
                    } else if has_door_open {
                        (
                            "🚨 Tapa frontal abierta",
                            "Cierra la tapa de acceso a los cartuchos para continuar.",
                        )
                    } else {
                        (
                            "🚨 Sin papel o problema de alimentación",
                            "Coloca papel en la bandeja de entrada y presiona el botón Reanudar en la impresora.",
                        )
                    };
                    ui.label(
                        egui::RichText::new(title)
                            .color(egui::Color32::WHITE)
                            .size(14.0)
                            .strong(),
                    );
                    ui.label(
                        egui::RichText::new(detail)
                            .color(egui::Color32::from_rgb(254, 226, 226))
                            .size(12.0),
                    );
                });
        }

        // Bottom Status Bar
        egui::Panel::bottom("bottom_status")
            .frame(
                egui::Frame::NONE
                    .fill(egui::Color32::from_rgb(13, 17, 24))
                    .inner_margin(egui::Margin::symmetric(18, 10)),
            )
            .show_inside(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    if self.busy {
                        ui.spinner();
                    }
                    if let Some(mp) = &self.maintenance_progress {
                        ui.add(
                            egui::ProgressBar::new(mp.fraction)
                                .desired_width(170.0)
                                .show_percentage()
                                .animate(true),
                        );
                    }
                    let color = if self.is_error_banner {
                        egui::Color32::from_rgb(248, 113, 113)
                    } else {
                        egui::Color32::from_rgb(125, 211, 252)
                    };
                    ui.label(
                        egui::RichText::new(&self.status_banner)
                            .color(color)
                            .strong(),
                    );
                });
            });

        // Main Scrollable Content Area
        egui::CentralPanel::default()
            .frame(
                egui::Frame::NONE
                    .fill(egui::Color32::from_rgb(16, 20, 28))
                    .inner_margin(egui::Margin::symmetric(20, 16)),
            )
            .show_inside(ui, |ui| {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .show(ui, |ui| {
                        match self.active_tab {
                            ActiveTab::DashboardInk => self.render_dashboard_tab(ui),
                            ActiveTab::MaintenanceCleaning => self.render_maintenance_tab(ui),
                            ActiveTab::CartridgeAlignment => self.render_alignment_tab(ui),
                            ActiveTab::TestPageAndPrint => self.render_test_and_print_tab(ui),
                            ActiveTab::MacOsCupsDriver => self.render_cups_driver_tab(ui),
                        }

                        if !self.logs.is_empty() {
                            ui.add_space(16.0);
                            ui.collapsing("📋 Historial de acciones", |ui| {
                                egui::Frame::NONE
                                    .fill(egui::Color32::from_rgb(11, 14, 20))
                                    .corner_radius(6.0)
                                    .inner_margin(egui::Margin::same(10))
                                    .show(ui, |ui| {
                                        for line in self.logs.iter().rev().take(10) {
                                            ui.label(
                                                egui::RichText::new(line)
                                                    .monospace()
                                                    .size(11.5)
                                                    .color(egui::Color32::from_rgb(180, 195, 210)),
                                            );
                                        }
                                    });
                            });
                        }
                    });
            });
    }
}

impl HpControlCenterApp {
    fn render_dashboard_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Niveles de Tinta");
        ui.add_space(10.0);

        let cmy_cart = self
            .telemetry
            .cartridges
            .iter()
            .find(|c| c.label_code == "CMY")
            .cloned()
            .unwrap_or(ledm::CartridgeStatus {
                station: 0,
                label_code: "CMY".to_string(),
                name: "Cartucho Tricolor".to_string(),
                selectability: "664XL".to_string(),
                level_percent: 0,
                state: "-".to_string(),
                measured_state: "-".to_string(),
                brand: "HP".to_string(),
                unique_id: "-".to_string(),
            });

        let k_cart = self
            .telemetry
            .cartridges
            .iter()
            .find(|c| c.label_code == "K")
            .cloned()
            .unwrap_or(ledm::CartridgeStatus {
                station: 1,
                label_code: "K".to_string(),
                name: "Cartucho Negro".to_string(),
                selectability: "664XL".to_string(),
                level_percent: 0,
                state: "-".to_string(),
                measured_state: "-".to_string(),
                brand: "HP".to_string(),
                unique_id: "-".to_string(),
            });

        // 1. Visual 4-Color Ink Tank Gauges (C, M, Y, K)
        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(24, 30, 42))
            .corner_radius(12.0)
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(48, 62, 84)))
            .inner_margin(egui::Margin::same(18))
            .show(ui, |ui| {
                let tanks = [
                    (
                        "CIAN",
                        "Tricolor (Izquierdo)",
                        cmy_cart.level_percent,
                        egui::Color32::from_rgb(0, 180, 235),
                        egui::Color32::from_rgb(0, 115, 165),
                        &cmy_cart.selectability,
                        &cmy_cart.measured_state,
                        &cmy_cart.state,
                    ),
                    (
                        "MAGENTA",
                        "Tricolor (Izquierdo)",
                        cmy_cart.level_percent,
                        egui::Color32::from_rgb(236, 0, 140),
                        egui::Color32::from_rgb(160, 0, 95),
                        &cmy_cart.selectability,
                        &cmy_cart.measured_state,
                        &cmy_cart.state,
                    ),
                    (
                        "AMARILLO",
                        "Tricolor (Izquierdo)",
                        cmy_cart.level_percent,
                        egui::Color32::from_rgb(250, 204, 21),
                        egui::Color32::from_rgb(185, 140, 0),
                        &cmy_cart.selectability,
                        &cmy_cart.measured_state,
                        &cmy_cart.state,
                    ),
                    (
                        "NEGRO",
                        "Negro (Derecho)",
                        k_cart.level_percent,
                        egui::Color32::from_rgb(90, 100, 115),
                        egui::Color32::from_rgb(35, 40, 50),
                        &k_cart.selectability,
                        &k_cart.measured_state,
                        &k_cart.state,
                    ),
                ];

                let avail_w = ui.available_width().max(360.0);
                let canvas_h = 245.0;
                let (rect, _resp) =
                    ui.allocate_exact_size(egui::vec2(avail_w, canvas_h), egui::Sense::hover());
                let painter = ui.painter_at(rect);

                let gap = 16.0;
                let col_w = ((avail_w - gap * 3.0) / 4.0).max(75.0);

                for (i, (title, subtitle, pct, col_bright, col_dark, model_sel, state_txt, life_state)) in
                    tanks.iter().enumerate()
                {
                    let is_refilled_unknown = state_txt.eq_ignore_ascii_case("unknown")
                        || life_state.to_ascii_lowercase().contains("refilled");
                    let x0 = rect.min.x + (i as f32) * (col_w + gap);
                    let card_rect = egui::Rect::from_min_size(
                        egui::pos2(x0, rect.min.y),
                        egui::vec2(col_w, canvas_h),
                    );

                    painter.rect_filled(
                        card_rect,
                        10.0,
                        egui::Color32::from_rgb(17, 22, 31),
                    );
                    painter.rect_stroke(
                        card_rect,
                        10.0,
                        egui::Stroke::new(1.0, col_bright.gamma_multiply(0.55)),
                        egui::StrokeKind::Inside,
                    );

                    painter.text(
                        egui::pos2(card_rect.center().x, card_rect.min.y + 18.0),
                        egui::Align2::CENTER_CENTER,
                        *title,
                        egui::FontId::proportional(14.5),
                        *col_bright,
                    );

                    let tube_w = (col_w * 0.44).clamp(38.0, 68.0);
                    let tube_h = 132.0;
                    let tube_rect = egui::Rect::from_center_size(
                        egui::pos2(card_rect.center().x, card_rect.min.y + 105.0),
                        egui::vec2(tube_w, tube_h),
                    );

                    painter.rect_filled(
                        tube_rect,
                        8.0,
                        egui::Color32::from_rgb(10, 13, 18),
                    );

                    let fill_ratio = (*pct as f32 / 100.0).clamp(0.0, 1.0);
                    if is_refilled_unknown && *pct == 0 {
                        painter.text(
                            tube_rect.center(),
                            egui::Align2::CENTER_CENTER,
                            "?",
                            egui::FontId::proportional(34.0),
                            egui::Color32::from_rgb(245, 158, 11),
                        );
                    } else if fill_ratio > 0.0 {
                        let fill_h = (tube_h - 4.0) * fill_ratio;
                        let fill_rect = egui::Rect::from_min_max(
                            egui::pos2(tube_rect.min.x + 2.0, tube_rect.max.y - 2.0 - fill_h),
                            egui::pos2(tube_rect.max.x - 2.0, tube_rect.max.y - 2.0),
                        );
                        painter.rect_filled(fill_rect, 6.0, *col_dark);

                        let hi_rect = egui::Rect::from_min_max(
                            fill_rect.min,
                            egui::pos2(
                                fill_rect.min.x + fill_rect.width() * 0.65,
                                fill_rect.max.y,
                            ),
                        );
                        painter.rect_filled(hi_rect, 6.0, *col_bright);

                        let gloss_rect = egui::Rect::from_min_max(
                            egui::pos2(tube_rect.min.x + 6.0, tube_rect.min.y + 8.0),
                            egui::pos2(tube_rect.min.x + 10.0, tube_rect.max.y - 8.0),
                        );
                        painter.rect_filled(
                            gloss_rect,
                            2.0,
                            egui::Color32::from_rgba_unmultiplied(255, 255, 255, 45),
                        );
                    }

                    for tick in [0.25f32, 0.50, 0.75] {
                        let ty = tube_rect.max.y - tube_h * tick;
                        painter.line_segment(
                            [
                                egui::pos2(tube_rect.max.x - 10.0, ty),
                                egui::pos2(tube_rect.max.x - 2.0, ty),
                            ],
                            egui::Stroke::new(
                                1.0,
                                egui::Color32::from_rgba_unmultiplied(255, 255, 255, 90),
                            ),
                        );
                    }

                    painter.rect_stroke(
                        tube_rect,
                        8.0,
                        egui::Stroke::new(1.5, egui::Color32::from_rgb(90, 108, 135)),
                        egui::StrokeKind::Inside,
                    );

                    let cap_rect = egui::Rect::from_center_size(
                        egui::pos2(card_rect.center().x, tube_rect.min.y - 4.0),
                        egui::vec2(tube_w * 0.55, 6.0),
                    );
                    painter.rect_filled(cap_rect, 2.0, *col_bright);

                    let (pct_label, pct_size, pct_color, status_badge) = if is_refilled_unknown {
                        (
                            "Recargado".to_string(),
                            14.5,
                            egui::Color32::from_rgb(251, 191, 36),
                            format!("HP {model_sel} • RECARGADO"),
                        )
                    } else {
                        (
                            format!("{pct}%"),
                            22.0,
                            egui::Color32::WHITE,
                            format!("HP {} • {}", model_sel, state_txt.to_uppercase()),
                        )
                    };

                    painter.text(
                        egui::pos2(card_rect.center().x, card_rect.max.y - 44.0),
                        egui::Align2::CENTER_CENTER,
                        pct_label,
                        egui::FontId::proportional(pct_size),
                        pct_color,
                    );

                    painter.text(
                        egui::pos2(card_rect.center().x, card_rect.max.y - 23.0),
                        egui::Align2::CENTER_CENTER,
                        *subtitle,
                        egui::FontId::proportional(11.0),
                        egui::Color32::from_rgb(160, 178, 200),
                    );
                    painter.text(
                        egui::pos2(card_rect.center().x, card_rect.max.y - 9.0),
                        egui::Align2::CENTER_CENTER,
                        status_badge,
                        egui::FontId::proportional(10.5),
                        egui::Color32::from_rgb(125, 211, 252),
                    );

                    if is_refilled_unknown {
                        ui.interact(card_rect, ui.id().with(("tank", i)), egui::Sense::hover())
                            .on_hover_text(
                                "Los cartuchos HP 664 no tienen sensor físico de líquido; su chip solo cuenta las gotas de fábrica hasta llegar a 0%.\nCuando se recarga un cartucho, la impresora lo marca como 'recargado / nivel desconocido', aunque esté lleno.",
                            );
                    }
                }
            });

        ui.add_space(14.0);

        // 2. Device Information Summary
        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(24, 30, 42))
            .corner_radius(10.0)
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(45, 58, 78)))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Estado de la Impresora")
                        .size(15.5)
                        .strong()
                        .color(egui::Color32::WHITE),
                );
                ui.add_space(10.0);

                egui::Grid::new("telemetry_grid")
                    .num_columns(2)
                    .spacing([28.0, 8.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Estado:").strong());
                        let st_text = if self.telemetry.status_categories.is_empty() {
                            "Lista".to_string()
                        } else {
                            self.telemetry.status_categories.join(", ")
                        };
                        ui.colored_label(egui::Color32::from_rgb(52, 211, 153), &st_text);
                        ui.end_row();

                        ui.label(egui::RichText::new("Alineación de cartuchos:").strong());
                        let cal_col = if self.telemetry.calibration_state.contains("Required") {
                            egui::Color32::from_rgb(251, 191, 36)
                        } else {
                            egui::Color32::from_rgb(52, 211, 153)
                        };
                        let cal_label = if self.telemetry.calibration_state == "CalibrationValid" {
                            "Alineados correctamente"
                        } else if self.telemetry.calibration_state.contains("Required") {
                            "Alineación pendiente"
                        } else {
                            &self.telemetry.calibration_state
                        };
                        ui.colored_label(cal_col, cal_label);
                        ui.end_row();

                        ui.label(egui::RichText::new("Cartucho Tricolor:").strong());
                        ui.label(format!(
                            "{}% ({} • Estado: {})",
                            cmy_cart.level_percent, cmy_cart.selectability, cmy_cart.state
                        ));
                        ui.end_row();

                        ui.label(egui::RichText::new("Cartucho Negro:").strong());
                        ui.label(format!(
                            "{}% ({} • Estado: {})",
                            k_cart.level_percent, k_cart.selectability, k_cart.state
                        ));
                        ui.end_row();

                        ui.label(egui::RichText::new("Total de páginas impresas:").strong());
                        ui.label(format!("{}", self.telemetry.total_impressions));
                        ui.end_row();

                        ui.label(egui::RichText::new("Modo monocartucho:").strong());
                        ui.label(if self.telemetry.single_cartridge_mode {
                            "Activo"
                        } else {
                            "Inactivo"
                        });
                        ui.end_row();
                    });

                if !self.telemetry.device_id_raw.is_empty() {
                    ui.add_space(8.0);
                    ui.collapsing("Detalles técnicos del dispositivo", |ui| {
                        ui.label(format!(
                            "Firmware: {} ({})  |  RAM: {} KB / {} KB  |  Tinta usada: {} µL",
                            self.telemetry.firmware_revision,
                            self.telemetry.firmware_date,
                            self.telemetry.memory_available_kb,
                            self.telemetry.memory_total_kb,
                            self.telemetry.ink_used_ul
                        ));
                        if !cmy_cart.unique_id.is_empty() || !k_cart.unique_id.is_empty() {
                            ui.label(format!(
                                "Chip Tricolor: {}  |  Chip Negro: {}",
                                cmy_cart.unique_id, k_cart.unique_id
                            ));
                        }
                        ui.label(
                            egui::RichText::new(&self.telemetry.device_id_raw)
                                .monospace()
                                .size(10.5),
                        );
                    });
                }
            });
    }

    fn render_maintenance_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Limpieza y Mantenimiento");
        ui.label("Coloca papel A4 o Carta en la bandeja antes de iniciar cualquiera de estas rutinas.");
        ui.add_space(12.0);

        if let Some(mp) = &self.maintenance_progress {
            let elapsed = format_elapsed(mp.started_at.elapsed());
            egui::Frame::NONE
                .fill(egui::Color32::from_rgb(15, 58, 92))
                .corner_radius(10.0)
                .stroke(egui::Stroke::new(1.5, egui::Color32::from_rgb(56, 189, 248)))
                .inner_margin(egui::Margin::same(14))
                .show(ui, |ui| {
                    ui.horizontal_wrapped(|ui| {
                        ui.spinner();
                        ui.label(
                            egui::RichText::new(format!(
                                "En curso: {}  •  Tiempo: {}",
                                mp.job.display_name(),
                                elapsed
                            ))
                            .size(14.5)
                            .strong()
                            .color(egui::Color32::WHITE),
                        );
                    });
                    ui.add_space(8.0);
                    ui.add(
                        egui::ProgressBar::new(mp.fraction)
                            .show_percentage()
                            .animate(true),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        egui::RichText::new(&mp.stage_text)
                            .size(12.5)
                            .color(egui::Color32::from_rgb(224, 242, 254)),
                    );
                });
            ui.add_space(12.0);
        } else if let Some((msg, is_err)) = &self.last_maintenance_result {
            let (fill, border, icon) = if *is_err {
                (
                    egui::Color32::from_rgb(80, 22, 26),
                    egui::Color32::from_rgb(248, 113, 113),
                    "⚠",
                )
            } else {
                (
                    egui::Color32::from_rgb(18, 66, 44),
                    egui::Color32::from_rgb(74, 222, 128),
                    "✅",
                )
            };
            egui::Frame::NONE
                .fill(fill)
                .corner_radius(8.0)
                .stroke(egui::Stroke::new(1.0, border))
                .inner_margin(egui::Margin::symmetric(14, 10))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(format!("{icon} {msg}"))
                            .size(13.0)
                            .strong()
                            .color(egui::Color32::WHITE),
                    );
                });
            ui.add_space(12.0);
        }

        let actions = [
            (
                MaintenanceJob::CleanLevel1,
                "🧹 Limpieza de Cabezales — Nivel 1",
                "Limpia los inyectores de ambos cartuchos e imprime una hoja de verificación. Úsalo si faltan líneas o algún color se ve tenue.",
                egui::Color32::from_rgb(2, 132, 199),
            ),
            (
                MaintenanceJob::CleanLevel2,
                "🧼 Limpieza Profunda de Cabezales — Nivel 2",
                "Ciclo de limpieza intensivo para destapar inyectores obstruidos que no mejoraron con el Nivel 1.",
                egui::Color32::from_rgb(124, 58, 237),
            ),
            (
                MaintenanceJob::PaperFeedCleaning,
                "🧻 Limpieza de Rodillos de Papel",
                "Limpia el polvo de los rodillos de alimentación cuando la impresora tiene dificultad para tomar hojas.",
                egui::Color32::from_rgb(13, 148, 136),
            ),
            (
                MaintenanceJob::RibSmearCleaning,
                "✨ Limpieza de Manchas de Tinta",
                "Elimina residuos de tinta en las guías internas si el papel sale manchado.",
                egui::Color32::from_rgb(79, 70, 229),
            ),
            (
                MaintenanceJob::UsagePage,
                "📊 Imprimir Informe de Estado",
                "Imprime una hoja con el resumen de contadores y estado de los cartuchos.",
                egui::Color32::from_rgb(71, 85, 105),
            ),
        ];

        let mut triggered_job: Option<MaintenanceJob> = None;

        for (job, title, desc, btn_color) in actions {
            let active_for_job = self
                .maintenance_progress
                .as_ref()
                .filter(|mp| mp.job == job);
            let border_color = if active_for_job.is_some() {
                egui::Color32::from_rgb(56, 189, 248)
            } else {
                egui::Color32::from_rgb(48, 62, 82)
            };

            egui::Frame::NONE
                .fill(egui::Color32::from_rgb(26, 33, 46))
                .corner_radius(10.0)
                .stroke(egui::Stroke::new(if active_for_job.is_some() { 1.6 } else { 1.0 }, border_color))
                .inner_margin(egui::Margin::same(14))
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(title)
                            .size(15.0)
                            .strong()
                            .color(egui::Color32::WHITE),
                    );
                    ui.add_space(4.0);
                    ui.label(egui::RichText::new(desc).size(12.5));
                    ui.add_space(10.0);
                    if let Some(mp) = active_for_job {
                        let elapsed = format_elapsed(mp.started_at.elapsed());
                        ui.horizontal_wrapped(|ui| {
                            ui.spinner();
                            ui.add(
                                egui::ProgressBar::new(mp.fraction)
                                    .desired_width(240.0)
                                    .show_percentage()
                                    .animate(true),
                            );
                            ui.label(
                                egui::RichText::new(format!("{elapsed} • {}", mp.stage_text))
                                    .size(12.0)
                                    .color(egui::Color32::from_rgb(186, 230, 253)),
                            );
                        });
                    } else {
                        let btn = egui::Button::new(
                            egui::RichText::new("Iniciar")
                                .color(egui::Color32::WHITE)
                                .strong(),
                        )
                        .fill(btn_color)
                        .min_size(egui::vec2(140.0, 34.0));
                        if ui.add_enabled(!self.busy, btn).clicked() {
                            triggered_job = Some(job);
                        }
                    }
                });
            ui.add_space(10.0);
        }

        if let Some(job) = triggered_job {
            self.run_maintenance(job);
        }
    }

    fn render_alignment_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Alineación de Cartuchos");
        let cal_status = if self.telemetry.calibration_state == "CalibrationValid" {
            "Alineados correctamente"
        } else if self.telemetry.calibration_state.contains("Required") {
            "Alineación pendiente"
        } else {
            &self.telemetry.calibration_state
        };
        ui.label(format!("Estado actual: {cal_status}"));
        ui.add_space(12.0);

        // Step 1
        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(26, 33, 46))
            .corner_radius(10.0)
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(50, 65, 88)))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Paso 1: Imprimir Hoja de Alineación")
                        .size(15.5)
                        .strong()
                        .color(egui::Color32::WHITE),
                );
                ui.add_space(6.0);
                ui.label("Imprime la hoja con los patrones numerados (A, B, C y D) para verificar la alineación vertical y horizontal entre el cartucho negro y el tricolor.");
                ui.add_space(10.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
                    let btn1 = egui::Button::new(
                        egui::RichText::new("🎯 Imprimir Hoja de Alineación")
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(2, 132, 199))
                    .min_size(egui::vec2(240.0, 36.0));
                    if ui.add_enabled(!self.busy, btn1).clicked() {
                        self.run_start_alignment();
                    }

                    let btn2 = egui::Button::new(
                        egui::RichText::new("📏 Imprimir Rejilla de Prueba")
                            .strong(),
                    )
                    .min_size(egui::vec2(220.0, 36.0));
                    if ui.add_enabled(!self.busy, btn2).clicked() {
                        self.run_color_test_page();
                    }
                });
            });

        ui.add_space(14.0);

        // Step 2
        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(26, 33, 46))
            .corner_radius(10.0)
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(50, 65, 88)))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Paso 2: Seleccionar Valores Alineados")
                        .size(15.5)
                        .strong()
                        .color(egui::Color32::WHITE),
                );
                ui.add_space(6.0);
                ui.label("Elige en cada grupo (A, B, C, D) el número (1 al 9) donde las líneas coincidan mejor:");
                ui.add_space(12.0);

                egui::Grid::new("align_selectors")
                    .num_columns(2)
                    .spacing([24.0, 12.0])
                    .show(ui, |ui| {
                        ui.label(egui::RichText::new("Grupo A:").strong());
                        ui.add(egui::Slider::new(&mut self.align_a, 1..=9));
                        ui.end_row();

                        ui.label(egui::RichText::new("Grupo B:").strong());
                        ui.add(egui::Slider::new(&mut self.align_b, 1..=9));
                        ui.end_row();

                        ui.label(egui::RichText::new("Grupo C:").strong());
                        ui.add(egui::Slider::new(&mut self.align_c, 1..=9));
                        ui.end_row();

                        ui.label(egui::RichText::new("Grupo D:").strong());
                        ui.add(egui::Slider::new(&mut self.align_d, 1..=9));
                        ui.end_row();
                    });

                ui.add_space(14.0);
                let save_btn = egui::Button::new(
                    egui::RichText::new("💾 Guardar Alineación")
                        .color(egui::Color32::WHITE)
                        .strong(),
                )
                .fill(egui::Color32::from_rgb(22, 163, 74))
                .min_size(egui::vec2(200.0, 36.0));
                if ui.add_enabled(!self.busy, save_btn).clicked() {
                    self.run_submit_alignment();
                }
            });
    }

    fn render_test_and_print_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Imprimir y Páginas de Prueba");
        ui.add_space(10.0);

        // Destination & Print Settings Card
        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(26, 33, 46))
            .corner_radius(10.0)
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(50, 65, 88)))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Opciones de Impresión")
                        .size(15.5)
                        .strong()
                        .color(egui::Color32::WHITE),
                );
                ui.add_space(10.0);

                ui.label(egui::RichText::new("Destino:").strong());
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(
                        &mut self.print_target,
                        PrintTargetMode::UsbDirect,
                        "USB Directo",
                    );
                    ui.radio_value(
                        &mut self.print_target,
                        PrintTargetMode::NetworkTcp9100,
                        "Router / Red TCP (Puerto 9100)",
                    );
                    ui.radio_value(
                        &mut self.print_target,
                        PrintTargetMode::CupsQueue,
                        "Cola del Sistema (Red / IPP / CUPS)",
                    );
                });

                if self.print_target == PrintTargetMode::NetworkTcp9100 {
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label("IP del Router / Servidor (:9100):");
                        ui.add(
                            egui::TextEdit::singleline(&mut self.network_addr)
                                .hint_text("192.168.1.1:9100")
                                .desired_width(220.0),
                        );
                    });
                } else if self.print_target == PrintTargetMode::CupsQueue {
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label("Cola seleccionada:");
                        egui::ComboBox::from_id_salt("cups_queue_combo")
                            .selected_text(&self.selected_cups_queue)
                            .show_ui(ui, |ui| {
                                for q in &self.cups_queues {
                                    ui.selectable_value(
                                        &mut self.selected_cups_queue,
                                        q.name.clone(),
                                        format!("{} ({})", q.name, q.device_uri),
                                    );
                                }
                            });
                    });
                }

                ui.add_space(8.0);
                ui.label(egui::RichText::new("Modo de Color:").strong());
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(
                        &mut self.print_color_mode,
                        ColorMode::ColorKCMY,
                        "Color (K + CMY)",
                    );
                    ui.radio_value(
                        &mut self.print_color_mode,
                        ColorMode::CompositeCMY,
                        "Negro compuesto (Solo Tricolor C+M+Y)",
                    );
                    ui.radio_value(
                        &mut self.print_color_mode,
                        ColorMode::GrayscaleCMY,
                        "Grises con color (Solo Tricolor C+M+Y)",
                    );
                    ui.radio_value(
                        &mut self.print_color_mode,
                        ColorMode::GrayscaleK,
                        "Escala de grises (Solo Negro K)",
                    );
                });
                if self.print_color_mode.uses_cmy_black() {
                    ui.label(
                        egui::RichText::new(
                            "Mezcla Cian + Magenta + Amarillo para formar el negro sin depender del cartucho negro. (Si retiras físicamente el cartucho negro, la impresora también activa su modo hardware de 1 cartucho).",
                        )
                        .size(11.5)
                        .color(egui::Color32::from_rgb(125, 211, 252)),
                    );
                }
                ui.add_space(6.0);

                ui.label(egui::RichText::new("Calidad:").strong());
                ui.horizontal_wrapped(|ui| {
                    ui.radio_value(
                        &mut self.print_quality,
                        PrintQuality::Draft150,
                        "Borrador (150 DPI)",
                    );
                    ui.radio_value(
                        &mut self.print_quality,
                        PrintQuality::Normal300,
                        "Normal (300 DPI)",
                    );
                    ui.radio_value(
                        &mut self.print_quality,
                        PrintQuality::High600,
                        "Alta (600 DPI)",
                    );
                });
                ui.add_space(6.0);

                ui.horizontal_wrapped(|ui| {
                    ui.label(egui::RichText::new("Papel:").strong());
                    egui::ComboBox::from_id_salt("paper_size_combo")
                        .selected_text(self.print_paper_size.display_name())
                        .show_ui(ui, |ui| {
                            for p in [
                                PaperSize::A4,
                                PaperSize::Letter,
                                PaperSize::Legal,
                                PaperSize::Photo4x6,
                                PaperSize::Photo5x7,
                                PaperSize::A5,
                            ] {
                                ui.selectable_value(
                                    &mut self.print_paper_size,
                                    p,
                                    p.display_name(),
                                );
                            }
                        });
                    ui.add_space(16.0);
                    ui.label(egui::RichText::new("Copias:").strong());
                    ui.add(egui::Slider::new(&mut self.print_copies, 1..=20));
                });
            });

        ui.add_space(14.0);

        // Test Pages Card
        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(26, 33, 46))
            .corner_radius(10.0)
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(50, 65, 88)))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Páginas de Prueba")
                        .size(15.5)
                        .strong()
                        .color(egui::Color32::WHITE),
                );
                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
                    let btn_hp = egui::Button::new(
                        egui::RichText::new("🖨️ Página de Diagnóstico Interna")
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(2, 132, 199))
                    .min_size(egui::vec2(230.0, 36.0));
                    if ui.add_enabled(!self.busy, btn_hp).clicked() {
                        self.run_maintenance(MaintenanceJob::DiagnosticsPage);
                    }

                    let btn_color = egui::Button::new(
                        egui::RichText::new("🎨 Página de Prueba de Color y Alineación")
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(217, 119, 6))
                    .min_size(egui::vec2(260.0, 36.0));
                    if ui.add_enabled(!self.busy, btn_color).clicked() {
                        self.run_color_test_page();
                    }
                });
            });

        ui.add_space(14.0);

        // Document Print Card
        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(26, 33, 46))
            .corner_radius(10.0)
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(50, 65, 88)))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Imprimir Documento o Imagen (PDF, PNG, JPEG)")
                        .size(15.5)
                        .strong()
                        .color(egui::Color32::WHITE),
                );
                ui.add_space(8.0);

                ui.label("Ruta del archivo:");
                ui.horizontal_wrapped(|ui| {
                    let text_w = (ui.available_width() - 165.0).max(200.0);
                    ui.add(
                        egui::TextEdit::singleline(&mut self.file_path_input)
                            .hint_text("/path/to/document.pdf")
                            .desired_width(text_w),
                    );
                    if ui
                        .add_enabled(
                            !self.busy,
                            egui::Button::new("📂 Seleccionar...")
                                .min_size(egui::vec2(135.0, 24.0)),
                        )
                        .clicked()
                    {
                        self.run_browse_file();
                    }
                });

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Páginas (ej. 1-16 o 1,3,5; vacío = todas):");
                    ui.add(
                        egui::TextEdit::singleline(&mut self.page_range_input)
                            .hint_text("1-16")
                            .desired_width(140.0),
                    );
                });

                ui.add_space(10.0);
                let btn_print = egui::Button::new(
                    egui::RichText::new("🖨️ Imprimir Archivo")
                        .color(egui::Color32::WHITE)
                        .strong(),
                )
                .fill(egui::Color32::from_rgb(22, 163, 74))
                .min_size(egui::vec2(180.0, 36.0));
                if ui.add_enabled(!self.busy, btn_print).clicked() {
                    self.run_print_file();
                }
            });
    }

    fn render_cups_driver_tab(&mut self, ui: &mut egui::Ui) {
        ui.heading("Conexión de Red y Colas del Sistema");
        ui.label("Administra las colas de impresión de macOS (tanto por USB como por red/router).");
        ui.add_space(12.0);

        egui::Frame::NONE
            .fill(egui::Color32::from_rgb(26, 33, 46))
            .corner_radius(10.0)
            .stroke(egui::Stroke::new(1.0, egui::Color32::from_rgb(50, 65, 88)))
            .inner_margin(egui::Margin::same(16))
            .show(ui, |ui| {
                ui.label(
                    egui::RichText::new("Colas Instaladas en el Mac")
                        .size(15.5)
                        .strong()
                        .color(egui::Color32::WHITE),
                );
                ui.add_space(8.0);

                for q in &self.cups_queues.clone() {
                    egui::Frame::NONE
                        .fill(egui::Color32::from_rgb(18, 23, 33))
                        .corner_radius(8.0)
                        .inner_margin(egui::Margin::same(12))
                        .show(ui, |ui| {
                            ui.label(
                                egui::RichText::new(&q.name)
                                    .size(14.5)
                                    .strong()
                                    .color(egui::Color32::WHITE),
                            );
                            ui.label(
                                egui::RichText::new(format!("Conexión: {}", q.device_uri))
                                    .monospace()
                                    .size(11.5),
                            );
                            let drv_col = if q.uses_custom_filter {
                                egui::Color32::from_rgb(52, 211, 153)
                            } else if q.is_usb_1115 {
                                egui::Color32::from_rgb(251, 191, 36)
                            } else {
                                egui::Color32::from_rgb(156, 163, 175)
                            };
                            ui.colored_label(
                                drv_col,
                                format!("Controlador: {}", q.current_driver_nick),
                            );
                            ui.add_space(6.0);
                            if ui.button("🗑 Vaciar trabajos en cola").clicked() {
                                match clear_cups_queue(&q.name) {
                                    Ok(m) => {
                                        self.status_banner = m.clone();
                                        self.log(m);
                                    }
                                    Err(e) => {
                                        self.is_error_banner = true;
                                        self.status_banner = e;
                                    }
                                }
                            }
                        });
                    ui.add_space(8.0);
                }

                ui.add_space(8.0);
                ui.horizontal_wrapped(|ui| {
                    ui.spacing_mut().item_spacing = egui::vec2(10.0, 8.0);
                    let install_usb_btn = egui::Button::new(
                        egui::RichText::new("⚙️ Configurar Driver USB en macOS")
                            .color(egui::Color32::WHITE)
                            .strong(),
                    )
                    .fill(egui::Color32::from_rgb(2, 132, 199))
                    .min_size(egui::vec2(260.0, 36.0));

                    if ui.add_enabled(!self.busy, install_usb_btn).clicked() {
                        self.run_install_cups();
                    }

                    let install_net_btn = egui::Button::new(
                        egui::RichText::new(format!(
                            "🌐 Configurar Cola Router ({}) en macOS",
                            self.network_addr
                        ))
                        .color(egui::Color32::WHITE)
                        .strong(),
                    )
                    .fill(egui::Color32::from_rgb(13, 148, 136))
                    .min_size(egui::vec2(300.0, 36.0));

                    if ui.add_enabled(!self.busy, install_net_btn).clicked() {
                        self.run_install_router_cups();
                    }
                });
            });
    }
}

fn allocate_status_pill(
    ui: &mut egui::Ui,
    active: bool,
    active_fill: egui::Color32,
    active_border: egui::Color32,
) -> (egui::Rect, egui::Response, egui::Color32) {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(34.0, 26.0), egui::Sense::hover());
    let (fill, mut border, fg) = if active {
        (active_fill, active_border, egui::Color32::WHITE)
    } else {
        (
            egui::Color32::from_rgb(15, 23, 42),
            egui::Color32::from_rgb(45, 58, 76),
            egui::Color32::from_rgb(82, 96, 114),
        )
    };
    if response.hovered() {
        border = if active {
            egui::Color32::WHITE
        } else {
            egui::Color32::from_rgb(120, 140, 165)
        };
    }
    let painter = ui.painter();
    painter.rect_filled(rect, 6.0, fill);
    painter.rect_stroke(
        rect,
        6.0,
        egui::Stroke::new(1.2, border),
        egui::StrokeKind::Inside,
    );
    (rect, response, fg)
}

fn draw_usb_indicator(ui: &mut egui::Ui, connected: bool, serial: &str) {
    let (rect, response, fg) = allocate_status_pill(
        ui,
        connected,
        egui::Color32::from_rgb(22, 163, 74),
        egui::Color32::from_rgb(74, 222, 128),
    );
    let c = rect.center();
    let painter = ui.painter();
    let stroke = egui::Stroke::new(1.7, fg);

    // Main vertical stem & base circle
    let base = egui::pos2(c.x, c.y + 6.5);
    let top = egui::pos2(c.x, c.y - 6.0);
    painter.circle_filled(base, 2.2, fg);
    painter.line_segment([base, top], stroke);

    // Top arrowhead
    painter.add(egui::Shape::convex_polygon(
        vec![
            egui::pos2(c.x, c.y - 8.8),
            egui::pos2(c.x - 2.6, c.y - 5.2),
            egui::pos2(c.x + 2.6, c.y - 5.2),
        ],
        fg,
        egui::Stroke::NONE,
    ));

    // Left branch (circle node)
    let left_fork = egui::pos2(c.x, c.y + 2.5);
    let left_elbow = egui::pos2(c.x - 5.2, c.y - 1.2);
    let left_tip = egui::pos2(c.x - 5.2, c.y - 3.6);
    painter.line_segment([left_fork, left_elbow], stroke);
    painter.line_segment([left_elbow, left_tip], stroke);
    painter.circle_filled(left_tip, 1.8, fg);

    // Right branch (square node)
    let right_fork = egui::pos2(c.x, c.y + 0.5);
    let right_elbow = egui::pos2(c.x + 5.2, c.y - 3.0);
    let right_tip = egui::pos2(c.x + 5.2, c.y - 5.2);
    painter.line_segment([right_fork, right_elbow], stroke);
    painter.line_segment([right_elbow, right_tip], stroke);
    painter.rect_filled(
        egui::Rect::from_center_size(right_tip, egui::vec2(3.4, 3.4)),
        0.5,
        fg,
    );

    let tooltip = if connected {
        format!(
            "USB conectado (Serie: {serial})\nControl total disponible: niveles de tinta, limpieza de cabezales, alineación e impresión directa."
        )
    } else {
        "USB desconectado\nConecta el cable USB al Mac para leer niveles de tinta, limpiar cabezales y alinear cartuchos.".to_string()
    };
    response.on_hover_text(tooltip);
}

fn draw_wifi_indicator(ui: &mut egui::Ui, router_addr: Option<&str>) {
    let connected = router_addr.is_some();
    let (rect, response, fg) = allocate_status_pill(
        ui,
        connected,
        egui::Color32::from_rgb(2, 132, 199),
        egui::Color32::from_rgb(56, 189, 248),
    );
    let c = rect.center();
    let painter = ui.painter();
    let origin = egui::pos2(c.x, c.y + 6.2);
    painter.circle_filled(origin, 1.9, fg);

    let start = -3.0 * std::f32::consts::FRAC_PI_4;
    let end = -std::f32::consts::FRAC_PI_4;
    for radius in [4.8_f32, 8.0_f32, 11.2_f32] {
        let steps = 14;
        let pts: Vec<egui::Pos2> = (0..=steps)
            .map(|i| {
                let t = i as f32 / steps as f32;
                let angle = start + (end - start) * t;
                egui::pos2(
                    origin.x + radius * angle.cos(),
                    origin.y + radius * angle.sin(),
                )
            })
            .collect();
        painter.add(egui::Shape::line(pts, egui::Stroke::new(1.7, fg)));
    }

    let tooltip = if let Some(addr) = router_addr {
        format!(
            "Router / Red conectado ({addr})\nPuede imprimir documentos, imágenes y la página de prueba de color directamente por red. Los routers solo comparten el canal de impresión (puerto 9100), por lo que la lectura de niveles de tinta y la limpieza de cabezales solo están disponibles conectando el cable USB al Mac."
        )
    } else {
        "Router / Red no detectado\nNo se encontró servidor de impresión activo en el puerto 9100 del router.".to_string()
    };
    response.on_hover_text(tooltip);
}

fn estimated_maintenance_secs(job: MaintenanceJob) -> f32 {
    match job {
        MaintenanceJob::CleanLevel1 => 35.0,
        MaintenanceJob::CleanLevel2 => 55.0,
        MaintenanceJob::PaperFeedCleaning => 25.0,
        MaintenanceJob::RibSmearCleaning => 30.0,
        MaintenanceJob::DiagnosticsPage | MaintenanceJob::UsagePage => 15.0,
    }
}

fn format_elapsed(d: std::time::Duration) -> String {
    let secs = d.as_secs();
    format!("{:02}:{:02}", secs / 60, secs % 60)
}
