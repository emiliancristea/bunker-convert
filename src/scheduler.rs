use clap::ValueEnum;
use serde::Deserialize;

#[derive(Debug, Clone, Deserialize, ValueEnum)]
#[serde(rename_all = "kebab-case")]
pub enum DevicePolicy {
    Auto,
    CpuOnly,
    GpuPreferred,
}

impl Default for DevicePolicy {
    fn default() -> Self {
        DevicePolicy::Auto
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StageDevice {
    Cpu,
    Gpu,
}

#[derive(Debug, Clone)]
pub struct TaskScheduler {
    policy: DevicePolicy,
    gpu_available: bool,
}

impl TaskScheduler {
    pub fn new(policy: DevicePolicy) -> Self {
        let gpu_available = detect_gpu();
        Self {
            policy,
            gpu_available,
        }
    }

    pub fn select_device(&self, _stage_name: &str) -> StageDevice {
        match self.policy {
            DevicePolicy::CpuOnly => StageDevice::Cpu,
            DevicePolicy::GpuPreferred => {
                if self.gpu_available {
                    StageDevice::Gpu
                } else {
                    StageDevice::Cpu
                }
            }
            DevicePolicy::Auto => {
                if self.gpu_available {
                    StageDevice::Gpu
                } else {
                    StageDevice::Cpu
                }
            }
        }
    }

    pub fn gpu_available(&self) -> bool {
        self.gpu_available
    }
}

fn detect_gpu() -> bool {
    // Placeholder heuristic; in real implementation this would query CUDA/Metal/Vulkan.
    std::env::var("BUNKER_FORCE_GPU")
        .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
        .unwrap_or(false)
}
