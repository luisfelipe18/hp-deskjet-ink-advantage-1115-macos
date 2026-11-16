#[path = "../pcl.rs"]
mod pcl;

use pcl::{ColorMode, PaperSize, PclJobConfig, PrintQuality, convert_cups_raster_to_pcl};
use std::fs::File;
use std::io::{self, BufReader, BufWriter};

fn main() {
    // CUPS Filter invocation convention:
    // argv[1]: job-id
    // argv[2]: user
    // argv[3]: title
    // argv[4]: copies
    // argv[5]: options
    // argv[6]: [optional raster file path; if omitted, read from stdin]
    let args: Vec<String> = std::env::args().collect();

    let copies: u16 = args
        .get(4)
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(1)
        .max(1);
    let options = args.get(5).map(String::as_str).unwrap_or("");

    let mut cfg = PclJobConfig {
        copies,
        use_pcl3gui_header: true,
        ..Default::default()
    };

    let lower_opts = options.to_ascii_lowercase();
    if lower_opts.contains("colormodel=gray") || lower_opts.contains("grayscale") {
        cfg.color_mode = ColorMode::GrayscaleK;
    } else {
        cfg.color_mode = ColorMode::ColorKCMY;
    }

    if lower_opts.contains("150dpi") || lower_opts.contains("draft") {
        cfg.quality = PrintQuality::Draft150;
    } else if lower_opts.contains("600dpi") || lower_opts.contains("high") {
        cfg.quality = PrintQuality::High600;
    } else {
        cfg.quality = PrintQuality::Normal300;
    }

    if lower_opts.contains("pagesize=letter") || lower_opts.contains("media=letter") {
        cfg.paper_size = PaperSize::Letter;
    } else if lower_opts.contains("pagesize=legal") || lower_opts.contains("media=legal") {
        cfg.paper_size = PaperSize::Legal;
    } else if lower_opts.contains("pagesize=4x6") {
        cfg.paper_size = PaperSize::Photo4x6;
    } else if lower_opts.contains("pagesize=5x7") {
        cfg.paper_size = PaperSize::Photo5x7;
    } else if lower_opts.contains("pagesize=a5") {
        cfg.paper_size = PaperSize::A5;
    } else {
        cfg.paper_size = PaperSize::A4;
    }

    eprintln!(
        "INFO: [rastertohp1115] Iniciando filtro Rust para HP DeskJet 1115 ({:?}, {} DPI, {:?})",
        cfg.color_mode,
        cfg.quality.dpi(),
        cfg.paper_size
    );

    let stdout = io::stdout();
    let writer = BufWriter::new(stdout.lock());

    let res = if let Some(file_path) = args.get(6) {
        match File::open(file_path) {
            Ok(f) => convert_cups_raster_to_pcl(BufReader::new(f), writer, &cfg),
            Err(e) => Err(format!("No se pudo abrir archivo de entrada '{file_path}': {e}")),
        }
    } else {
        let stdin = io::stdin();
        convert_cups_raster_to_pcl(BufReader::new(stdin.lock()), writer, &cfg)
    };

    match res {
        Ok(pages) => {
            eprintln!("INFO: [rastertohp1115] Trabajo completado ({pages} páginas procesadas).");
        }
        Err(e) => {
            eprintln!("ERROR: [rastertohp1115] {e}");
            std::process::exit(1);
        }
    }
}
