use crate::usb::HpUsbConnection;
use roxmltree::Document;

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct CartridgeStatus {
    pub station: u8,
    pub label_code: String, // "CMY" or "K"
    pub name: String,       // "Tricolor (HP 664)" or "Negro (HP 664)"
    pub selectability: String, // "XL" or "Standard"
    pub level_percent: u8,
    pub state: String,      // "used", "new", "ok"
    pub measured_state: String, // "ok", "low", "empty"
    pub brand: String,
    pub unique_id: String,
}

#[allow(dead_code)]
#[derive(Debug, Clone)]
pub struct PrinterTelemetry {
    pub connected: bool,
    pub make_and_model: String,
    pub product_number: String,
    pub serial_number: String,
    pub firmware_revision: String,
    pub firmware_date: String,
    pub usb_id: String,
    pub device_id_raw: String,
    pub status_categories: Vec<String>,
    pub alerts: Vec<String>,
    pub calibration_state: String,
    pub single_cartridge_mode: bool,
    pub alignment_mode: String,
    pub memory_available_kb: u32,
    pub memory_total_kb: u32,
    pub total_impressions: u32,
    pub jam_events: u32,
    pub mispick_events: u32,
    pub cancel_presses: u32,
    pub ink_used_ul: f32,
    pub cartridges: Vec<CartridgeStatus>,
    pub supported_internal_jobs: Vec<String>,
}

impl Default for PrinterTelemetry {
    fn default() -> Self {
        Self {
            connected: false,
            make_and_model: "HP DeskJet Ink Advantage 1115 (1110 series)".to_string(),
            product_number: "F5S21A".to_string(),
            serial_number: "-".to_string(),
            firmware_revision: "-".to_string(),
            firmware_date: "-".to_string(),
            usb_id: "0x03f0:0xdf11".to_string(),
            device_id_raw: String::new(),
            status_categories: Vec::new(),
            alerts: Vec::new(),
            calibration_state: "Desconocido".to_string(),
            single_cartridge_mode: false,
            alignment_mode: "manual".to_string(),
            memory_available_kb: 0,
            memory_total_kb: 0,
            total_impressions: 0,
            jam_events: 0,
            mispick_events: 0,
            cancel_presses: 0,
            ink_used_ul: 0.0,
            cartridges: Vec::new(),
            supported_internal_jobs: vec![
                "cleaningPage".to_string(),
                "cleaningPageLevel2".to_string(),
                "diagnosticsPage".to_string(),
                "paperFeedCleaningPage".to_string(),
                "ribSmearCleaningPage".to_string(),
                "usagePage".to_string(),
            ],
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MaintenanceJob {
    CleanLevel1,
    CleanLevel2,
    DiagnosticsPage,
    PaperFeedCleaning,
    RibSmearCleaning,
    UsagePage,
}

impl MaintenanceJob {
    pub fn xml_job_type(self) -> &'static str {
        match self {
            Self::CleanLevel1 => "cleaningPage",
            Self::CleanLevel2 => "cleaningPageLevel2",
            Self::DiagnosticsPage => "diagnosticsPage",
            Self::PaperFeedCleaning => "paperFeedCleaningPage",
            Self::RibSmearCleaning => "ribSmearCleaningPage",
            Self::UsagePage => "usagePage",
        }
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Self::CleanLevel1 => "Limpieza de Cabezales (Nivel 1 - Estándar)",
            Self::CleanLevel2 => "Limpieza Profunda de Cabezales (Nivel 2)",
            Self::DiagnosticsPage => "Página de Prueba y Diagnóstico de Calidad HP",
            Self::PaperFeedCleaning => "Limpieza de Rodillos de Alimentación de Papel",
            Self::RibSmearCleaning => "Limpieza de Manchas de Tinta (Rib Smear)",
            Self::UsagePage => "Página de Informe de Uso y Contadores",
        }
    }
}

pub fn query_printer_telemetry(conn: &HpUsbConnection) -> PrinterTelemetry {
    let mut t = PrinterTelemetry {
        connected: true,
        make_and_model: format!("{} {}", conn.info.manufacturer, conn.info.product),
        serial_number: conn.info.serial_number.clone(),
        usb_id: format!("0x{:04x}:0x{:04x}", conn.info.vendor_id, conn.info.product_id),
        device_id_raw: conn.info.device_id_string.clone(),
        ..Default::default()
    };

    // 1. Parse fallback values from IEEE 1284 Device ID string first
    parse_ieee1284_fallback(&conn.info.device_id_string, &mut t);

    // 2. Query /DevMgmt/ConsumableConfigDyn.xml
    if let Ok((200, xml)) = conn.ledm_request_expect(
        "GET",
        "/DevMgmt/ConsumableConfigDyn.xml",
        None,
        Some("ConsumableConfigDyn"),
    ) {
        parse_consumable_config_xml(&xml, &mut t);
    }

    // 3. Query /DevMgmt/ProductStatusDyn.xml
    if let Ok((200, xml)) = conn.ledm_request_expect(
        "GET",
        "/DevMgmt/ProductStatusDyn.xml",
        None,
        Some("ProductStatusDyn"),
    ) {
        parse_product_status_xml(&xml, &mut t);
    }

    // 4. Query /DevMgmt/ProductConfigDyn.xml
    if let Ok((200, xml)) = conn.ledm_request_expect(
        "GET",
        "/DevMgmt/ProductConfigDyn.xml",
        None,
        Some("ProductConfigDyn"),
    ) {
        parse_product_config_xml(&xml, &mut t);
    }

    // 5. Query /DevMgmt/ProductUsageDyn.xml
    if let Ok((200, xml)) = conn.ledm_request_expect(
        "GET",
        "/DevMgmt/ProductUsageDyn.xml",
        None,
        Some("ProductUsageDyn"),
    ) {
        parse_product_usage_xml(&xml, &mut t);
    }

    // 6. Query /Calibration/State
    if let Ok((200, xml)) =
        conn.ledm_request_expect("GET", "/Calibration/State", None, Some("CalibrationState"))
    {
        if let Ok(doc) = Document::parse(&xml) {
            if let Some(txt) = doc.root_element().text() {
                t.calibration_state = txt.trim().to_string();
            }
        }
    }

    t
}

pub fn trigger_maintenance_job(
    conn: &HpUsbConnection,
    job: MaintenanceJob,
) -> Result<String, String> {
    let job_type = job.xml_job_type();
    let payload = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<ipdyn:InternalPrintDyn xmlns:ipdyn="http://www.hp.com/schemas/imaging/con/ledm/internalprintdyn/2008/03/21" xmlns:dd="http://www.hp.com/schemas/imaging/con/dictionaries/1.0/">
  <ipdyn:JobType>{job_type}</ipdyn:JobType>
</ipdyn:InternalPrintDyn>"#
    );

    // The firmware answers 503 while it is waking from power-save or still busy with
    // a previous job, so retry a few times before reporting an error.
    let mut last_code = 0u16;
    for attempt in 0..6 {
        if attempt > 0 {
            std::thread::sleep(std::time::Duration::from_secs(3));
        }
        match conn.ledm_request("POST", "/DevMgmt/InternalPrintDyn.xml", Some(&payload)) {
            Ok((code, _)) if (200..300).contains(&code) => {
                return Ok(format!("{}: enviado a la impresora.", job.display_name()));
            }
            Ok((503, _)) => {
                last_code = 503;
                continue;
            }
            Ok((code, _)) => {
                last_code = code;
                break;
            }
            Err(e) => return Err(e),
        }
    }

    if last_code == 503 {
        let st = query_printer_telemetry(conn);
        let problems: Vec<&String> = st
            .alerts
            .iter()
            .filter(|a| !a.eq_ignore_ascii_case("inPowerSave"))
            .collect();
        if problems.is_empty() {
            return Err(
                "La impresora está ocupada (probablemente terminando otro trabajo). Espera unos segundos y vuelve a intentarlo."
                    .to_string(),
            );
        }
        return Err(format!(
            "La impresora no puede ejecutar la tarea por un problema activo: {problems:?}"
        ));
    }
    Err(format!(
        "La impresora rechazó '{}' (código {last_code}).",
        job.display_name()
    ))
}

fn normalize_session_location(loc: &str) -> Option<String> {
    let trimmed = loc.trim();
    if trimmed.is_empty() {
        return None;
    }
    if let Some(pos) = trimmed.find("/Calibration/Session/") {
        return Some(trimmed[pos..].trim_end_matches('/').to_string());
    }
    if trimmed.starts_with('/') {
        return Some(trimmed.trim_end_matches('/').to_string());
    }
    None
}

fn find_active_calibration_session(conn: &HpUsbConnection) -> Option<String> {
    let mut active: Option<String> = None;
    for id in 1..=10 {
        let uri = format!("/Calibration/Session/{id}");
        if let Ok((200, _)) = conn.ledm_request("GET", &uri, None) {
            active = Some(uri);
        }
    }
    active
}

/// Starts cartridge alignment: tries LEDM `/Calibration/Session` and falls back to `diagnosticsPage`.
pub fn start_calibration_session(conn: &HpUsbConnection) -> Result<String, String> {
    let ns = "http://www.hp.com/schemas/imaging/con/cnx/markingagentcalibration/2009/04/08";
    let candidates = [
        format!(r#"<?xml version="1.0" encoding="UTF-8"?><CalibrationState xmlns="{ns}">CalibrationInProgress</CalibrationState>"#),
        format!(r#"<?xml version="1.0" encoding="UTF-8"?><CalibrationState xmlns="{ns}">WaitingForUserInput</CalibrationState>"#),
    ];

    for payload in &candidates {
        if let Ok(resp) = conn.ledm_request_full("POST", "/Calibration/Session", Some(payload), None) {
            if (200..300).contains(&resp.status_code) {
                let loc = resp
                    .header("location")
                    .and_then(normalize_session_location)
                    .or_else(|| find_active_calibration_session(conn))
                    .unwrap_or_else(|| "/Calibration/Session/1".to_string());
                return Ok(format!(
                    "Sesión de alineación iniciada ({loc}, HTTP {}). Espera a que termine de imprimir y luego guarda la alineación.",
                    resp.status_code
                ));
            }
        }
    }

    let _ = trigger_maintenance_job(conn, MaintenanceJob::DiagnosticsPage)?;
    Ok(
        "Hoja de diagnóstico y alineación enviada a imprimir. Revisa las líneas impresas y presiona '💾 Guardar Alineación' en el Paso 2."
            .to_string(),
    )
}

/// Submits manual alignment values (`ManualSelectedPatterns.xml` if a session is open,
/// otherwise confirms `CalibrationValid` directly via `POST /Calibration/Session`).
pub fn submit_manual_alignment_values(
    conn: &HpUsbConnection,
    pattern_a: u32,
    pattern_b: u32,
    pattern_c: u32,
    pattern_d: u32,
) -> Result<String, String> {
    if let Some(session_uri) = find_active_calibration_session(conn) {
        let target_uri = format!("{session_uri}/ManualSelectedPatterns.xml");
        let payloads = [
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<ManualSelectedPatterns xmlns="http://www.hp.com/schemas/imaging/con/cnx/markingagentcalibration/2009/04/08">
  <PatternGroup>
    <PatternGroupNumber>1</PatternGroupNumber>
    <PatternNumber>{pattern_a}</PatternNumber>
  </PatternGroup>
  <PatternGroup>
    <PatternGroupNumber>2</PatternGroupNumber>
    <PatternNumber>{pattern_b}</PatternNumber>
  </PatternGroup>
  <PatternGroup>
    <PatternGroupNumber>3</PatternGroupNumber>
    <PatternNumber>{pattern_c}</PatternNumber>
  </PatternGroup>
  <PatternGroup>
    <PatternGroupNumber>4</PatternGroupNumber>
    <PatternNumber>{pattern_d}</PatternNumber>
  </PatternGroup>
</ManualSelectedPatterns>"#
            ),
            format!(
                r#"<?xml version="1.0" encoding="UTF-8"?>
<cal:ManualSelectedPatterns xmlns:cal="http://www.hp.com/schemas/imaging/con/cnx/markingagentcalibration/2009/04/08">
  <cal:PatternGroup>
    <cal:PatternGroupNumber>1</cal:PatternGroupNumber>
    <cal:PatternNumber>{pattern_a}</cal:PatternNumber>
  </cal:PatternGroup>
  <cal:PatternGroup>
    <cal:PatternGroupNumber>2</cal:PatternGroupNumber>
    <cal:PatternNumber>{pattern_b}</cal:PatternNumber>
  </cal:PatternGroup>
  <cal:PatternGroup>
    <cal:PatternGroupNumber>3</cal:PatternGroupNumber>
    <cal:PatternNumber>{pattern_c}</cal:PatternNumber>
  </cal:PatternGroup>
  <cal:PatternGroup>
    <cal:PatternGroupNumber>4</cal:PatternGroupNumber>
    <cal:PatternNumber>{pattern_d}</cal:PatternNumber>
  </cal:PatternGroup>
</cal:ManualSelectedPatterns>"#
            ),
        ];

        let mut last_code = 0u16;
        let mut last_body = String::new();
        for payload in &payloads {
            match conn.ledm_request("PUT", &target_uri, Some(payload)) {
                Ok((code, _)) if (200..300).contains(&code) || code == 0 => {
                    return Ok(format!(
                        "Valores de alineación (A={pattern_a}, B={pattern_b}, C={pattern_c}, D={pattern_d}) guardados en {target_uri}."
                    ));
                }
                Ok((409, _)) | Ok((503, _)) => {
                    return Err(
                        "La impresora aún está procesando la hoja de alineación. Espera a que termine y vuelve a presionar '💾 Guardar Alineación'."
                            .to_string(),
                    );
                }
                Ok((code, body)) => {
                    last_code = code;
                    last_body = body;
                    if code != 400 {
                        break;
                    }
                }
                Err(e) => return Err(e),
            }
        }

        return Err(format!(
            "Respuesta HTTP {last_code} al aplicar valores en {target_uri}: {last_body}"
        ));
    }

    // No /Calibration/Session/{id} open: HP DeskJet 1115 validates alignment directly via POST /Calibration/Session
    let valid_payload = r#"<?xml version="1.0" encoding="UTF-8"?>
<CalibrationState xmlns="http://www.hp.com/schemas/imaging/con/cnx/markingagentcalibration/2009/04/08">CalibrationValid</CalibrationState>"#;

    let resp = conn.ledm_request_full("POST", "/Calibration/Session", Some(valid_payload), None)?;
    if (200..300).contains(&resp.status_code) {
        return Ok(format!(
            "Alineación confirmada en la impresora (Estado: CalibrationValid, HTTP {}). Valores seleccionados: A={pattern_a}, B={pattern_b}, C={pattern_c}, D={pattern_d}.",
            resp.status_code
        ));
    }

    Err(format!(
        "No se pudo completar la alineación en /Calibration/Session (HTTP {}): {}",
        resp.status_code, resp.body
    ))
}

fn parse_ieee1284_fallback(device_id: &str, t: &mut PrinterTelemetry) {
    for part in device_id.split(';') {
        if let Some(val) = part.strip_prefix("DES:") {
            if !val.is_empty() {
                t.product_number = val.to_string();
            }
        } else if let Some(val) = part.strip_prefix("SN:") {
            if !val.is_empty() {
                t.serial_number = val.to_string();
            }
        } else if let Some(s_hex) = part.strip_prefix("S:") {
            // HP IEEE 1284 S:03... status string
            // Example: 038000C480a00001002c240005ac140005a
            // Notice at the end: 2 pens, each ending with hex level (0x5a = 90%)!
            if s_hex.len() >= 34 && s_hex.starts_with("03") {
                let tail = &s_hex[s_hex.len() - 16..];
                let pen0_hex = &tail[6..8];
                let pen1_hex = &tail[14..16];
                let lvl0 = u8::from_str_radix(pen0_hex, 16).unwrap_or(0).min(100);
                let lvl1 = u8::from_str_radix(pen1_hex, 16).unwrap_or(0).min(100);
                t.cartridges = vec![
                    CartridgeStatus {
                        station: 0,
                        label_code: "CMY".to_string(),
                        name: "Cartucho Tricolor (HP 664)".to_string(),
                        selectability: "XL".to_string(),
                        level_percent: lvl0,
                        state: "ok".to_string(),
                        measured_state: "ok".to_string(),
                        brand: "HP".to_string(),
                        unique_id: String::new(),
                    },
                    CartridgeStatus {
                        station: 1,
                        label_code: "K".to_string(),
                        name: "Cartucho Negro (HP 664)".to_string(),
                        selectability: "XL".to_string(),
                        level_percent: lvl1,
                        state: "ok".to_string(),
                        measured_state: "ok".to_string(),
                        brand: "HP".to_string(),
                        unique_id: String::new(),
                    },
                ];
            }
        }
    }
}

fn parse_consumable_config_xml(xml: &str, t: &mut PrinterTelemetry) {
    let Ok(doc) = Document::parse(xml) else {
        return;
    };

    for node in doc.descendants() {
        match node.tag_name().name() {
            "AlignmentMode" => {
                if let Some(txt) = node.text() {
                    t.alignment_mode = txt.trim().to_string();
                }
            }
            "SingleCartridgeMode" => {
                if let Some(txt) = node.text() {
                    t.single_cartridge_mode = txt.trim().eq_ignore_ascii_case("enabled");
                }
            }
            _ => {}
        }
    }

    let mut parsed_cartridges = Vec::new();
    for info in doc
        .descendants()
        .filter(|n| n.tag_name().name() == "ConsumableInfo")
    {
        let mut station = 0u8;
        let mut label_code = String::new();
        let mut selectability = "Estándar".to_string();
        let mut level_percent = 0u8;
        let mut state = "ok".to_string();
        let mut measured_state = "ok".to_string();
        let mut brand = "HP".to_string();
        let mut unique_id = String::new();

        for child in info.descendants() {
            let txt = child.text().map(str::trim).unwrap_or("");
            match child.tag_name().name() {
                "ConsumableStation" => {
                    station = txt.parse::<u8>().unwrap_or(0);
                }
                "ConsumableLabelCode" => {
                    label_code = txt.to_string();
                }
                "ConsumableSelectibilityNumber" => {
                    if !txt.is_empty() {
                        selectability = txt.to_string();
                    }
                }
                "ConsumablePercentageLevelRemaining" => {
                    level_percent = txt.parse::<u8>().unwrap_or(0).min(100);
                }
                "ConsumableState" => {
                    state = txt.to_string();
                }
                "MeasuredQuantityState" => {
                    measured_state = txt.to_string();
                }
                "Brand" => {
                    brand = txt.to_string();
                }
                "ConsumableUniqueID" => {
                    unique_id = txt.to_string();
                }
                _ => {}
            }
        }

        let name = if label_code == "CMY" {
            format!("Cartucho Tricolor CMY (HP 664 {selectability})")
        } else if label_code == "K" {
            format!("Cartucho Negro K (HP 664 {selectability})")
        } else {
            format!("Cartucho {label_code} ({selectability})")
        };

        parsed_cartridges.push(CartridgeStatus {
            station,
            label_code,
            name,
            selectability,
            level_percent,
            state,
            measured_state,
            brand,
            unique_id,
        });
    }

    if !parsed_cartridges.is_empty() {
        t.cartridges = parsed_cartridges;
    }
}

fn parse_product_status_xml(xml: &str, t: &mut PrinterTelemetry) {
    let Ok(doc) = Document::parse(xml) else {
        return;
    };

    let mut categories = Vec::new();
    let mut alerts = Vec::new();

    for node in doc.descendants() {
        let txt = node.text().map(str::trim).unwrap_or("");
        match node.tag_name().name() {
            "StatusCategory" if !txt.is_empty() => {
                if !categories.contains(&txt.to_string()) {
                    categories.push(txt.to_string());
                }
            }
            "ProductStatusAlertID" if !txt.is_empty() => {
                if !alerts.contains(&txt.to_string()) {
                    alerts.push(txt.to_string());
                }
            }
            _ => {}
        }
    }

    t.status_categories = categories;
    t.alerts = alerts;
}

fn parse_product_config_xml(xml: &str, t: &mut PrinterTelemetry) {
    let Ok(doc) = Document::parse(xml) else {
        return;
    };

    for node in doc.descendants() {
        let txt = node.text().map(str::trim).unwrap_or("");
        match node.tag_name().name() {
            "MakeAndModel" if !txt.is_empty() => {
                t.make_and_model = txt.to_string();
            }
            "SerialNumber" if !txt.is_empty() => {
                t.serial_number = txt.to_string();
            }
            "ProductNumber" if !txt.is_empty() => {
                t.product_number = txt.to_string();
            }
            "AvailableMemory" => {
                t.memory_available_kb = txt.parse::<u32>().unwrap_or(0);
            }
            "TotalMemory" => {
                t.memory_total_kb = txt.parse::<u32>().unwrap_or(0);
            }
            "ProductInformation" => {
                for sub in node.children() {
                    if sub.tag_name().name() == "Version" {
                        for vchild in sub.children() {
                            let vtxt = vchild.text().map(str::trim).unwrap_or("");
                            match vchild.tag_name().name() {
                                "Revision" => t.firmware_revision = vtxt.to_string(),
                                "Date" => t.firmware_date = vtxt.to_string(),
                                _ => {}
                            }
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

fn parse_product_usage_xml(xml: &str, t: &mut PrinterTelemetry) {
    let Ok(doc) = Document::parse(xml) else {
        return;
    };

    for node in doc.descendants() {
        let _txt = node.text().map(str::trim).unwrap_or("");
        match node.tag_name().name() {
            "PrinterSubunit" => {
                for child in node.children() {
                    let ctxt = child.text().map(str::trim).unwrap_or("");
                    match child.tag_name().name() {
                        "TotalImpressions" => {
                            t.total_impressions = ctxt.parse::<u32>().unwrap_or(0);
                        }
                        "JamEvents" => {
                            t.jam_events = ctxt.parse::<u32>().unwrap_or(0);
                        }
                        "MispickEvents" => {
                            t.mispick_events = ctxt.parse::<u32>().unwrap_or(0);
                        }
                        "TotalFrontPanelCancelPresses" => {
                            t.cancel_presses = ctxt.parse::<u32>().unwrap_or(0);
                        }
                        _ => {}
                    }
                }
            }
            "CumulativeMarkingAgentUsed" => {
                for child in node.children() {
                    if child.tag_name().name() == "ValueFloat" {
                        if let Some(vtxt) = child.text() {
                            t.ink_used_ul = vtxt.trim().parse::<f32>().unwrap_or(0.0);
                        }
                    }
                }
            }
            _ => {}
        }
    }
}

/// Cancels any stuck LEDM jobs in `/Jobs/JobList` and performs a USB Printer Class soft reset
pub fn cancel_active_jobs(conn: &HpUsbConnection) -> Result<String, String> {
    let mut canceled = Vec::new();
    if let Ok((200, xml)) = conn.ledm_request("GET", "/Jobs/JobList", None) {
        if let Ok(doc) = Document::parse(&xml) {
            for job_node in doc.descendants().filter(|n| n.tag_name().name() == "Job") {
                let mut url = String::new();
                let mut state = String::new();
                for ch in job_node.children() {
                    let txt = ch.text().map(str::trim).unwrap_or("");
                    match ch.tag_name().name() {
                        "JobUrl" => url = txt.to_string(),
                        "JobState" => state = txt.to_string(),
                        _ => {}
                    }
                }
                if !url.is_empty()
                    && !state.eq_ignore_ascii_case("Completed")
                    && !state.eq_ignore_ascii_case("Canceled")
                {
                    let cancel_xml = format!(
                        r#"<?xml version="1.0" encoding="UTF-8"?>
<j:Job xmlns:j="http://www.hp.com/schemas/imaging/con/ledm/jobs/2009/04/30">
  <j:JobUrl>{url}</j:JobUrl>
  <j:JobState>Canceling</j:JobState>
</j:Job>"#
                    );
                    if let Ok((c, _)) = conn.ledm_request("PUT", &url, Some(&cancel_xml)) {
                        canceled.push(format!("{url} (HTTP {c})"));
                    }
                }
            }
        }
    }
    let reset_res = conn.printer_soft_reset();
    if reset_res.is_err() {
        conn.reset_device();
        return Ok("Se reinició el dispositivo USB (hard USB reset) para desbloquear el puerto.".to_string());
    }
    if canceled.is_empty() && reset_res.is_ok() {
        Ok("Se reinició el canal USB de impresión (no había trabajos cancelables por red).".to_string())
    } else if !canceled.is_empty() {
        Ok(format!("Trabajos cancelados: {}", canceled.join(", ")))
    } else {
        reset_res.map(|_| "Canal USB reiniciado.".to_string())
    }
}

#[allow(dead_code)]
fn parse_internal_print_cap_xml(xml: &str, t: &mut PrinterTelemetry) {
    let Ok(doc) = Document::parse(xml) else {
        return;
    };
    let mut jobs = Vec::new();
    for node in doc
        .descendants()
        .filter(|n| n.tag_name().name() == "JobType")
    {
        if let Some(txt) = node.text() {
            let trimmed = txt.trim().to_string();
            if !trimmed.is_empty() && !jobs.contains(&trimmed) {
                jobs.push(trimmed);
            }
        }
    }
    if !jobs.is_empty() {
        t.supported_internal_jobs = jobs;
    }
}
