//! Private per-window negotiation, swap-chain transitions and display policy.
mod policy;
use super::*;
use anyhow::ensure;
use policy::same_device_name;
use windows::Win32::{
    Devices::Display::*,
    Foundation::{ERROR_INSUFFICIENT_BUFFER, RECT},
    UI::WindowsAndMessaging::GetWindowRect,
};

pub(super) struct DisplayState {
    pub(super) white_scale: f32,
    pub(super) gamut: Option<bool>,
    pub(super) dirty: bool,
    valid: bool,
    sampled: std::time::Instant,
}
impl Default for DisplayState {
    fn default() -> Self {
        Self {
            white_scale: 1.,
            gamut: None,
            dirty: true,
            valid: true,
            sampled: std::time::Instant::now(),
        }
    }
}
fn format(mode: WindowColorMode) -> DXGI_FORMAT {
    if mode == WindowColorMode::WideGamutSdr {
        DXGI_FORMAT_R16G16B16A16_FLOAT
    } else {
        RENDER_TARGET_FORMAT
    }
}
fn color_space(mode: WindowColorMode) -> DXGI_COLOR_SPACE_TYPE {
    if mode == WindowColorMode::WideGamutSdr {
        DXGI_COLOR_SPACE_RGB_FULL_G10_NONE_P709
    } else {
        DXGI_COLOR_SPACE_RGB_FULL_G22_NONE_P709
    }
}
impl DirectXRenderer {
    pub(crate) fn color_status(&self) -> WindowColorStatus {
        WindowColorStatus::new(
            self.color_mode,
            self.color_mode,
            true,
            self.color_display.gamut,
            None,
        )
    }
    pub(crate) fn set_color_mode(
        &mut self,
        mode: WindowColorMode,
        appearance: WindowBackgroundAppearance,
    ) -> Result<WindowColorStatus> {
        ensure!(
            mode != WindowColorMode::WideGamutSdr
                || appearance == WindowBackgroundAppearance::Opaque,
            "wide SDR requires an opaque window"
        );
        if mode == self.color_mode {
            return Ok(self.color_status());
        }
        let devices = self.devices.as_ref().context("devices missing")?;
        let wide = if mode == WindowColorMode::WideGamutSdr {
            let flags = unsafe {
                devices
                    .device
                    .CheckFormatSupport(DXGI_FORMAT_R16G16B16A16_FLOAT)?
            };
            let required = (D3D11_FORMAT_SUPPORT_RENDER_TARGET.0
                | D3D11_FORMAT_SUPPORT_BLENDABLE.0
                | D3D11_FORMAT_SUPPORT_SHADER_SAMPLE.0) as u32;
            ensure!(
                flags & required == required,
                "device cannot blend/sample FP16 SDR"
            );
            Some(wide_sdr::Compositor::new(
                &devices.device,
                size(
                    DevicePixels(self.width as i32),
                    DevicePixels(self.height as i32),
                ),
            )?)
        } else {
            None
        };
        let display = if mode == WindowColorMode::WideGamutSdr {
            DisplayState::read(self.hwnd)?
        } else {
            DisplayState::default()
        };
        if let Err(error) = self.configure_swap_chain(mode) {
            if let Err(rollback) = self.configure_swap_chain(self.color_mode) {
                self.skip_draws = true;
                return Err(error.context(format!(
                    "color transition failed; renderer recovery required: {rollback:#}"
                )));
            }
            return Err(error);
        }
        self.wide_sdr = wide;
        self.color_mode = mode;
        self.color_display = display;
        Ok(self.color_status())
    }
    // Called only outside drawing. Never expose a second live HWND flip chain.
    fn configure_swap_chain(&mut self, mode: WindowColorMode) -> Result<()> {
        let devices = self.devices.as_ref().context("devices missing")?;
        let resources = self.resources.as_mut().context("resources missing")?;
        let Some(chain) = resources.swap_chain.clone() else {
            return Ok(());
        };
        let chain3: IDXGISwapChain3 = chain.cast()?;
        unsafe {
            devices.device_context.ClearState();
            resources.render_target.take();
            resources.render_target_view.take();
            chain.ResizeBuffers(
                BUFFER_COUNT as u32,
                self.width,
                self.height,
                format(mode),
                DXGI_SWAP_CHAIN_FLAG(0),
            )?;
        }
        resources.recreate_resources(devices, self.width, self.height)?;
        unsafe {
            chain3.SetColorSpace1(color_space(mode))?;
        }
        Ok(())
    }
    pub(crate) fn needs_color_recovery(&self) -> bool {
        self.skip_draws
    }
    pub(crate) fn mark_color_profile_dirty(&mut self) {
        self.color_display.dirty = true;
    }
    pub(crate) fn refresh_color_profile(&mut self) -> Result<bool> {
        if self.color_mode != WindowColorMode::WideGamutSdr || self.hwnd.is_invalid() {
            return Ok(false);
        }
        let current = self
            .devices
            .as_ref()
            .context("devices missing")?
            .dxgi_factory
            .clone();
        let stale = unsafe { !current.IsCurrent().as_bool() };
        // Messages are the fast path. Periodic sampling on requested frames also
        // catches SDR-white changes for which Windows sends no reliable event.
        if !self.color_display.dirty
            && !stale
            && self.color_display.sampled.elapsed() < std::time::Duration::from_secs(1)
        {
            return Ok(false);
        }
        match DisplayState::read(self.hwnd) {
            Ok(display) => {
                let changed = display.white_scale != self.color_display.white_scale
                    || display.gamut != self.color_display.gamut
                    || !self.color_display.valid;
                self.color_display = display;
                // Replace a stale factory so IsCurrent doesn't force enumeration every frame.
                if stale {
                    self.devices.as_mut().unwrap().dxgi_factory = unsafe { CreateDXGIFactory1()? };
                }
                Ok(changed)
            }
            Err(error) => {
                self.color_display.valid = false;
                self.color_display.dirty = true;
                Err(error)
            }
        }
    }
    pub(super) fn ensure_color_ready(&self) -> Result<()> {
        ensure!(
            self.color_mode != WindowColorMode::WideGamutSdr || self.color_display.valid,
            "display color configuration is unavailable; wide frame deferred"
        );
        Ok(())
    }
    pub(super) fn resize_color_space(&self) -> Result<()> {
        if let Some(chain) = self.resources.as_ref().and_then(|r| r.swap_chain.as_ref()) {
            // Default legacy chains on older systems need no new interface.
            if self.color_mode == WindowColorMode::WideGamutSdr {
                unsafe {
                    chain
                        .cast::<IDXGISwapChain3>()?
                        .SetColorSpace1(color_space(self.color_mode))?;
                }
            }
        }
        Ok(())
    }
    pub(super) fn buffer_format(&self) -> DXGI_FORMAT {
        format(self.color_mode)
    }
}
impl DisplayState {
    fn read(hwnd: HWND) -> Result<Self> {
        let mut state = Self {
            dirty: false,
            ..Default::default()
        };
        if hwnd.is_invalid() {
            return Ok(state);
        }
        let factory: IDXGIFactory6 = unsafe { CreateDXGIFactory1()? };
        let mut window = RECT::default();
        unsafe {
            GetWindowRect(hwnd, &mut window)?;
        }
        let mut best = None;
        let mut best_area = 0;
        let mut a = 0;
        while let Ok(adapter) = unsafe { factory.EnumAdapters1(a) } {
            let mut o = 0;
            while let Ok(output) = unsafe { adapter.EnumOutputs(o) } {
                let desc = unsafe { output.GetDesc()? };
                let area = overlap(window, desc.DesktopCoordinates);
                if (best.is_none() || area > best_area) && desc.AttachedToDesktop.as_bool() {
                    best = Some(output);
                    best_area = area;
                }
                o += 1;
            }
            a += 1;
        }
        let output = best.context("no active display for wide SDR window")?;
        let desc = unsafe { output.GetDesc()? };
        if let Ok(output6) = output.cast::<IDXGIOutput6>() {
            let color = unsafe { output6.GetDesc1()? };
            // Conservative positive evidence only: legacy/default sRGB metadata
            // cannot establish that a physical panel is narrow gamut.
            let [rx, ry] = color.RedPrimary;
            let [gx, gy] = color.GreenPrimary;
            let [bx, by] = color.BluePrimary;
            let area = ((gx - rx) * (by - ry) - (gy - ry) * (bx - rx)).abs();
            if area.is_finite() && area > 0.25 {
                state.gamut = Some(true);
            }
            if color.ColorSpace == DXGI_COLOR_SPACE_RGB_FULL_G2084_NONE_P2020 {
                state.white_scale = sdr_white(&desc.DeviceName)?;
            }
        } else {
            anyhow::bail!("cannot determine HDR/SDR display state for wide presentation");
        }
        Ok(state)
    }
}
fn overlap(a: RECT, b: RECT) -> u64 {
    policy::overlap(
        [a.left, a.top, a.right, a.bottom],
        [b.left, b.top, b.right, b.bottom],
    )
}
fn white_scale(level: u32) -> Result<f32> {
    policy::white_scale(level).context("invalid SDR reference white")
}
fn sdr_white(device_name: &[u16; 32]) -> Result<f32> {
    for _ in 0..3 {
        let (mut path_count, mut mode_count) = (0, 0);
        unsafe {
            GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut path_count, &mut mode_count)
                .ok()?;
        }
        let mut paths = vec![DISPLAYCONFIG_PATH_INFO::default(); path_count as usize];
        let mut modes = vec![DISPLAYCONFIG_MODE_INFO::default(); mode_count as usize];
        let result = unsafe {
            QueryDisplayConfig(
                QDC_ONLY_ACTIVE_PATHS,
                &mut path_count,
                paths.as_mut_ptr(),
                &mut mode_count,
                modes.as_mut_ptr(),
                None,
            )
        };
        if result == ERROR_INSUFFICIENT_BUFFER {
            continue;
        }
        result.ok()?;
        let mut scale = None;
        for path in &paths[..path_count as usize] {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME {
                header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME,
                    size: std::mem::size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32,
                    adapterId: path.sourceInfo.adapterId,
                    id: path.sourceInfo.id,
                },
                ..Default::default()
            };
            if unsafe { DisplayConfigGetDeviceInfo(&mut source.header) } != 0
                || !same_device_name(&source.viewGdiDeviceName, device_name)
            {
                continue;
            }
            let mut white = DISPLAYCONFIG_SDR_WHITE_LEVEL {
                header: DISPLAYCONFIG_DEVICE_INFO_HEADER {
                    r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SDR_WHITE_LEVEL,
                    size: std::mem::size_of::<DISPLAYCONFIG_SDR_WHITE_LEVEL>() as u32,
                    adapterId: path.targetInfo.adapterId,
                    id: path.targetInfo.id,
                },
                ..Default::default()
            };
            ensure!(
                unsafe { DisplayConfigGetDeviceInfo(&mut white.header) } == 0,
                "cannot query SDR reference white"
            );
            let current = white_scale(white.SDRWhiteLevel)?;
            ensure!(
                scale.is_none_or(|old| old == current),
                "ambiguous SDR white for cloned displays"
            );
            scale = Some(current);
        }
        return scale.context("no SDR white configuration for selected display");
    }
    anyhow::bail!("display configuration changed during SDR white query")
}
#[cfg(test)]
mod tests;
