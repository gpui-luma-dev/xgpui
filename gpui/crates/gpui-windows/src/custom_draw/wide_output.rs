//! Explicit linear P3 SDR output, sharing legacy host recording transactions.
use super::*;

/// Cacheable associated-alpha linear Display P3/D65 SDR content.
///
/// Host allocation fixes the storage to RGBA16Float, 2D, one mip/sample and
/// shader-readable. Producers store finite straight RGB/alpha in 0..=1, multiply
/// RGB by alpha, and store zero RGB at zero alpha. Handle validation cannot prove
/// pixel correctness. Use `Dx11CustomContext::record_wide_sdr` to encode writes;
/// only the host submits. This type does not enable wide window presentation.
#[derive(Clone, Debug)]
pub struct Dx11WideSdrOutput {
    bundle: TextureBundle,
    owner: ID3D11DeviceContext,
    recording: Arc<Recording>,
}
impl Dx11CustomContext {
    /// Allocate a host-owned linear P3, premultiplied-alpha RGBA16Float source.
    /// Extents are bounded to 16384 per dimension and 64 MiB per output.
    /// Contents are undefined until the producer records initialization.
    pub fn create_wide_sdr_output(&self, extent: Size<DevicePixels>) -> Result<Dx11WideSdrOutput> {
        ensure!(
            self.recording.state.load(Ordering::Acquire) == PENDING,
            "DirectX preparation is no longer writable"
        );
        Ok(Dx11WideSdrOutput {
            bundle: allocate_texture_format(
                &self.device,
                extent,
                DXGI_FORMAT_R16G16B16A16_FLOAT,
                8,
            )?,
            owner: self.owner.clone(),
            recording: self.recording.clone(),
        })
    }

    /// Whether the output belongs to this retained host/device and is local
    /// pending, staged in the same accepted batch, or already submitted.
    pub fn can_reuse_wide_sdr(&self, output: &Dx11WideSdrOutput) -> bool {
        self.owner.as_raw() == output.owner.as_raw() && self.recording.can_reuse(&output.recording)
    }

    /// Record writes to an output created in this transaction. Rejects writes
    /// to submitted/staged, abandoned or foreign outputs before calling `encode`.
    /// An encoding error invalidates the entire transaction, as for `record`.
    /// Raw D3D handles remain cooperative: do not retain and mutate published views.
    pub fn record_wide_sdr<T>(
        &self,
        output: &Dx11WideSdrOutput,
        encode: impl FnOnce(&ID3D11DeviceContext, &ID3D11RenderTargetView) -> Result<T>,
    ) -> Result<T> {
        ensure!(
            self.owner.as_raw() == output.owner.as_raw()
                && Arc::ptr_eq(&self.recording, &output.recording)
                && self.recording.state.load(Ordering::Acquire) == PENDING,
            "cannot mutate submitted or foreign DirectX wide output"
        );
        self.record(|deferred| encode(deferred, &output.bundle.rtv))
    }
}
impl Dx11WideSdrOutput {
    /// Texture for producer view creation/sampling. Immutable after submission;
    /// all initialization/writes must be recorded on the supplied host context.
    pub fn texture(&self) -> &ID3D11Texture2D {
        &self.bundle.texture
    }

    pub(crate) fn validate(
        self,
        context: &Dx11CustomContext,
        extent: Size<DevicePixels>,
    ) -> Result<TextureBundle> {
        ensure!(
            context.can_reuse_wide_sdr(&self),
            "foreign or abandoned DirectX wide output"
        );
        ensure!(
            self.bundle.extent == extent,
            "invalid DirectX wide output extent"
        );
        ensure!(
            unsafe { self.bundle.texture.GetDevice()? }.as_raw() == context.device.as_raw(),
            "DirectX wide output belongs to another device"
        );
        let mut desc = D3D11_TEXTURE2D_DESC::default();
        unsafe {
            self.bundle.texture.GetDesc(&mut desc);
        }
        ensure!(
            desc.Format == DXGI_FORMAT_R16G16B16A16_FLOAT
                && desc.Width == extent.width.0 as u32
                && desc.Height == extent.height.0 as u32
                && desc.MipLevels == 1
                && desc.ArraySize == 1
                && desc.SampleDesc.Count == 1
                && desc.SampleDesc.Quality == 0
                && desc.BindFlags & D3D11_BIND_SHADER_RESOURCE.0 as u32 != 0,
            "invalid DirectX wide texture structure"
        );
        Ok(self.bundle)
    }
}

#[cfg(test)]
mod tests;
