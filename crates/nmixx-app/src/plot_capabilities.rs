use std::collections::HashSet;

use thiserror::Error;

use crate::{DeviceSession, HostSchema, SessionError};
use nmixx_core::protocol::{PLOT_CAP_FAST, PLOT_CAP_NORMAL, PlotCapabilityEntry};

#[derive(Debug, Clone, PartialEq)]
pub struct DevicePlotCapabilities {
    pub fast_max_channels: u8,
    pub normal_max_channels: u8,
    pub fast_block_samples: u8,
    pub fast_rate_hz: u32,
    pub normal_rate_hz: u32,
    pub channels: Vec<DevicePlotChannel>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct DevicePlotChannel {
    pub id: u16,
    pub modes: u8,
    pub fast_scale: f32,
}

impl DevicePlotChannel {
    pub fn supports_fast(self) -> bool {
        self.modes & PLOT_CAP_FAST != 0
    }

    pub fn supports_normal(self) -> bool {
        self.modes & PLOT_CAP_NORMAL != 0
    }
}

impl From<PlotCapabilityEntry> for DevicePlotChannel {
    fn from(value: PlotCapabilityEntry) -> Self {
        Self {
            id: value.id,
            modes: value.modes,
            fast_scale: value.fast_scale,
        }
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct PlotChannelInfo {
    pub id: u16,
    pub symbol: Option<String>,
    pub name: Option<String>,
    pub unit: Option<String>,
    pub description: Option<String>,
    pub supports_fast: bool,
    pub supports_normal: bool,
    pub fast_scale: Option<f32>,
}

#[derive(Debug, Error)]
pub enum PlotCapabilitiesError {
    #[error(transparent)]
    Session(#[from] SessionError),
    #[error("Plot capability pages disagree on device limits or sample rates")]
    InconsistentPages,
    #[error("Plot capability pagination did not make forward progress")]
    InvalidPagination,
    #[error("Plot capability list contains duplicate parameter ID 0x{0:04X}")]
    DuplicateParameter(u16),
}

impl DevicePlotCapabilities {
    pub fn discover(session: &DeviceSession) -> Result<Self, PlotCapabilitiesError> {
        let mut start = 0u8;
        let mut global: Option<(u8, u8, u8, u32, u32)> = None;
        let mut channels = Vec::new();
        let mut ids = HashSet::new();

        loop {
            let page = session.plot_capabilities_page(start)?;
            let page_global = (
                page.fast_max_channels,
                page.normal_max_channels,
                page.fast_block_samples,
                page.fast_rate_hz,
                page.normal_rate_hz,
            );
            match global {
                None => global = Some(page_global),
                Some(expected) if expected == page_global => {}
                Some(_) => return Err(PlotCapabilitiesError::InconsistentPages),
            }

            for entry in page.entries {
                if !ids.insert(entry.id) {
                    return Err(PlotCapabilitiesError::DuplicateParameter(entry.id));
                }
                channels.push(entry.into());
            }

            match page.next {
                None => break,
                Some(next) if next > start => start = next,
                Some(_) => return Err(PlotCapabilitiesError::InvalidPagination),
            }
        }

        let (fast_max_channels, normal_max_channels, fast_block_samples, fast_rate_hz, normal_rate_hz) =
            global.ok_or(PlotCapabilitiesError::InvalidPagination)?;

        Ok(Self {
            fast_max_channels,
            normal_max_channels,
            fast_block_samples,
            fast_rate_hz,
            normal_rate_hz,
            channels,
        })
    }

    pub fn channel(&self, id: u16) -> Option<DevicePlotChannel> {
        self.channels.iter().copied().find(|channel| channel.id == id)
    }

    pub fn with_schema(&self, schema: &HostSchema) -> Vec<PlotChannelInfo> {
        self.channels
            .iter()
            .map(|channel| {
                let metadata = schema.parameter_by_id(channel.id);
                PlotChannelInfo {
                    id: channel.id,
                    symbol: metadata.map(|value| value.symbol.clone()),
                    name: metadata.and_then(|value| value.name.clone()),
                    unit: metadata.and_then(|value| value.unit.clone()),
                    description: metadata.map(|value| value.description.clone()),
                    supports_fast: channel.supports_fast(),
                    supports_normal: channel.supports_normal(),
                    fast_scale: channel.supports_fast().then_some(channel.fast_scale),
                }
            })
            .collect()
    }
}
