//! GPU context management for wgpu device and queue access.
//!
//! Provides both standalone (headless) and shared (from eframe) GPU initialization.

use std::sync::Arc;

use crate::error::{MicrotomeError, Result};

/// Shared GPU context wrapping a wgpu device and queue.
///
/// Can be created standalone for headless/testing use, or from an existing
/// device and queue provided by a windowing framework like eframe.
pub struct GpuContext {
    /// The wgpu logical device.
    pub device: Arc<wgpu::Device>,
    /// The wgpu command queue.
    pub queue: Arc<wgpu::Queue>,
}

impl GpuContext {
    /// Creates a new standalone GPU context for headless or testing use.
    ///
    /// Requests a high-performance adapter and creates a device with default limits.
    pub async fn new_standalone() -> Result<Self> {
        let instance =
            wgpu::Instance::new(wgpu::InstanceDescriptor::new_without_display_handle_from_env());

        let adapter = instance
            .request_adapter(&wgpu::RequestAdapterOptions {
                power_preference: wgpu::PowerPreference::HighPerformance,
                force_fallback_adapter: false,
                compatible_surface: None,
            })
            .await
            .map_err(|e| MicrotomeError::GpuInit(e.to_string()))?;

        let (device, queue) = adapter
            .request_device(&wgpu::DeviceDescriptor {
                label: Some("microtome-device"),
                required_features: wgpu::Features::empty(),
                required_limits: wgpu::Limits::default(),
                ..Default::default()
            })
            .await
            .map_err(|e| MicrotomeError::GpuInit(e.to_string()))?;

        Ok(Self {
            device: Arc::new(device),
            queue: Arc::new(queue),
        })
    }

    /// Wraps an existing wgpu device and queue (e.g., from eframe).
    pub fn from_existing(device: Arc<wgpu::Device>, queue: Arc<wgpu::Queue>) -> Self {
        Self { device, queue }
    }
}
