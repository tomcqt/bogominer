mod global;

use crate::backend::miner::Miner;
use global::{GlobalGpuMiner, SelectedAdapter};

const VENDOR_NVIDIA: u32 = 0x10DE;
const VENDOR_AMD: u32 = 0x1002;
const VENDOR_INTEL: u32 = 0x8086;
const VENDOR_APPLE: u32 = 0x106B;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Vendor {
    Nvidia,
    Amd,
    Intel,
    Apple,
    Other,
}

impl Vendor {
    fn from_id(id: u32) -> Self {
        match id & 0xFFFF {
            VENDOR_NVIDIA => Vendor::Nvidia,
            VENDOR_AMD => Vendor::Amd,
            VENDOR_INTEL => Vendor::Intel,
            VENDOR_APPLE => Vendor::Apple,
            _ => Vendor::Other,
        }
    }

    fn as_str(&self) -> &'static str {
        match self {
            Vendor::Nvidia => "nvidia",
            Vendor::Amd => "amd",
            Vendor::Intel => "intel",
            Vendor::Apple => "apple",
            Vendor::Other => "other",
        }
    }
}

// rank adapters so we prefer a real discrete gpu over integrated / cpu / other.
fn rank(device_type: wgpu::DeviceType) -> u8 {
    match device_type {
        wgpu::DeviceType::DiscreteGpu => 4,
        wgpu::DeviceType::IntegratedGpu => 3,
        wgpu::DeviceType::VirtualGpu => 2,
        wgpu::DeviceType::Cpu => 1,
        wgpu::DeviceType::Other => 0,
    }
}

fn pick_adapter(verbose: bool) -> Option<(wgpu::Adapter, Vendor, String)> {
    let instance = wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: wgpu::Backends::all(),
        ..Default::default()
    });

    let mut best: Option<(wgpu::Adapter, Vendor, String, u8)> = None;
    for adapter in instance.enumerate_adapters(wgpu::Backends::all()) {
        let info = adapter.get_info();
        let vendor = Vendor::from_id(info.vendor);
        let has_int64 = adapter.features().contains(wgpu::Features::SHADER_INT64);
        if verbose {
            eprintln!(
                "[gpu] candidate: name={:?} vendor={} type={:?} backend={:?} int64={}",
                info.name,
                vendor.as_str(),
                info.device_type,
                info.backend,
                has_int64
            );
        }
        if !has_int64 {
            continue;
        }
        let score = rank(info.device_type);
        let label = format!("{}, {:?}", info.name, info.backend);
        match &best {
            Some((_, _, _, best_score)) if *best_score >= score => {}
            _ => best = Some((adapter, vendor, label, score)),
        }
    }

    best.map(|(a, v, l, _)| (a, v, l))
}

// public entry point used by the worker when gpu mining is enabled.
// prefers native backends when feature is present
pub fn select_backend() -> Result<Box<dyn Miner>, String> {
    let (adapter, vendor, label) =
        pick_adapter(true).ok_or_else(|| "no gpu with SHADER_INT64 support found".to_string())?;

    eprintln!(
        "[gpu] selected vendor={} adapter={}",
        vendor.as_str(),
        label
    );

    #[cfg(feature = "gpu-cuda")]
    if vendor == Vendor::Nvidia {
        match cuda::CudaMiner::new() {
            Ok(m) => {
                let m: Box<dyn Miner> = Box::new(m);
                eprintln!("[gpu] backend chosen: {}", m.name());
                return Ok(m);
            }
            Err(e) => eprintln!("[gpu] cuda unavailable ({e}); falling back"),
        }
    }

    #[cfg(feature = "gpu-hip")]
    if vendor == Vendor::Amd {
        match hip::HipMiner::new() {
            Ok(m) => {
                let m: Box<dyn Miner> = Box::new(m);
                eprintln!("[gpu] backend chosen: {}", m.name());
                return Ok(m);
            }
            Err(e) => eprintln!("[gpu] hip unabailable ({e}); falling back"),
        }
    }

    let selected = SelectedAdapter { adapter, label };
    let miner: Box<dyn Miner> = Box::new(GlobalGpuMiner::new(selected)?);
    eprintln!("[gpu] backend chosen: {}", miner.name());
    Ok(miner)
}

// same as select_backend but without constructing real miner, for
// settings panel. returns core name
pub fn _probe_active_core() -> Option<String> {
    let (_, vendor, label) = pick_adapter(false)?;

    #[cfg(feature = "gpu-cuda")]
    if vendor == Vendor::Nvidia && cuda::CudaMiner::is_runtime_available() {
        return Some(format!("gpu/cuda ({})", label));
    }

    #[cfg(feature = "gpu-hip")]
    if vendor == Vendor::Amd && hip::HipMiner::is_runtime_available() {
        return Some(format!("gpu/hip ({})", label));
    }

    let _ = vendor;
    Some(format!("gpu/global ({})", label))
}

// cheap probe for the settings panel: is any usable gpu present?
pub fn is_available() -> bool {
    pick_adapter(false).is_some()
}

// describe the gpu that would be selected, for the settings panel.
pub fn describe() -> Option<String> {
    pick_adapter(false).map(|(_, v, l)| format!("{} ({})", l, v.as_str()))
}
