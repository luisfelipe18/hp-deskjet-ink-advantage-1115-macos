use std::io::Write;
use std::net::{TcpStream, ToSocketAddrs};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct CupsQueueInfo {
    pub name: String,
    pub device_uri: String,
    pub is_usb_1115: bool,
    pub current_driver_nick: String,
    pub uses_custom_filter: bool,
}

pub fn generate_ppd_content(filter_path: &str) -> String {
    format!(
        r#"*PPD-Adobe: "4.3"
*%%%% PPD file for HP DeskJet Ink Advantage 1115 (1110 series)
*FormatVersion: "4.3"
*FileVersion: "1.0"
*LanguageVersion: Spanish
*LanguageEncoding: ISOLatin1
*PCFileName: "HP1115.PPD"
*Manufacturer: "HP"
*Product: "(DeskJet 1110 series)"
*ModelName: "HP DeskJet Ink Advantage 1115"
*ShortNickName: "HP DeskJet 1115"
*NickName: "HP DeskJet Ink Advantage 1115"
*PSVersion: "(3010.000) 0"
*LanguageLevel: "3"
*ColorDevice: True
*DefaultColorSpace: RGB
*FileSystem: False
*Throughput: "7"
*LandscapeOrientation: Plus90
*TTRasterizer: Type42
*1284DeviceID: "MFG:HP;MDL:DeskJet 1110 series;CMD:PCL3GUI,PJL,Automatic,DW-PCL,DESKJET,DYN;CLS:PRINTER;DES:F5S21A;"
*cupsVersion: 2.3
*cupsModelNumber: 1115
*cupsManualCopies: False
*cupsFilter: "application/vnd.cups-raster 50 {filter_path}"

*OpenUI *PageSize/Tamaño de Papel: PickOne
*OrderDependency: 10 AnySetup *PageSize
*DefaultPageSize: A4
*PageSize A4/A4 (210 x 297 mm): "<</PageSize[595 842]/ImagingBBox null/cupsInteger0 26>>setpagedevice"
*PageSize Letter/Carta US (8.5 x 11 in): "<</PageSize[612 792]/ImagingBBox null/cupsInteger0 2>>setpagedevice"
*PageSize Legal/Oficio US (8.5 x 14 in): "<</PageSize[612 1008]/ImagingBBox null/cupsInteger0 3>>setpagedevice"
*PageSize A5/A5 (148 x 210 mm): "<</PageSize[420 595]/ImagingBBox null/cupsInteger0 25>>setpagedevice"
*PageSize 4x6/Foto 4x6 in (10 x 15 cm): "<</PageSize[288 432]/ImagingBBox null/cupsInteger0 74>>setpagedevice"
*PageSize 5x7/Foto 5x7 in (13 x 18 cm): "<</PageSize[360 504]/ImagingBBox null/cupsInteger0 75>>setpagedevice"
*CloseUI: *PageSize

*OpenUI *PageRegion/Tamaño de Papel: PickOne
*OrderDependency: 10 AnySetup *PageRegion
*DefaultPageRegion: A4
*PageRegion A4/A4 (210 x 297 mm): "<</PageSize[595 842]/ImagingBBox null/cupsInteger0 26>>setpagedevice"
*PageRegion Letter/Carta US (8.5 x 11 in): "<</PageSize[612 792]/ImagingBBox null/cupsInteger0 2>>setpagedevice"
*PageRegion Legal/Oficio US (8.5 x 14 in): "<</PageSize[612 1008]/ImagingBBox null/cupsInteger0 3>>setpagedevice"
*PageRegion A5/A5 (148 x 210 mm): "<</PageSize[420 595]/ImagingBBox null/cupsInteger0 25>>setpagedevice"
*PageRegion 4x6/Foto 4x6 in (10 x 15 cm): "<</PageSize[288 432]/ImagingBBox null/cupsInteger0 74>>setpagedevice"
*PageRegion 5x7/Foto 5x7 in (13 x 18 cm): "<</PageSize[360 504]/ImagingBBox null/cupsInteger0 75>>setpagedevice"
*CloseUI: *PageRegion

*DefaultImageableArea: A4
*ImageableArea A4/A4: "9 36 586 833"
*ImageableArea Letter/Carta US: "9 36 603 783"
*ImageableArea Legal/Oficio US: "9 36 603 999"
*ImageableArea A5/A5: "9 36 411 586"
*ImageableArea 4x6/Foto 4x6: "9 36 279 423"
*ImageableArea 5x7/Foto 5x7: "9 36 351 495"

*DefaultPaperDimension: A4
*PaperDimension A4/A4: "595 842"
*PaperDimension Letter/Carta US: "612 792"
*PaperDimension Legal/Oficio US: "612 1008"
*PaperDimension A5/A5: "420 595"
*PaperDimension 4x6/Foto 4x6: "288 432"
*PaperDimension 5x7/Foto 5x7: "360 504"

*OpenUI *ColorModel/Modo de Color: PickOne
*OrderDependency: 10 AnySetup *ColorModel
*DefaultColorModel: RGB
*ColorModel RGB/Color: "<</cupsColorSpace 1/cupsBitsPerColor 8/cupsColorOrder 0/cupsCompression 0>>setpagedevice"
*ColorModel Gray/Escala de Grises: "<</cupsColorSpace 3/cupsBitsPerColor 8/cupsColorOrder 0/cupsCompression 0>>setpagedevice"
*CloseUI: *ColorModel

*OpenUI *Resolution/Calidad: PickOne
*OrderDependency: 10 AnySetup *Resolution
*DefaultResolution: 300dpi
*Resolution 150dpi/Borrador (150 DPI): "<</HWResolution[150 150]>>setpagedevice"
*Resolution 300dpi/Normal (300 DPI): "<</HWResolution[300 300]>>setpagedevice"
*Resolution 600dpi/Alta (600 DPI): "<</HWResolution[600 600]>>setpagedevice"
*CloseUI: *Resolution
"#
    )
}

pub fn ensure_ppd_file_in_workspace(workspace_dir: &Path) -> Result<PathBuf, String> {
    let preferred_dir = workspace_dir.join("ppd");
    let ppd_dir = if std::fs::create_dir_all(&preferred_dir).is_ok() {
        preferred_dir
    } else {
        let fallback = std::env::temp_dir().join("hp_deskjet_1115_ppd");
        std::fs::create_dir_all(&fallback)
            .map_err(|e| format!("No se pudo crear directorio ppd/: {e}"))?;
        fallback
    };
    let ppd_path = ppd_dir.join("HP_DeskJet_1115.ppd");
    let content = generate_ppd_content("/usr/libexec/cups/filter/rastertohp1115");
    std::fs::write(&ppd_path, content)
        .map_err(|e| format!("No se pudo escribir el archivo PPD: {e}"))?;
    Ok(ppd_path)
}

pub fn list_cups_queues() -> Vec<CupsQueueInfo> {
    let mut queues = Vec::new();
    let Ok(out) = Command::new("lpstat").arg("-v").output() else {
        return queues;
    };
    let text = String::from_utf8_lossy(&out.stdout);
    for line in text.lines() {
        if let Some(rest) = line
            .strip_prefix("device for ")
            .or_else(|| line.strip_prefix("dispositivo para "))
        {
            if let Some((qname, uri)) = rest.split_once(": ") {
                let name = qname.trim().to_string();
                let device_uri = uri.trim().to_string();
                let is_usb_1115 =
                    device_uri.starts_with("usb://HP/") && device_uri.contains("1110");

                let ppd_file = PathBuf::from(format!("/etc/cups/ppd/{name}.ppd"));
                let mut current_driver_nick = "Desconocido".to_string();
                let mut uses_custom_filter = false;
                if let Ok(ppd_txt) = std::fs::read_to_string(&ppd_file) {
                    for pline in ppd_txt.lines() {
                        if let Some(nick) = pline.strip_prefix("*NickName:") {
                            current_driver_nick = nick.trim().trim_matches('"').to_string();
                        }
                        if pline.contains("rastertohp1115") {
                            uses_custom_filter = true;
                        }
                    }
                }

                queues.push(CupsQueueInfo {
                    name,
                    device_uri,
                    is_usb_1115,
                    current_driver_nick,
                    uses_custom_filter,
                });
            }
        }
    }
    queues
}

fn find_compiled_cups_filter(workspace_dir: &Path) -> Result<PathBuf, String> {
    let exe_sibling = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(|d| d.join("rastertohp1115")));
    let candidates = [
        exe_sibling,
        Some(workspace_dir.join("target/release/rastertohp1115")),
        Some(workspace_dir.join("target/debug/rastertohp1115")),
        Some(PathBuf::from("/usr/libexec/cups/filter/rastertohp1115")),
    ];
    for candidate in candidates.into_iter().flatten() {
        if candidate.exists() {
            return Ok(candidate);
        }
    }
    Err("No se encontró el filtro compilado 'rastertohp1115'.".to_string())
}

fn install_cups_queue_macos(
    workspace_dir: &Path,
    queue_name: &str,
    device_uri: &str,
    description: &str,
    success_msg: String,
) -> Result<String, String> {
    let ppd_path = ensure_ppd_file_in_workspace(workspace_dir)?;
    let filter_src = find_compiled_cups_filter(workspace_dir)?;

    let install_cmd = format!(
        "cp '{}' /usr/libexec/cups/filter/rastertohp1115 && chown root:wheel /usr/libexec/cups/filter/rastertohp1115 && chmod 755 /usr/libexec/cups/filter/rastertohp1115 && lpadmin -p {} -E -v '{}' -P '{}' -D '{}'",
        filter_src.display(),
        queue_name,
        device_uri,
        ppd_path.display(),
        description
    );

    let osa_script = format!(
        "do shell script \"{}\" with administrator privileges",
        install_cmd.replace('"', "\\\"")
    );

    let out = Command::new("osascript")
        .arg("-e")
        .arg(&osa_script)
        .output()
        .map_err(|e| format!("No se pudo lanzar osascript: {e}"))?;

    if out.status.success() {
        Ok(success_msg)
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr);
        Err(format!(
            "No se completó la instalación ({}). Comando manual:\nsudo {}",
            stderr.trim(),
            install_cmd
        ))
    }
}

pub fn install_cups_driver_macos(workspace_dir: &Path, serial: &str) -> Result<String, String> {
    let uri = format!("usb://HP/DeskJet%201110%20series?serial={serial}");
    install_cups_queue_macos(
        workspace_dir,
        "HP_DeskJet_1110_series",
        &uri,
        "HP DeskJet Ink Advantage 1115",
        "Driver CUPS instalado para la cola HP_DeskJet_1110_series.".to_string(),
    )
}

pub fn install_router_cups_driver_macos(
    workspace_dir: &Path,
    addr_str: &str,
) -> Result<String, String> {
    let target = normalize_tcp_target(addr_str);
    let uri = format!("socket://{target}");
    install_cups_queue_macos(
        workspace_dir,
        "HP_DeskJet_1115_Router",
        &uri,
        "HP DeskJet Ink Advantage 1115 (Router)",
        format!("Cola CUPS 'HP_DeskJet_1115_Router' configurada en macOS hacia {uri}."),
    )
}

pub fn clear_cups_queue(queue_name: &str) -> Result<String, String> {
    let out = Command::new("cancel")
        .args(["-a", queue_name])
        .output()
        .map_err(|e| format!("Error ejecutando cancel: {e}"))?;
    if out.status.success() {
        Ok(format!(
            "Cola de impresión '{queue_name}' vaciada correctamente."
        ))
    } else {
        Err(String::from_utf8_lossy(&out.stderr).trim().to_string())
    }
}

pub fn normalize_tcp_target(addr_str: &str) -> String {
    let trimmed = addr_str.trim();
    if trimmed.contains(':') {
        trimmed.to_string()
    } else {
        format!("{trimmed}:9100")
    }
}

pub fn probe_tcp_print_server(addr_str: &str) -> bool {
    let target = normalize_tcp_target(addr_str);
    let Ok(mut addrs) = target.to_socket_addrs() else {
        return false;
    };
    let Some(sock_addr) = addrs.next() else {
        return false;
    };
    TcpStream::connect_timeout(&sock_addr, Duration::from_millis(700)).is_ok()
}

pub fn detect_router_print_server(preferred_addr: &str) -> Option<String> {
    let mut candidates = Vec::new();
    let pref = normalize_tcp_target(preferred_addr);
    if !pref.is_empty() {
        candidates.push(pref);
    }

    if let Ok(out) = Command::new("route").args(["-n", "get", "default"]).output() {
        let txt = String::from_utf8_lossy(&out.stdout);
        for line in txt.lines() {
            if let Some(gw) = line.trim().strip_prefix("gateway:") {
                let gw_target = format!("{}:9100", gw.trim());
                if !candidates.contains(&gw_target) {
                    candidates.push(gw_target);
                }
            }
        }
    }

    let fallback = "192.168.1.1:9100".to_string();
    if !candidates.contains(&fallback) {
        candidates.push(fallback);
    }

    candidates
        .into_iter()
        .find(|cand| probe_tcp_print_server(cand))
}

pub struct TcpPrintSession {
    stream: TcpStream,
    target: String,
}

impl TcpPrintSession {
    pub fn connect(addr_str: &str) -> Result<Self, String> {
        let target = normalize_tcp_target(addr_str);
        let sock_addr = target
            .to_socket_addrs()
            .map_err(|e| format!("Dirección de red inválida '{target}': {e}"))?
            .next()
            .ok_or_else(|| format!("No se pudo resolver '{target}'"))?;

        let stream = TcpStream::connect_timeout(&sock_addr, Duration::from_secs(5)).map_err(
            |e| format!("No se pudo conectar al servidor de impresión '{target}': {e}"),
        )?;
        let _ = stream.set_write_timeout(Some(Duration::from_secs(120)));
        Ok(Self { stream, target })
    }

    pub fn write_all(&mut self, data: &[u8]) -> Result<usize, String> {
        self.stream
            .write_all(data)
            .map_err(|e| format!("Error enviando datos a '{}': {e}", self.target))?;
        let _ = self.stream.flush();
        Ok(data.len())
    }
}

/// Sends raw PCL print data over TCP (e.g. Router USB print server on port 9100)
pub fn send_raw_pcl_over_tcp(addr_str: &str, data: &[u8]) -> Result<usize, String> {
    let mut sess = TcpPrintSession::connect(addr_str)?;
    sess.write_all(data)
}

/// Sends a file or image through an existing CUPS queue (e.g., IPP network queue on router/Raspberry Pi)
pub fn print_file_via_cups_queue(
    queue_name: &str,
    file_path: &Path,
    page_range: Option<&str>,
) -> Result<String, String> {
    let mut cmd = Command::new("lp");
    cmd.args(["-d", queue_name]);
    if let Some(range) = page_range {
        let trimmed = range.trim();
        if !trimmed.is_empty()
            && !trimmed.eq_ignore_ascii_case("all")
            && !trimmed.eq_ignore_ascii_case("todas")
        {
            cmd.args(["-P", trimmed]);
        }
    }
    cmd.arg(file_path.to_string_lossy().as_ref());

    let out = cmd
        .output()
        .map_err(|e| format!("No se pudo ejecutar 'lp': {e}"))?;
    if out.status.success() {
        let stdout = String::from_utf8_lossy(&out.stdout).trim().to_string();
        Ok(format!("Trabajo enviado a la cola '{queue_name}' ({stdout})."))
    } else {
        let stderr = String::from_utf8_lossy(&out.stderr).trim().to_string();
        Err(format!("Error en cola '{queue_name}': {stderr}"))
    }
}
