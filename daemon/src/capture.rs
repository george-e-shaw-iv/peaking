/// Screen capture using Windows Graphics Capture API (WGC).
///
/// WGC is a first-party Windows 10 (1903+) API that captures the primary monitor
/// at the GPU driver level.  It is safe from anti-cheat systems because it uses the
/// same mechanism as Xbox Game Bar.
///
/// On non-Windows platforms the public API compiles but `run` returns an error.
use anyhow::Result;
use tokio::sync::{mpsc, watch};

/// A single captured video frame as tightly-packed BGRA8 pixels.
#[derive(Debug)]
pub struct RawFrame {
    /// Row-major BGRA pixels: width × height × 4 bytes.
    pub bgra_data: Vec<u8>,
}

// ── Windows implementation ────────────────────────────────────────────────────

#[cfg(windows)]
mod imp {
    use std::sync::Arc;
    use std::time::Duration;

    use anyhow::{Context, Result};
    use tokio::sync::{mpsc, watch};
    use windows::core::Interface;
    use windows::Foundation::TypedEventHandler;
    use windows::Graphics::Capture::{
        Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem,
        GraphicsCaptureSession,
    };
    use windows::Win32::System::WinRT::Direct3D11::IDirect3DDxgiInterfaceAccess;
    use windows::Graphics::DirectX::DirectXPixelFormat;
    use windows::Win32::Foundation::POINT;
    use windows::Win32::Graphics::Direct3D::D3D_DRIVER_TYPE_HARDWARE;
    use windows::Win32::Graphics::Direct3D11::{
        D3D11CreateDevice, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
        D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC,
        D3D11_USAGE_STAGING, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D,
    };
    use windows::Win32::Graphics::Dxgi::IDXGIDevice;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
    use windows::Win32::Graphics::Dxgi::Common::DXGI_SAMPLE_DESC;
    use windows::Win32::Graphics::Gdi::{MONITOR_DEFAULTTOPRIMARY, MonitorFromPoint};
    use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;

    use super::RawFrame;

    /// Safety: IDirect3DDevice wraps a D3D11 device, which is thread-safe.
    struct SendDevice(windows::Graphics::DirectX::Direct3D11::IDirect3DDevice);
    unsafe impl Send for SendDevice {}

    /// Creates a hardware D3D11 device with BGRA surface support.
    fn create_d3d11_device() -> Result<(ID3D11Device, ID3D11DeviceContext)> {
        let mut device = None;
        let mut context = None;
        unsafe {
            D3D11CreateDevice(
                None,
                D3D_DRIVER_TYPE_HARDWARE,
                None,
                D3D11_CREATE_DEVICE_BGRA_SUPPORT,
                None,
                D3D11_SDK_VERSION,
                Some(&mut device),
                None,
                Some(&mut context),
            )
            .context("D3D11CreateDevice failed")?;
        }
        Ok((device.unwrap(), context.unwrap()))
    }

    /// Wraps a D3D11 device into the `IDirect3DDevice` WinRT interface required by WGC.
    fn create_direct3d_device(
        d3d_device: &ID3D11Device,
    ) -> Result<windows::Graphics::DirectX::Direct3D11::IDirect3DDevice> {
        let dxgi_device: IDXGIDevice = d3d_device.cast()?;
        let inspectable = unsafe {
            windows::Win32::System::WinRT::Direct3D11::CreateDirect3D11DeviceFromDXGIDevice(
                &dxgi_device,
            )?
        };
        Ok(inspectable.cast()?)
    }

    /// Copies a WGC frame's GPU surface into a CPU-side BGRA byte vector,
    /// handling row-pitch padding.
    unsafe fn readback_frame(
        device: &ID3D11Device,
        context: &ID3D11DeviceContext,
        frame: &Direct3D11CaptureFrame,
        width: u32,
        height: u32,
    ) -> Result<Vec<u8>> {
        let surface = frame.Surface()?;
        let dxgi_access: IDirect3DDxgiInterfaceAccess = surface.cast()?;
        let texture: ID3D11Texture2D = dxgi_access.GetInterface()?;

        // Staging texture for CPU readback.
        let desc = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_STAGING,
            BindFlags: 0,
            CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
            MiscFlags: 0,
        };
        let mut staging: Option<ID3D11Texture2D> = None;
        device
            .CreateTexture2D(&desc, None, Some(&mut staging))
            .context("CreateTexture2D (staging) failed")?;
        let staging = staging.unwrap();

        context.CopyResource(&staging, &texture);

        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        context
            .Map(&staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped))
            .context("ID3D11DeviceContext::Map failed")?;

        let row_pitch = mapped.RowPitch as usize;
        let row_bytes = width as usize * 4;
        let mut bgra = Vec::with_capacity(height as usize * row_bytes);
        for row in 0..height as usize {
            let src = std::slice::from_raw_parts(
                (mapped.pData as *const u8).add(row * row_pitch),
                row_bytes,
            );
            bgra.extend_from_slice(src);
        }

        context.Unmap(&staging, 0);
        Ok(bgra)
    }

    pub async fn run(
        frame_tx: mpsc::Sender<RawFrame>,
        mut stop_rx: watch::Receiver<bool>,
    ) -> Result<()> {
        let (d3d_device, d3d_context) = create_d3d11_device()?;
        let direct3d_device = SendDevice(create_direct3d_device(&d3d_device)?);

        // Get the primary monitor and create a WGC capture item for it.
        let monitor =
            unsafe { MonitorFromPoint(POINT { x: 0, y: 0 }, MONITOR_DEFAULTTOPRIMARY) };

        let capture_item: GraphicsCaptureItem = unsafe {
            let interop = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>()?;
            interop.CreateForMonitor(monitor)?
        };

        let size = capture_item.Size()?;
        let width = size.Width as u32;
        let height = size.Height as u32;

        // CreateFreeThreaded: no dispatcher queue / message pump needed.
        let frame_pool = Direct3D11CaptureFramePool::CreateFreeThreaded(
            &direct3d_device.0,
            DirectXPixelFormat::B8G8R8A8UIntNormalized,
            2,
            size,
        )?;

        let session: GraphicsCaptureSession = frame_pool.CreateCaptureSession(&capture_item)?;

        // Bridge WGC callback → std sync channel (non-blocking send from callback).
        let (cb_tx, cb_rx) = std::sync::mpsc::sync_channel::<Direct3D11CaptureFrame>(4);
        let cb_tx = Arc::new(cb_tx);

        frame_pool.FrameArrived(&TypedEventHandler::new({
            let cb_tx = Arc::clone(&cb_tx);
            move |pool: &Option<Direct3D11CaptureFramePool>, _| {
                if let Some(pool) = pool {
                    if let Ok(frame) = pool.TryGetNextFrame() {
                        let _ = cb_tx.try_send(frame);
                    }
                }
                Ok(())
            }
        }))?;

        // Disable the yellow capture border (requires Windows 11 22H2+; silently ignored on older builds).
        let _ = session.SetIsBorderRequired(false);

        session.StartCapture()?;
        eprintln!("[capture] WGC session started ({}×{})", width, height);

        loop {
            if *stop_rx.borrow_and_update() {
                break;
            }

            match cb_rx.recv_timeout(Duration::from_millis(50)) {
                Ok(frame) => {
                    match unsafe { readback_frame(&d3d_device, &d3d_context, &frame, width, height) }
                    {
                        Ok(bgra_data) => {
                            let raw = RawFrame { bgra_data };
                            if frame_tx.send(raw).await.is_err() {
                                break; // Encoder task dropped.
                            }
                        }
                        Err(e) => eprintln!("[capture] Frame readback error: {e}"),
                    }
                    drop(frame);
                }
                Err(std::sync::mpsc::RecvTimeoutError::Timeout) => continue,
                Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            }
        }

        session.Close()?;
        frame_pool.Close()?;
        eprintln!("[capture] WGC session closed");
        Ok(())
    }
}

// ── Public API ────────────────────────────────────────────────────────────────

/// Captures the primary monitor using WGC, sending [`RawFrame`]s to `frame_tx`
/// until `stop_rx` is set to `true`.
pub async fn run(
    frame_tx: mpsc::Sender<RawFrame>,
    stop_rx: watch::Receiver<bool>,
) -> Result<()> {
    #[cfg(windows)]
    {
        imp::run(frame_tx, stop_rx).await
    }
    #[cfg(not(windows))]
    {
        let _ = (frame_tx, stop_rx);
        anyhow::bail!("Screen capture (WGC) is only supported on Windows")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raw_frame_stores_data() {
        let data = vec![0u8; 4];
        let frame = RawFrame { bgra_data: data.clone() };
        assert_eq!(frame.bgra_data, data);
    }

    /// On non-Windows the `run` stub must return an error immediately.
    #[cfg(not(windows))]
    #[tokio::test]
    async fn run_returns_error_on_non_windows() {
        let (tx, _rx) = mpsc::channel(1);
        let (_stop_tx, stop_rx) = watch::channel(false);
        let result = run(tx, stop_rx).await;
        assert!(result.is_err());
        let msg = format!("{}", result.unwrap_err());
        assert!(msg.contains("Windows"));
    }
}
