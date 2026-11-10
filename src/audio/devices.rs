//! Host and input-device discovery.
//!
//! With ASIO only one driver can be loaded at a time, so enumerate only while no stream is open.

use anyhow::{Context, anyhow};
use cpal::traits::{DeviceTrait, HostTrait};
use cpal::{HostId, SampleFormat, SupportedBufferSize};

use super::capture::SUPPORTED_FORMATS;

pub const STANDARD_RATES: [u32; 6] = [44_100, 48_000, 88_200, 96_000, 176_400, 192_000];
const PREFERRED_RATE: u32 = 48_000;
const BUFFER_CHOICES: [u32; 8] = [32, 64, 128, 256, 512, 1024, 2048, 4096];

#[derive(Debug, Clone, PartialEq)]
pub struct DeviceInfo {
    pub name: String,
    pub channels: u16,
    pub sample_format: SampleFormat,
    pub default_rate: u32,
    pub rates: Vec<u32>,
    pub buffer: SupportedBufferSize,
}

impl DeviceInfo {
    /// The saved rate if the device supports it, else its default, else 48 kHz or the first rate.
    pub fn pick_rate(&self, saved: Option<u32>) -> u32 {
        saved
            .filter(|r| self.rates.contains(r))
            .or(Some(self.default_rate).filter(|r| *r > 0 && self.rates.contains(r)))
            .or(Some(PREFERRED_RATE).filter(|r| self.rates.contains(r)))
            .or(self.rates.first().copied())
            .unwrap_or(PREFERRED_RATE)
    }

    /// Fixed buffer sizes the driver accepts (empty when it does not say).
    pub fn buffer_choices(&self) -> Vec<u32> {
        match self.buffer {
            SupportedBufferSize::Range { min, max } => BUFFER_CHOICES
                .into_iter()
                .filter(|b| (min..=max).contains(b))
                .collect(),
            SupportedBufferSize::Unknown => Vec::new(),
        }
    }
}

pub fn host_ids() -> Vec<HostId> {
    cpal::available_hosts()
}

pub fn host_by_name(name: &str) -> Option<HostId> {
    host_ids().into_iter().find(|h| h.name() == name)
}

/// The saved host if still available, else ASIO when compiled in, else the platform default.
pub fn preferred_host(saved: Option<&str>) -> HostId {
    if let Some(id) = saved.and_then(host_by_name) {
        return id;
    }
    let ids = host_ids();
    ids.iter()
        .copied()
        .find(|h| h.name() == "ASIO")
        .unwrap_or_else(|| cpal::default_host().id())
}

pub fn list_input_devices(host_id: HostId) -> anyhow::Result<Vec<DeviceInfo>> {
    let host = cpal::host_from_id(host_id)
        .with_context(|| format!("{} を初期化できません", host_id.name()))?;
    let devices = host
        .input_devices()
        .with_context(|| format!("{} の入力デバイスを列挙できません", host_id.name()))?;
    Ok(devices.filter_map(|d| describe(&d)).collect())
}

pub fn find_input_device(host_id: HostId, name: &str) -> anyhow::Result<cpal::Device> {
    let host = cpal::host_from_id(host_id)
        .with_context(|| format!("{} を初期化できません", host_id.name()))?;
    host.input_devices()
        .with_context(|| format!("{} の入力デバイスを列挙できません", host_id.name()))?
        .find(|d| d.to_string() == name)
        .ok_or_else(|| anyhow!("入力デバイス「{name}」が見つかりません"))
}

pub fn default_input_device_name(host_id: HostId) -> Option<String> {
    cpal::host_from_id(host_id)
        .ok()?
        .default_input_device()
        .map(|d| d.to_string())
}

pub fn describe(device: &cpal::Device) -> Option<DeviceInfo> {
    let name = device.to_string();
    let configs: Vec<_> = match device.supported_input_configs() {
        Ok(c) => c
            .filter(|c| SUPPORTED_FORMATS.contains(&c.sample_format()))
            .collect(),
        Err(e) => {
            log::info!("skipping input device {name}: {e}");
            return None;
        }
    };
    let channels = configs.iter().map(|c| c.channels()).max()?;
    let default = device.default_input_config().ok();
    let sample_format = default
        .as_ref()
        .map(|d| d.sample_format())
        .filter(|f| SUPPORTED_FORMATS.contains(f))
        .or_else(|| configs.first().map(|c| c.sample_format()))?;
    let usable = configs
        .iter()
        .filter(|c| c.channels() == channels && c.sample_format() == sample_format);
    let mut rates: Vec<u32> = STANDARD_RATES
        .into_iter()
        .filter(|r| {
            usable
                .clone()
                .any(|c| (c.min_sample_rate()..=c.max_sample_rate()).contains(r))
        })
        .collect();
    let default_rate = default.as_ref().map_or(0, |d| d.sample_rate());
    if default_rate > 0 && !rates.contains(&default_rate) {
        rates.push(default_rate);
        rates.sort_unstable();
    }
    let buffer = default
        .as_ref()
        .map_or(SupportedBufferSize::Unknown, |d| *d.buffer_size());
    Some(DeviceInfo {
        name,
        channels,
        sample_format,
        default_rate,
        rates,
        buffer,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(rates: Vec<u32>, default_rate: u32) -> DeviceInfo {
        DeviceInfo {
            name: "x".into(),
            channels: 4,
            sample_format: SampleFormat::I32,
            default_rate,
            rates,
            buffer: SupportedBufferSize::Range { min: 64, max: 1024 },
        }
    }

    #[test]
    fn pick_rate_prefers_saved_then_default_then_48k() {
        let d = info(vec![44_100, 48_000, 96_000], 44_100);
        assert_eq!(d.pick_rate(Some(96_000)), 96_000);
        assert_eq!(d.pick_rate(Some(192_000)), 44_100);
        assert_eq!(d.pick_rate(None), 44_100);
        let unknown_default = info(vec![44_100, 48_000], 0);
        assert_eq!(unknown_default.pick_rate(None), 48_000);
        assert_eq!(info(vec![96_000], 0).pick_rate(None), 96_000);
    }

    #[test]
    fn buffer_choices_respect_driver_range() {
        assert_eq!(
            info(vec![], 0).buffer_choices(),
            vec![64, 128, 256, 512, 1024]
        );
        let mut d = info(vec![], 0);
        d.buffer = SupportedBufferSize::Unknown;
        assert!(d.buffer_choices().is_empty());
    }
}
