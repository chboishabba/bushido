#[cfg(windows)]
use std::sync::mpsc;

#[cfg(windows)]
use std::panic::{catch_unwind, AssertUnwindSafe};

use crate::crash_log;

/// Capture visible viewport as base64 PNG via CapturePreview → IStream
#[cfg(windows)]
#[tauri::command]
pub async fn capture_visible(app: tauri::AppHandle, id: String) -> Result<String, String> {
    use tauri::Manager;

    crash_log::log_info("screenshot", &format!("capture_visible called for id={}", id));

    let wv = app.get_webview(&id).ok_or_else(|| {
        crash_log::log_error("screenshot", &format!("webview not found: {}", id));
        "webview not found".to_string()
    })?;

    let (tx, rx) = mpsc::channel::<Result<Vec<u8>, String>>();
    let tx_fallback = tx.clone();

    #[cfg(windows)]
    {
        let with_result = wv.with_webview(move |wv| {
            use webview2_com::Microsoft::Web::WebView2::Win32::*;
            use windows::Win32::System::Com::StructuredStorage::CreateStreamOnHGlobal;
            use windows::Win32::System::Com::{IStream, STREAM_SEEK_SET, STATFLAG_NONAME};
            use windows::Win32::Foundation::HGLOBAL;

            crash_log::log_info("screenshot", "inside with_webview closure");
            let tx = tx;
            unsafe {
                let controller = wv.controller();
                let core = match controller.CoreWebView2() {
                    Ok(c) => c,
                    Err(e) => {
                        crash_log::log_error("screenshot", &format!("CoreWebView2 failed: {}", e));
                        let _ = tx.send(Err(e.to_string()));
                        return;
                    }
                };

                let stream: IStream = match CreateStreamOnHGlobal(HGLOBAL::default(), true) {
                    Ok(s) => s,
                    Err(e) => {
                        crash_log::log_error("screenshot", &format!("CreateStreamOnHGlobal failed: {}", e));
                        let _ = tx.send(Err(e.to_string()));
                        return;
                    }
                };

                let stream_clone = stream.clone();
                let handler = webview2_com::CapturePreviewCompletedHandler::create(
                    Box::new(move |hr| {
                        crash_log::log_info("screenshot", &format!("CapturePreview callback fired, hr={:?}", hr));
                        let stream_ref = AssertUnwindSafe(&stream_clone);
                        let tx_ref = AssertUnwindSafe(&tx);
                        let _ = catch_unwind(move || {
                            if hr.is_err() {
                                let _ = tx_ref.send(Err(format!("CapturePreview failed: {:?}", hr)));
                                return;
                            }
                            let result = (|| -> Result<Vec<u8>, String> {
                                let stream = &*stream_ref;
                                stream.Seek(0, STREAM_SEEK_SET, None)
                                    .map_err(|e| format!("Seek failed: {}", e))?;
                                let mut stat = Default::default();
                                stream.Stat(&mut stat, STATFLAG_NONAME)
                                    .map_err(|e| format!("Stat failed: {}", e))?;
                                let size = stat.cbSize as usize;
                                crash_log::log_info("screenshot", &format!("stream size = {} bytes", size));
                                if size == 0 {
                                    return Err("Empty stream".into());
                                }
                                let mut buf = vec![0u8; size];
                                let mut read = 0u32;
                                let hr = stream.Read(
                                    buf.as_mut_ptr() as *mut _,
                                    size as u32,
                                    Some(&mut read),
                                );
                                if hr.is_err() {
                                    return Err(format!("Read failed: {:?}", hr));
                                }
                                crash_log::log_info("screenshot", &format!("read {} bytes from stream", read));
                                buf.truncate(read as usize);
                                Ok(buf)
                            })();
                            let _ = tx_ref.send(result);
                        });
                        Ok(())
                    })
                );

                let capture_hr = core.CapturePreview(
                    COREWEBVIEW2_CAPTURE_PREVIEW_IMAGE_FORMAT_PNG,
                    &stream,
                    &handler,
                );
                if capture_hr.is_err() {
                    crash_log::log_error("screenshot", &format!("CapturePreview call failed: {:?}", capture_hr));
                } else {
                    crash_log::log_info("screenshot", "CapturePreview call dispatched");
                }
            }
        });

        if let Err(e) = with_result {
            crash_log::log_error("screenshot", &format!("with_webview failed: {}", e));
            let _ = tx_fallback.send(Err(format!("with_webview failed: {}", e)));
        }
    }

    crash_log::log_info("screenshot", "waiting for capture result...");
    let bytes = rx.recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| {
            crash_log::log_error("screenshot", &format!("recv timed out: {:?}", e));
            "Capture timed out".to_string()
        })?
        .map_err(|e| {
            crash_log::log_error("screenshot", &format!("capture error: {}", e));
            e
        })?;

    crash_log::log_info("screenshot", &format!("success! {} bytes captured", bytes.len()));
    use base64::Engine as _;
    Ok(base64::engine::general_purpose::STANDARD.encode(&bytes))
}

/// Capture viewport preview + hide webview off-screen (for area select overlay)
#[cfg(windows)]
#[tauri::command]
pub async fn capture_preview_for_select(app: tauri::AppHandle, id: String) -> Result<String, String> {
    use tauri::Manager;

    let b64 = capture_visible(app.clone(), id.clone()).await?;

    if let Some(wv) = app.get_webview(&id) {
        let _ = wv.set_position(tauri::LogicalPosition::new(-9999.0, -9999.0));
    }

    Ok(b64)
}

/// Helper: call a CDP method and return the JSON response string
#[cfg(windows)]
fn cdp_call(
    wv: &tauri::webview::PlatformWebview,
    method_name: &str,
    params_json: &str,
    tx: mpsc::Sender<Result<String, String>>,
) {
    use windows::core::PCWSTR;

    unsafe {
        let controller = wv.controller();
        let core = match controller.CoreWebView2() {
            Ok(c) => c,
            Err(e) => { let _ = tx.send(Err(e.to_string())); return; }
        };

        let method: Vec<u16> = method_name.encode_utf16().chain(std::iter::once(0)).collect();
        let params_wide: Vec<u16> = params_json.encode_utf16().chain(std::iter::once(0)).collect();

        let handler = webview2_com::CallDevToolsProtocolMethodCompletedHandler::create(
            Box::new(move |hr, json| {
                let tx_ref = AssertUnwindSafe(&tx);
                let _ = catch_unwind(move || {
                    if hr.is_err() {
                        let _ = tx_ref.send(Err(format!("CDP call failed: {:?}", hr)));
                        return;
                    }
                    // webview2-com converts PCWSTR → String automatically
                    let _ = tx_ref.send(Ok(json));
                });
                Ok(())
            })
        );

        let _ = core.CallDevToolsProtocolMethod(
            PCWSTR::from_raw(method.as_ptr()),
            PCWSTR::from_raw(params_wide.as_ptr()),
            &handler,
        );
    }
}

/// Capture a specific area using CDP Page.captureScreenshot with clip
#[cfg(windows)]
#[tauri::command]
pub async fn capture_area(app: tauri::AppHandle, id: String, x: f64, y: f64, w: f64, h: f64, scale: f64) -> Result<String, String> {
    use tauri::Manager;

    crash_log::log_info("screenshot", &format!("capture_area id={} x={} y={} w={} h={} scale={}", id, x, y, w, h, scale));

    let wv = app.get_webview(&id).ok_or_else(|| {
        crash_log::log_error("screenshot", &format!("capture_area: webview not found: {}", id));
        "webview not found".to_string()
    })?;
    let (tx, rx) = mpsc::channel::<Result<String, String>>();

    let params = serde_json::json!({
        "format": "png",
        "clip": { "x": x, "y": y, "width": w, "height": h, "scale": scale }
    });
    let params_str = params.to_string();

    #[cfg(windows)]
    {
        let _ = wv.with_webview(move |wv| {
            cdp_call(&wv, "Page.captureScreenshot", &params_str, tx);
        });
    }

    let json_str = rx.recv_timeout(std::time::Duration::from_secs(10))
        .map_err(|e| {
            crash_log::log_error("screenshot", &format!("capture_area timed out: {:?}", e));
            "Area capture timed out".to_string()
        })??;

    crash_log::log_info("screenshot", "capture_area CDP response received");
    extract_cdp_data(&json_str)
}

/// Capture full page (beyond viewport) using CDP
#[cfg(windows)]
#[tauri::command]
pub async fn capture_fullpage(app: tauri::AppHandle, id: String) -> Result<String, String> {
    use tauri::Manager;

    crash_log::log_info("screenshot", &format!("capture_fullpage called for id={}", id));

    let wv = app.get_webview(&id).ok_or_else(|| {
        crash_log::log_error("screenshot", &format!("capture_fullpage: webview not found: {}", id));
        "webview not found".to_string()
    })?;

    // step 1: get layout metrics
    let (tx1, rx1) = mpsc::channel::<Result<String, String>>();

    #[cfg(windows)]
    {
        let wv_ref = wv.clone();
        let _ = wv_ref.with_webview(move |wv| {
            cdp_call(&wv, "Page.getLayoutMetrics", "{}", tx1);
        });
    }

    let metrics_json = rx1.recv_timeout(std::time::Duration::from_secs(5))
        .map_err(|e| {
            crash_log::log_error("screenshot", &format!("getLayoutMetrics timed out: {:?}", e));
            "getLayoutMetrics timed out".to_string()
        })??;

    let metrics: serde_json::Value = serde_json::from_str(&metrics_json)
        .map_err(|e| {
            crash_log::log_error("screenshot", &format!("Parse metrics failed: {}", e));
            format!("Parse metrics: {}", e)
        })?;

    let content_width = metrics.pointer("/contentSize/width")
        .and_then(|v| v.as_f64()).unwrap_or(1280.0);
    let content_height = metrics.pointer("/contentSize/height")
        .and_then(|v| v.as_f64()).unwrap_or(800.0);

    // cap to prevent OOM
    let capped_height = content_height.min(16384.0);
    crash_log::log_info("screenshot", &format!("fullpage metrics: {}x{} (capped height: {})", content_width, content_height, capped_height));

    // step 2: capture with captureBeyondViewport
    let (tx2, rx2) = mpsc::channel::<Result<String, String>>();

    let params = serde_json::json!({
        "format": "png",
        "captureBeyondViewport": true,
        "clip": { "x": 0, "y": 0, "width": content_width, "height": capped_height, "scale": 1 }
    });
    let params_str = params.to_string();

    #[cfg(windows)]
    {
        let _ = wv.with_webview(move |wv| {
            cdp_call(&wv, "Page.captureScreenshot", &params_str, tx2);
        });
    }

    let json_str = rx2.recv_timeout(std::time::Duration::from_secs(15))
        .map_err(|e| {
            crash_log::log_error("screenshot", &format!("fullpage capture timed out: {:?}", e));
            "Full page capture timed out".to_string()
        })??;

    crash_log::log_info("screenshot", "capture_fullpage CDP response received");
    extract_cdp_data(&json_str)
}

/// Extract base64 "data" field from CDP JSON response
fn extract_cdp_data(json_str: &str) -> Result<String, String> {
    let val: serde_json::Value = serde_json::from_str(json_str)
        .map_err(|e| format!("JSON parse: {}", e))?;
    val.get("data")
        .and_then(|d| d.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No data in CDP response".to_string())
}

#[cfg(target_os = "linux")]
const MAX_PIXELS: u64 = 16_777_216;
#[cfg(target_os = "linux")]
const MAX_PNG: usize = 64 * 1024 * 1024;

#[cfg(target_os = "linux")]
fn bounded_dimensions(w: u32, h: u32) -> Result<(), String> {
    if w == 0 || h == 0 || w > 16384 || h > 16384 || u64::from(w) * u64::from(h) > MAX_PIXELS {
        return Err("Screenshot dimensions exceed capture limits".into());
    }
    Ok(())
}

#[cfg(target_os = "linux")]
fn decode_png(data: &str) -> Result<(Vec<u8>, image::RgbaImage), String> {
    use base64::Engine as _;
    if data.len() > (MAX_PNG + 2) / 3 * 4 { return Err("Screenshot payload too large".into()); }
    let bytes = base64::engine::general_purpose::STANDARD.decode(data)
        .map_err(|_| "Invalid screenshot base64".to_string())?;
    if bytes.len() > MAX_PNG || bytes.len() < 33 || &bytes[..8] != b"\x89PNG\r\n\x1a\n"
        || &bytes[8..12] != 13u32.to_be_bytes().as_slice() || &bytes[12..16] != b"IHDR" {
        return Err("Invalid screenshot PNG header".into());
    }
    let w = u32::from_be_bytes(bytes[16..20].try_into().unwrap());
    let h = u32::from_be_bytes(bytes[20..24].try_into().unwrap());
    bounded_dimensions(w, h)?;
    let rgba = image::load_from_memory_with_format(&bytes, image::ImageFormat::Png)
        .map_err(|_| "Invalid screenshot PNG".to_string())?.to_rgba8();
    if rgba.dimensions() != (w, h) { return Err("Screenshot dimensions changed".into()); }
    Ok((bytes, rgba))
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn capture_visible(app: tauri::AppHandle, webview: tauri::Webview, id: String) -> Result<String, String> {
    use base64::Engine as _;
    use gtk::{glib, gio, prelude::*};
    use tauri::Manager;
    use webkit2gtk::{LoadEvent, SnapshotOptions, SnapshotRegion, WebViewExt};
    use std::{cell::Cell, rc::Rc, time::Duration};
    if webview.label() != "main" { return Err("Trusted browser chrome required".into()); }
    let owner = crate::live_native_tab_owner(&app, &id).ok_or("Not a live ordinary tab")?;
    let wv = app.get_webview(&id).ok_or("Webview not found")?;
    let pos = wv.position().map_err(|e| e.to_string())?;
    if pos.x < 0 || pos.y < 0 { return Err("Tab is not on screen".into()); }
    let (tx, rx) = tokio::sync::oneshot::channel();
    let callback_app = app.clone();
    let callback_id = id.clone();
    let callback_owner = owner.clone();
    wv.with_webview(move |platform| {
        let view = platform.inner();
        if tx.is_closed() { return; }
        let admissible = || -> Result<(), String> {
            if !crate::native_tab_matches_owner(&callback_app, &callback_id, &callback_owner)
                || !view.is_mapped() || view.is_loading() {
                return Err("Tab unavailable or loading".into());
            }
            let scale = u32::try_from(view.scale_factor()).map_err(|_| "Invalid display scale")?;
            let w = u32::try_from(view.allocated_width()).ok().and_then(|v| v.checked_mul(scale)).ok_or("Invalid viewport")?;
            let h = u32::try_from(view.allocated_height()).ok().and_then(|v| v.checked_mul(scale)).ok_or("Invalid viewport")?;
            bounded_dimensions(w, h)
        };
        if let Err(e) = admissible() { let _ = tx.send(Err(e)); return; }
        let uri = view.uri();
        let changed = Rc::new(Cell::new(false));
        let changed_signal = changed.clone();
        let handler = view.connect_load_changed(move |_, event| {
            if event == LoadEvent::Started { changed_signal.set(true); }
        });
        let cancel = gio::Cancellable::new();
        let deadline_cancel = cancel.clone();
        let completed = Rc::new(Cell::new(false));
        let deadline_completed = completed.clone();
        glib::timeout_add_local_once(Duration::from_secs(5), move || {
            if !deadline_completed.get() { deadline_cancel.cancel(); }
        });
        let callback_view = view.clone();
        view.snapshot(SnapshotRegion::Visible, SnapshotOptions::empty(), Some(&cancel), move |result| {
            completed.set(true);
            callback_view.disconnect(handler);
            if tx.is_closed() { return; }
            let png = (|| -> Result<Vec<u8>, String> {
                if changed.get() || callback_view.uri() != uri
                    || !crate::native_tab_matches_owner(&callback_app, &callback_id, &callback_owner) {
                    return Err("Tab changed during capture".into());
                }
                let surface = result.map_err(|e| e.to_string())?;
                let image = gtk::cairo::ImageSurface::try_from(surface)
                    .map_err(|_| "Unsupported snapshot surface".to_string())?;
                let (w, h) = (image.width(), image.height());
                bounded_dimensions(u32::try_from(w).map_err(|_| "Invalid width")?,
                                   u32::try_from(h).map_err(|_| "Invalid height")?)?;
                let pixbuf = gtk::gdk::pixbuf_get_from_surface(&image, 0, 0, w, h)
                    .ok_or("Snapshot pixel conversion failed")?;
                let bytes = pixbuf.save_to_bufferv("png", &[]).map_err(|e| e.to_string())?;
                if bytes.is_empty() || bytes.len() > MAX_PNG { return Err("Snapshot PNG exceeds limits".into()); }
                Ok(bytes)
            })();
            let _ = tx.send(png);
        });
    }).map_err(|e| e.to_string())?;
    let bytes = tokio::time::timeout(Duration::from_secs(6), rx).await
        .map_err(|_| "Screenshot timed out")?.map_err(|_| "Screenshot callback lost")??;
    if !crate::native_tab_matches_owner(&app, &id, &owner) { return Err("Tab changed during capture".into()); }
    Ok(base64::engine::general_purpose::STANDARD.encode(bytes))
}

// On Linux the frontend uses the frozen visible image for selection. Do not
// expose a second command that moves an arbitrary or stale native view.
#[cfg(not(windows))]
#[tauri::command]
pub async fn capture_preview_for_select(_app: tauri::AppHandle, _id: String) -> Result<String, String> {
    Err("Use the visible snapshot selection overlay".into())
}

#[tauri::command]
pub fn screenshot_fullpage_available() -> bool { cfg!(windows) }

#[cfg(target_os = "linux")]
fn safe_screenshot_name(name: &str) -> bool {
    !name.is_empty() && name.len() <= 180 && name.ends_with(".png")
        && !name.starts_with('.') && !name.contains('/') && !name.contains('\\')
        && !name.chars().any(char::is_control)
}

#[cfg(target_os = "linux")]
fn save_png_exclusive(dir: &std::path::Path, name: &str, bytes: &[u8]) -> Result<String, String> {
    use std::{io::Write, os::unix::fs::OpenOptionsExt};
    if !safe_screenshot_name(name) { return Err("Invalid screenshot filename".into()); }
    let stem = name.strip_suffix(".png").unwrap();
    for i in 0..1000 {
        let path = dir.join(if i == 0 { name.to_owned() } else { format!("{stem} ({i}).png") });
        match std::fs::OpenOptions::new().write(true).create_new(true).mode(0o600).open(&path) {
            Ok(mut file) => {
                // A failed write is explicitly reported; never unlink a path
                // that might have been replaced by another same-user process.
                file.write_all(bytes).and_then(|_| file.sync_all()).map_err(|_| "Screenshot write failed; partial file may remain")?;
                return Ok(path.to_string_lossy().into_owned());
            }
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(_) => return Err("Cannot create screenshot file".into()),
        }
    }
    Err("Too many screenshot filename collisions".into())
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn save_screenshot(webview: tauri::Webview, data: String, suggested_name: String) -> Result<String, String> {
    if webview.label() != "main" { return Err("Trusted browser chrome required".into()); }
    tokio::task::spawn_blocking(move || {
        let (bytes, _) = decode_png(&data)?;
        let dir = dirs::download_dir().ok_or("Downloads directory unavailable")?;
        let name = if suggested_name.is_empty() {
            format!("Bushido Screenshot {}.png", chrono::Local::now().format("%Y-%m-%d %H%M%S"))
        } else { suggested_name };
        save_png_exclusive(&dir, &name, &bytes)
    }).await.map_err(|_| "Screenshot worker failed".to_string())?
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn copy_image_to_clipboard(app: tauri::AppHandle, webview: tauri::Webview, data: String) -> Result<(), String> {
    use gtk::{gdk, gdk_pixbuf, glib};
    if webview.label() != "main" { return Err("Trusted browser chrome required".into()); }
    let (_, rgba) = tokio::task::spawn_blocking(move || decode_png(&data))
        .await.map_err(|_| "Screenshot decoder failed".to_string())??;
    let (w, h) = rgba.dimensions();
    let (tx, rx) = tokio::sync::oneshot::channel();
    app.run_on_main_thread(move || {
        if tx.is_closed() { return; }
        let result = (|| -> Result<(), String> {
            let display = gdk::Display::default().ok_or("No graphical clipboard display")?;
            let bytes = glib::Bytes::from_owned(rgba.into_raw());
            let pixbuf = gdk_pixbuf::Pixbuf::from_bytes(&bytes, gdk_pixbuf::Colorspace::Rgb,
                true, 8, w as i32, h as i32, (w * 4) as i32);
            // GTK retains the selection payload after this command returns,
            // unlike a temporary clipboard owner dropped on the worker.
            gtk::Clipboard::for_display(&display, &gdk::SELECTION_CLIPBOARD).set_image(&pixbuf);
            Ok(())
        })();
        let _ = tx.send(result);
    }).map_err(|e| e.to_string())?;
    tokio::time::timeout(std::time::Duration::from_secs(5), rx).await
        .map_err(|_| "Clipboard dispatch timed out")?.map_err(|_| "Clipboard callback lost")?
}

#[cfg(all(test, target_os = "linux"))]
mod linux_tests {
    use super::*;

    #[test]
    fn bounds_and_names() {
        assert!(bounded_dimensions(4096, 4096).is_ok());
        assert!(bounded_dimensions(8192, 4096).is_err());
        assert!(bounded_dimensions(0, 1).is_err());
        for invalid in ["../x.png", "/tmp/x.png", "x\\y.png", ".hidden.png", "x.jpg", "x\n.png"] {
            assert!(!safe_screenshot_name(invalid));
        }
        assert!(safe_screenshot_name("Bushido Screenshot.png"));
    }

    #[test]
    fn exclusive_write_preserves_existing_and_symlink() {
        let dir = std::env::temp_dir().join(format!("bushido-shot-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir(&dir).unwrap();
        std::fs::write(dir.join("x.png"), b"existing").unwrap();
        std::os::unix::fs::symlink(dir.join("missing"), dir.join("x (1).png")).unwrap();
        let actual = save_png_exclusive(&dir, "x.png", b"new").unwrap();
        assert!(actual.ends_with("x (2).png"));
        assert_eq!(std::fs::read(dir.join("x.png")).unwrap(), b"existing");
        assert!(!dir.join("missing").exists());
        std::fs::remove_dir_all(dir).unwrap();
    }
}

/// Save base64 PNG to Downloads directory
#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub async fn save_screenshot(data: String, suggested_name: String) -> Result<String, String> {
    use base64::Engine as _;

    crash_log::log_info("screenshot", &format!("save_screenshot called, data_len={} name={}", data.len(), suggested_name));

    let bytes = base64::engine::general_purpose::STANDARD.decode(&data)
        .map_err(|e| {
            crash_log::log_error("screenshot", &format!("save base64 decode failed: {}", e));
            format!("Base64 decode: {}", e)
        })?;

    let download_dir = dirs::download_dir()
        .unwrap_or_else(|| std::path::PathBuf::from("."));

    let name = if suggested_name.is_empty() {
        let now = chrono::Local::now();
        format!("Bushido Screenshot {}.png", now.format("%Y-%m-%d %H%M%S"))
    } else {
        suggested_name
    };

    let path = download_dir.join(&name);

    let final_path = if path.exists() {
        let stem = path.file_stem().unwrap_or_default().to_string_lossy().to_string();
        let ext = path.extension().unwrap_or_default().to_string_lossy().to_string();
        let mut i = 1;
        loop {
            let candidate = download_dir.join(format!("{} ({}).{}", stem, i, ext));
            if !candidate.exists() { break candidate; }
            i += 1;
        }
    } else {
        path
    };

    std::fs::write(&final_path, &bytes).map_err(|e| {
        crash_log::log_error("screenshot", &format!("save write failed: {} path={}", e, final_path.display()));
        format!("Write failed: {}", e)
    })?;
    crash_log::log_info("screenshot", &format!("saved {} bytes to {}", bytes.len(), final_path.display()));
    Ok(final_path.to_string_lossy().to_string())
}

/// Copy base64 PNG image to system clipboard
#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub async fn copy_image_to_clipboard(data: String) -> Result<(), String> {
    crash_log::log_info("screenshot", &format!("copy_image_to_clipboard called, data_len={}", data.len()));

    // arboard needs to run on a thread with Win32 message pump access
    tokio::task::spawn_blocking(move || {
        use base64::Engine as _;

        let bytes = base64::engine::general_purpose::STANDARD.decode(&data)
            .map_err(|e| {
                crash_log::log_error("screenshot", &format!("clipboard base64 decode failed: {}", e));
                format!("Base64 decode: {}", e)
            })?;

        let img = image::load_from_memory(&bytes)
            .map_err(|e| {
                crash_log::log_error("screenshot", &format!("clipboard image decode failed: {}", e));
                format!("Image decode: {}", e)
            })?;
        let rgba = img.to_rgba8();
        let (w, h) = rgba.dimensions();

        let img_data = arboard::ImageData {
            width: w as usize,
            height: h as usize,
            bytes: std::borrow::Cow::Owned(rgba.into_raw()),
        };

        let mut clipboard = arboard::Clipboard::new()
            .map_err(|e| {
                crash_log::log_error("screenshot", &format!("clipboard init failed: {}", e));
                format!("Clipboard init: {}", e)
            })?;
        clipboard.set_image(img_data)
            .map_err(|e| {
                crash_log::log_error("screenshot", &format!("clipboard set_image failed: {}", e));
                format!("Clipboard set: {}", e)
            })?;

        crash_log::log_info("screenshot", &format!("copied {}x{} image to clipboard", w, h));
        Ok::<(), String>(())
    })
    .await
    .map_err(|e| {
        crash_log::log_error("screenshot", &format!("clipboard thread error: {}", e));
        format!("Thread error: {}", e)
    })?
}

/// Generate QR code for a URL, return as base64 PNG
#[tauri::command]
pub async fn generate_qr_code(url: String) -> Result<String, String> {
    use base64::Engine as _;

    crash_log::log_info("screenshot", &format!("generate_qr_code for url={}", url));

    let code = qrcode::QrCode::new(url.as_bytes())
        .map_err(|e| {
            crash_log::log_error("screenshot", &format!("QR encode failed: {}", e));
            format!("QR encode: {}", e)
        })?;

    let image = code.render::<image::Luma<u8>>()
        .quiet_zone(true)
        .min_dimensions(256, 256)
        .build();

    let mut buf = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut buf);
    image::ImageEncoder::write_image(
        encoder,
        image.as_raw(),
        image.width(),
        image.height(),
        image::ExtendedColorType::L8,
    ).map_err(|e| format!("PNG encode: {}", e))?;

    Ok(base64::engine::general_purpose::STANDARD.encode(&buf))
}

#[cfg(not(any(windows, target_os = "linux")))]
#[tauri::command]
pub async fn capture_visible(_app: tauri::AppHandle, _id: String) -> Result<String, String> {
    Err("Screenshot capture is not yet implemented on Linux".into())
}

#[cfg(not(windows))]
#[tauri::command]
pub async fn capture_area(_app: tauri::AppHandle, _id: String, _x: f64, _y: f64, _w: f64, _h: f64, _scale: f64) -> Result<String, String> {
    Err("Area screenshot capture is not yet implemented on Linux".into())
}

#[cfg(not(windows))]
#[tauri::command]
pub async fn capture_fullpage(_app: tauri::AppHandle, _id: String) -> Result<String, String> {
    Err("Full-page screenshot capture is not yet implemented on Linux".into())
}
