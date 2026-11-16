#![allow(non_snake_case, non_camel_case_types, dead_code)]

use std::ffi::{CStr, c_char, c_int, c_uchar, c_uint};
use std::ptr;
use std::slice;
use std::time::{Duration, Instant};

pub const HP_VENDOR_ID: u16 = 0x03f0;
pub const DESKJET_1110_PRODUCT_ID: u16 = 0xdf11;
pub const DESKJET_1110_ALT_PRODUCT_ID: u16 = 0xe011;

#[repr(C)]
pub struct libusb_context {
    _private: [u8; 0],
}

#[repr(C)]
pub struct libusb_device {
    _private: [u8; 0],
}

#[repr(C)]
pub struct libusb_device_handle {
    _private: [u8; 0],
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct libusb_device_descriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bcdUSB: u16,
    pub bDeviceClass: u8,
    pub bDeviceSubClass: u8,
    pub bDeviceProtocol: u8,
    pub bMaxPacketSize0: u8,
    pub idVendor: u16,
    pub idProduct: u16,
    pub bcdDevice: u16,
    pub iManufacturer: u8,
    pub iProduct: u8,
    pub iSerialNumber: u8,
    pub bNumConfigurations: u8,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct libusb_endpoint_descriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bEndpointAddress: u8,
    pub bmAttributes: u8,
    pub wMaxPacketSize: u16,
    pub bInterval: u8,
    pub bRefresh: u8,
    pub bSynchAddress: u8,
    pub extra: *const c_uchar,
    pub extra_length: c_int,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct libusb_interface_descriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub bInterfaceNumber: u8,
    pub bAlternateSetting: u8,
    pub bNumEndpoints: u8,
    pub bInterfaceClass: u8,
    pub bInterfaceSubClass: u8,
    pub bInterfaceProtocol: u8,
    pub iInterface: u8,
    pub endpoint: *const libusb_endpoint_descriptor,
    pub extra: *const c_uchar,
    pub extra_length: c_int,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct libusb_interface {
    pub altsetting: *const libusb_interface_descriptor,
    pub num_altsetting: c_int,
}

#[repr(C)]
#[derive(Debug, Copy, Clone)]
pub struct libusb_config_descriptor {
    pub bLength: u8,
    pub bDescriptorType: u8,
    pub wTotalLength: u16,
    pub bNumInterfaces: u8,
    pub bConfigurationValue: u8,
    pub iConfiguration: u8,
    pub bmAttributes: u8,
    pub MaxPower: u8,
    pub interface: *const libusb_interface,
    pub extra: *const c_uchar,
    pub extra_length: c_int,
}

unsafe extern "C" {
    fn libusb_init(ctx: *mut *mut libusb_context) -> c_int;
    fn libusb_exit(ctx: *mut libusb_context);
    fn libusb_get_device_list(
        ctx: *mut libusb_context,
        list: *mut *mut *mut libusb_device,
    ) -> isize;
    fn libusb_free_device_list(list: *mut *mut libusb_device, unref_devices: c_int);
    fn libusb_get_device_descriptor(
        dev: *mut libusb_device,
        desc: *mut libusb_device_descriptor,
    ) -> c_int;
    fn libusb_get_active_config_descriptor(
        dev: *mut libusb_device,
        config: *mut *mut libusb_config_descriptor,
    ) -> c_int;
    fn libusb_free_config_descriptor(config: *mut libusb_config_descriptor);
    fn libusb_open(dev: *mut libusb_device, dev_handle: *mut *mut libusb_device_handle) -> c_int;
    fn libusb_close(dev_handle: *mut libusb_device_handle);
    fn libusb_set_auto_detach_kernel_driver(
        dev_handle: *mut libusb_device_handle,
        enable: c_int,
    ) -> c_int;
    fn libusb_claim_interface(
        dev_handle: *mut libusb_device_handle,
        interface_number: c_int,
    ) -> c_int;
    fn libusb_release_interface(
        dev_handle: *mut libusb_device_handle,
        interface_number: c_int,
    ) -> c_int;
    fn libusb_clear_halt(dev_handle: *mut libusb_device_handle, endpoint: c_uchar) -> c_int;
    fn libusb_reset_device(dev_handle: *mut libusb_device_handle) -> c_int;
    fn libusb_control_transfer(
        dev_handle: *mut libusb_device_handle,
        request_type: u8,
        bRequest: u8,
        wValue: u16,
        wIndex: u16,
        data: *mut c_uchar,
        wLength: u16,
        timeout: c_uint,
    ) -> c_int;
    fn libusb_bulk_transfer(
        dev_handle: *mut libusb_device_handle,
        endpoint: c_uchar,
        data: *mut c_uchar,
        length: c_int,
        actual_length: *mut c_int,
        timeout: c_uint,
    ) -> c_int;
    fn libusb_get_string_descriptor_ascii(
        dev_handle: *mut libusb_device_handle,
        desc_index: u8,
        data: *mut c_uchar,
        length: c_int,
    ) -> c_int;
    fn libusb_strerror(errcode: c_int) -> *const c_char;
}

pub fn usb_error_name(err: c_int) -> String {
    unsafe {
        let ptr = libusb_strerror(err);
        if ptr.is_null() {
            format!("USB Error {err}")
        } else {
            CStr::from_ptr(ptr).to_string_lossy().into_owned()
        }
    }
}

#[derive(Debug, Clone)]
pub struct UsbInterfaceEndpoints {
    pub interface_number: u8,
    pub interface_class: u8,
    pub interface_subclass: u8,
    pub interface_protocol: u8,
    pub ep_out: u8,
    pub ep_in: u8,
}

#[derive(Debug, Clone)]
pub struct PrinterUsbInfo {
    pub vendor_id: u16,
    pub product_id: u16,
    pub manufacturer: String,
    pub product: String,
    pub serial_number: String,
    pub device_id_string: String,
    pub print_interface: Option<UsbInterfaceEndpoints>,
    pub ledm_interface: Option<UsbInterfaceEndpoints>,
}

pub struct HpUsbConnection {
    ctx: *mut libusb_context,
    handle: *mut libusb_device_handle,
    pub info: PrinterUsbInfo,
}

impl Drop for HpUsbConnection {
    fn drop(&mut self) {
        unsafe {
            if !self.handle.is_null() {
                libusb_close(self.handle);
            }
            if !self.ctx.is_null() {
                libusb_exit(self.ctx);
            }
        }
    }
}

impl HpUsbConnection {
    pub fn open() -> Result<Self, String> {
        unsafe {
            let mut ctx: *mut libusb_context = ptr::null_mut();
            let rc = libusb_init(&mut ctx);
            if rc < 0 {
                return Err(format!("No se pudo inicializar libusb: {}", usb_error_name(rc)));
            }

            let mut list: *mut *mut libusb_device = ptr::null_mut();
            let count = libusb_get_device_list(ctx, &mut list);
            if count < 0 {
                libusb_exit(ctx);
                return Err(format!(
                    "No se pudo listar dispositivos USB: {}",
                    usb_error_name(count as c_int)
                ));
            }

            let devices = slice::from_raw_parts(list, count as usize);
            let mut matched_dev: *mut libusb_device = ptr::null_mut();
            let mut matched_desc: Option<libusb_device_descriptor> = None;

            for &dev in devices {
                let mut desc: libusb_device_descriptor = std::mem::zeroed();
                if libusb_get_device_descriptor(dev, &mut desc) == 0 {
                    if desc.idVendor == HP_VENDOR_ID
                        && (desc.idProduct == DESKJET_1110_PRODUCT_ID
                            || desc.idProduct == DESKJET_1110_ALT_PRODUCT_ID)
                    {
                        matched_dev = dev;
                        matched_desc = Some(desc);
                        break;
                    }
                }
            }

            if matched_dev.is_null() {
                libusb_free_device_list(list, 1);
                libusb_exit(ctx);
                return Err(
                    "No se encontró la impresora HP DeskJet Ink Advantage 1115 (1110 series) por USB."
                        .to_string(),
                );
            }

            let desc = matched_desc.unwrap();
            let mut print_interface = None;
            let mut ledm_interface = None;

            let mut config: *mut libusb_config_descriptor = ptr::null_mut();
            if libusb_get_active_config_descriptor(matched_dev, &mut config) == 0
                && !config.is_null()
            {
                let cfg = &*config;
                let ifaces = slice::from_raw_parts(cfg.interface, cfg.bNumInterfaces as usize);
                for iface in ifaces {
                    if iface.num_altsetting > 0 && !iface.altsetting.is_null() {
                        let alt = &*iface.altsetting;
                        let eps =
                            slice::from_raw_parts(alt.endpoint, alt.bNumEndpoints as usize);
                        let mut ep_out = 0;
                        let mut ep_in = 0;
                        for ep in eps {
                            if (ep.bmAttributes & 0x03) == 0x02 {
                                if (ep.bEndpointAddress & 0x80) != 0 {
                                    ep_in = ep.bEndpointAddress;
                                } else {
                                    ep_out = ep.bEndpointAddress;
                                }
                            }
                        }
                        let ep_info = UsbInterfaceEndpoints {
                            interface_number: alt.bInterfaceNumber,
                            interface_class: alt.bInterfaceClass,
                            interface_subclass: alt.bInterfaceSubClass,
                            interface_protocol: alt.bInterfaceProtocol,
                            ep_out,
                            ep_in,
                        };
                        if alt.bInterfaceClass == 7 && print_interface.is_none() {
                            print_interface = Some(ep_info);
                        } else if alt.bInterfaceClass == 0xFF
                            && alt.bInterfaceSubClass == 0x04
                            && alt.bInterfaceProtocol == 0x01
                            && ledm_interface.is_none()
                        {
                            ledm_interface = Some(ep_info);
                        }
                    }
                }
                libusb_free_config_descriptor(config);
            }

            let mut handle: *mut libusb_device_handle = ptr::null_mut();
            let open_rc = libusb_open(matched_dev, &mut handle);
            libusb_free_device_list(list, 1);

            if open_rc < 0 || handle.is_null() {
                libusb_exit(ctx);
                return Err(format!(
                    "No se pudo abrir el dispositivo USB HP DeskJet 1115: {}",
                    usb_error_name(open_rc)
                ));
            }

            let _ = libusb_set_auto_detach_kernel_driver(handle, 1);

            let manufacturer = read_string_descriptor(handle, desc.iManufacturer)
                .unwrap_or_else(|| "HP".to_string());
            let product = read_string_descriptor(handle, desc.iProduct)
                .unwrap_or_else(|| "DeskJet 1110 series".to_string());
            let serial_number = read_string_descriptor(handle, desc.iSerialNumber)
                .unwrap_or_else(|| "Desconocido".to_string());

            let device_id_string = read_ieee1284_device_id(handle).unwrap_or_default();

            Ok(Self {
                ctx,
                handle,
                info: PrinterUsbInfo {
                    vendor_id: desc.idVendor,
                    product_id: desc.idProduct,
                    manufacturer,
                    product,
                    serial_number,
                    device_id_string,
                    print_interface,
                    ledm_interface,
                },
            })
        }
    }

    pub fn refresh_device_id(&mut self) -> Result<String, String> {
        let id = read_ieee1284_device_id(self.handle)?;
        self.info.device_id_string = id.clone();
        Ok(id)
    }

    pub fn reset_device(&self) {
        unsafe {
            let _ = libusb_reset_device(self.handle);
        }
    }

    /// Sends an HTTP request over the HP LEDM USB interface (Interface 1: Class 0xFF, SubClass 0x04, Protocol 0x01)
    pub fn ledm_request(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
    ) -> Result<(u16, String), String> {
        self.ledm_request_expect(method, path, body, None)
    }

    fn claim_interface_with_retry(&self, interface_number: u8) -> Result<(), String> {
        let mut last_rc = 0;
        for attempt in 0..5 {
            if attempt > 0 {
                std::thread::sleep(Duration::from_millis(200));
            }
            let rc = unsafe { libusb_claim_interface(self.handle, interface_number as c_int) };
            if rc == 0 {
                return Ok(());
            }
            last_rc = rc;
            // Retry on LIBUSB_ERROR_ACCESS (-3) or LIBUSB_ERROR_BUSY (-6)
            if rc != -3 && rc != -6 {
                break;
            }
        }
        Err(format!(
            "No se pudo reclamar la interfaz USB #{interface_number}: {}",
            usb_error_name(last_rc)
        ))
    }

    pub fn ledm_request_expect(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        expected_substr: Option<&str>,
    ) -> Result<(u16, String), String> {
        let resp = self.ledm_request_full(method, path, body, expected_substr)?;
        Ok((resp.status_code, resp.body))
    }

    pub fn ledm_request_full(
        &self,
        method: &str,
        path: &str,
        body: Option<&str>,
        expected_substr: Option<&str>,
    ) -> Result<LedmHttpResponse, String> {
        let ledm = self
            .info
            .ledm_interface
            .as_ref()
            .ok_or_else(|| "La impresora no expone interfaz LEDM USB (FF/04/01)".to_string())?;

        self.claim_interface_with_retry(ledm.interface_number)?;

        let result = (|| {
            // Drain any stale leftover bytes from an interrupted transfer
            let mut drain = [0u8; 4096];
            for _ in 0..4 {
                let mut transferred = 0;
                let rc = unsafe {
                    libusb_bulk_transfer(
                        self.handle,
                        ledm.ep_in,
                        drain.as_mut_ptr(),
                        drain.len() as c_int,
                        &mut transferred,
                        50,
                    )
                };
                if rc < 0 || transferred == 0 {
                    break;
                }
            }

            let req = if let Some(b) = body {
                format!(
                    "{method} {path} HTTP/1.1\r\nHost: localhost\r\nUser-Agent: HP-Rust-Driver/1.0\r\nContent-Type: text/xml; charset=utf-8\r\nContent-Length: {}\r\n\r\n{b}",
                    b.len()
                )
            } else {
                format!(
                    "{method} {path} HTTP/1.1\r\nHost: localhost\r\nUser-Agent: HP-Rust-Driver/1.0\r\nAccept: text/xml, application/xml, */*\r\n\r\n"
                )
            };

            let req_bytes = req.as_bytes();
            let mut sent_total = 0usize;
            while sent_total < req_bytes.len() {
                let mut actual = 0;
                let chunk = &req_bytes[sent_total..];
                let rc = unsafe {
                    libusb_bulk_transfer(
                        self.handle,
                        ledm.ep_out,
                        chunk.as_ptr() as *mut c_uchar,
                        chunk.len() as c_int,
                        &mut actual,
                        3000,
                    )
                };
                if rc < 0 {
                    return Err(format!(
                        "Error al escribir petición LEDM ({path}): {}",
                        usb_error_name(rc)
                    ));
                }
                if actual == 0 {
                    break;
                }
                sent_total += actual as usize;
            }

            // Read up to 2 responses if the first one was a stale buffered response from a prior call
            for _attempt in 0..2 {
                let mut resp_buf = Vec::with_capacity(16384);
                let mut chunk = [0u8; 4096];
                let start = Instant::now();

                while start.elapsed() < Duration::from_millis(4500) {
                    let mut actual = 0;
                    let rc = unsafe {
                        libusb_bulk_transfer(
                            self.handle,
                            ledm.ep_in,
                            chunk.as_mut_ptr(),
                            chunk.len() as c_int,
                            &mut actual,
                            1500,
                        )
                    };
                    if actual > 0 {
                        resp_buf.extend_from_slice(&chunk[..actual as usize]);
                        if http_response_complete(&resp_buf) {
                            break;
                        }
                    } else if rc < 0 {
                        if String::from_utf8_lossy(&resp_buf).contains("HTTP/1.") {
                            break;
                        }
                        return Err(format!(
                            "Tiempo de espera leyendo LEDM ({path}): {}",
                            usb_error_name(rc)
                        ));
                    }
                }

                let parsed = parse_http_response_full(&resp_buf)?;
                if let Some(exp) = expected_substr {
                    if parsed.status_code == 200 && !parsed.body.contains(exp) {
                        // Stale buffered response from an earlier call; discard and read the real one
                        continue;
                    }
                }
                return Ok(parsed);
            }

            Err(format!("Respuesta LEDM desincronizada para {path}"))
        })();

        unsafe {
            let _ = libusb_release_interface(self.handle, ledm.interface_number as c_int);
        }

        result
    }

    /// Sends USB Printer Class SOFT_RESET (bmRequestType=0x21, bRequest=0x02) on Interface 0
    pub fn printer_soft_reset(&self) -> Result<(), String> {
        let prn_iface = self
            .info
            .print_interface
            .as_ref()
            .map(|p| p.interface_number as u16)
            .unwrap_or(0);
        let rc = unsafe {
            libusb_control_transfer(
                self.handle,
                0x21,
                0x02,
                0x0000,
                prn_iface,
                ptr::null_mut(),
                0,
                2000,
            )
        };
        if rc < 0 {
            Err(format!("Error en USB Soft Reset: {}", usb_error_name(rc)))
        } else {
            Ok(())
        }
    }

    /// Sends raw PCL3GUI / PJL / PML / DYN data to the USB Printer interface (Interface 0: Class 0x07)
    pub fn send_raw_print_data(&self, data: &[u8]) -> Result<usize, String> {
        let session = self.begin_print_session()?;
        session.write_all(data)
    }

    pub fn begin_print_session(&self) -> Result<UsbPrintSession<'_>, String> {
        let prn = self
            .info
            .print_interface
            .as_ref()
            .ok_or_else(|| "No se encontró interfaz USB Printer (Class 7)".to_string())?
            .clone();
        self.claim_interface_with_retry(prn.interface_number)?;
        Ok(UsbPrintSession { conn: self, prn })
    }
}

pub struct UsbPrintSession<'a> {
    conn: &'a HpUsbConnection,
    prn: UsbInterfaceEndpoints,
}

impl<'a> Drop for UsbPrintSession<'a> {
    fn drop(&mut self) {
        unsafe {
            let _ = libusb_release_interface(self.conn.handle, self.prn.interface_number as c_int);
        }
    }
}

impl<'a> UsbPrintSession<'a> {
    pub fn write_all(&self, data: &[u8]) -> Result<usize, String> {
        let mut total = 0usize;
        let mut in_buf = [0u8; 512];

        for chunk in data.chunks(16384) {
            let mut offset = 0usize;
            let stall_start = Instant::now();

            while offset < chunk.len() {
                let mut actual = 0;
                let sub = &chunk[offset..];
                let rc = unsafe {
                    libusb_bulk_transfer(
                        self.conn.handle,
                        self.prn.ep_out,
                        sub.as_ptr() as *mut c_uchar,
                        sub.len() as c_int,
                        &mut actual,
                        5000,
                    )
                };
                if actual > 0 {
                    offset += actual as usize;
                    total += actual as usize;
                }
                if rc == 0 {
                    continue;
                }
                // LIBUSB_ERROR_TIMEOUT (-7): printer RAM buffer is full while physically printing
                if rc == -7 {
                    // Drain any USB printer backchannel bytes so the printer doesn't stall
                    if self.prn.ep_in != 0 {
                        let mut in_actual = 0;
                        unsafe {
                            let _ = libusb_bulk_transfer(
                                self.conn.handle,
                                self.prn.ep_in,
                                in_buf.as_mut_ptr(),
                                in_buf.len() as c_int,
                                &mut in_actual,
                                20,
                            );
                        }
                    }
                    if stall_start.elapsed() < Duration::from_secs(120) {
                        std::thread::sleep(Duration::from_millis(100));
                        continue;
                    }
                }
                return Err(format!(
                    "Error enviando datos a la impresora USB: {}",
                    usb_error_name(rc)
                ));
            }
        }
        Ok(total)
    }
}

fn read_string_descriptor(handle: *mut libusb_device_handle, index: u8) -> Option<String> {
    if index == 0 {
        return None;
    }
    let mut buf = [0u8; 256];
    let len = unsafe {
        libusb_get_string_descriptor_ascii(handle, index, buf.as_mut_ptr(), buf.len() as c_int)
    };
    if len > 0 {
        Some(String::from_utf8_lossy(&buf[..len as usize]).trim().to_string())
    } else {
        None
    }
}

pub fn read_ieee1284_device_id(handle: *mut libusb_device_handle) -> Result<String, String> {
    let mut buf = [0u8; 1024];
    // Class-specific IN request to Interface 0: GET_DEVICE_ID (bRequest = 0x00, bmRequestType = 0xA1)
    let len = unsafe {
        libusb_control_transfer(
            handle,
            0xA1,
            0x00,
            0x0000,
            0x0000,
            buf.as_mut_ptr(),
            buf.len() as u16,
            2000,
        )
    };
    if len < 2 {
        return Err(format!(
            "No se pudo leer IEEE 1284 Device ID: {}",
            usb_error_name(len)
        ));
    }
    let declared_len = u16::from_be_bytes([buf[0], buf[1]]) as usize;
    let end = declared_len.min(len as usize).max(2);
    Ok(String::from_utf8_lossy(&buf[2..end]).trim().to_string())
}

fn has_complete_xml_or_status(buf: &[u8]) -> bool {
    let text = String::from_utf8_lossy(buf);
    if let Some(hdr_end) = text.find("\r\n\r\n") {
        let headers = &text[..hdr_end];
        let body = text[(hdr_end + 4)..].trim();
        if headers.starts_with("HTTP/1.1 204")
            || headers.starts_with("HTTP/1.1 202")
            || headers.starts_with("HTTP/1.1 404")
            || headers.starts_with("HTTP/1.1 405")
        {
            return true;
        }
        if body.ends_with('>') && body.contains("</") {
            return true;
        }
    }
    false
}

fn http_response_complete(buf: &[u8]) -> bool {
    let full_text = String::from_utf8_lossy(buf);
    let Some(http_pos) = full_text.find("HTTP/1.") else {
        return false;
    };
    let text = &full_text[http_pos..];
    if let Some(hdr_end) = text.find("\r\n\r\n") {
        let headers = &text[..hdr_end];
        let body = &text[(hdr_end + 4)..];
        for line in headers.lines() {
            let lower = line.to_ascii_lowercase();
            if let Some(rest) = lower.strip_prefix("content-length:") {
                if let Ok(expected) = rest.trim().parse::<usize>() {
                    return body.len() >= expected;
                }
                return false;
            }
            if lower.contains("transfer-encoding: chunked") {
                return body.ends_with("0\r\n\r\n") || body.contains("\r\n0\r\n\r\n");
            }
        }
        let trimmed = body.trim();
        if trimmed.ends_with('>') && trimmed.contains("</") {
            return true;
        }
        if headers.starts_with("HTTP/1.1 201")
            || headers.starts_with("HTTP/1.1 202")
            || headers.starts_with("HTTP/1.1 204")
            || headers.starts_with("HTTP/1.1 400")
            || headers.starts_with("HTTP/1.1 404")
            || headers.starts_with("HTTP/1.1 405")
            || headers.starts_with("HTTP/1.1 500")
            || headers.starts_with("HTTP/1.1 503")
        {
            return true;
        }
    }
    false
}

pub struct LedmHttpResponse {
    pub status_code: u16,
    pub headers: String,
    pub body: String,
}

impl LedmHttpResponse {
    pub fn header(&self, name: &str) -> Option<&str> {
        let needle = name.to_ascii_lowercase();
        for line in self.headers.lines().skip(1) {
            if let Some((k, v)) = line.split_once(':') {
                if k.trim().eq_ignore_ascii_case(&needle) {
                    return Some(v.trim());
                }
            }
        }
        None
    }
}

fn parse_http_response_full(buf: &[u8]) -> Result<LedmHttpResponse, String> {
    let full_text = String::from_utf8_lossy(buf);
    if let Some(http_pos) = full_text.find("HTTP/1.") {
        let text = &full_text[http_pos..];
        if let Some(hdr_end) = text.find("\r\n\r\n") {
            let headers = &text[..hdr_end];
            let status_line = headers.lines().next().unwrap_or("");
            let status_code = status_line
                .split_whitespace()
                .nth(1)
                .and_then(|s| s.parse::<u16>().ok())
                .unwrap_or(0);

            let raw_body = &text[(hdr_end + 4)..];
            let is_chunked = headers
                .lines()
                .any(|l| l.to_ascii_lowercase().contains("transfer-encoding: chunked"));

            let body = if is_chunked {
                decode_chunked_body(raw_body)
            } else {
                raw_body.trim().to_string()
            };

            return Ok(LedmHttpResponse {
                status_code,
                headers: headers.to_string(),
                body,
            });
        }
    }
    if let Some(xml_start) = full_text.find("<?xml") {
        return Ok(LedmHttpResponse {
            status_code: 200,
            headers: String::new(),
            body: full_text[xml_start..].trim().to_string(),
        });
    }
    Err(format!(
        "Respuesta HTTP LEDM incompleta ({} bytes): {:?}",
        buf.len(),
        &full_text[..full_text.len().min(200)]
    ))
}

fn decode_chunked_body(input: &str) -> String {
    let mut out = String::new();
    let bytes = input.as_bytes();
    let mut pos = 0;
    while pos < bytes.len() {
        let Some(rel_end) = input[pos..].find("\r\n") else {
            out.push_str(&input[pos..]);
            break;
        };
        let len_str = input[pos..pos + rel_end].trim();
        let chunk_size = usize::from_str_radix(len_str.split(';').next().unwrap_or(""), 16);
        let Ok(size) = chunk_size else {
            out.push_str(&input[pos..]);
            break;
        };
        pos += rel_end + 2;
        if size == 0 {
            break;
        }
        if pos + size <= bytes.len() {
            out.push_str(&String::from_utf8_lossy(&bytes[pos..pos + size]));
            pos += size;
            if input[pos..].starts_with("\r\n") {
                pos += 2;
            }
        } else {
            out.push_str(&String::from_utf8_lossy(&bytes[pos..]));
            break;
        }
    }
    out.trim().to_string()
}
