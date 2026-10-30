use std::{
    ffi::{CStr, c_char, c_int, c_uint, c_void},
    mem,
    ptr::NonNull,
};

const NVML_SUCCESS: c_int = 0;
const GPU_TEMPERATURE_SENSOR: c_uint = 0;
const CLOCK_GRAPHICS: c_uint = 0;
const CLOCK_MEMORY: c_uint = 2;
const MAX_DEVICES: c_uint = 256;
const PCI_BUS_ID_LEGACY_LENGTH: usize = 16;

type Device = *mut c_void;
type Init = unsafe extern "C" fn() -> c_int;
type Shutdown = unsafe extern "C" fn() -> c_int;
type GetCount = unsafe extern "C" fn(*mut c_uint) -> c_int;
type GetHandleByIndex = unsafe extern "C" fn(c_uint, *mut Device) -> c_int;
type GetName = unsafe extern "C" fn(Device, *mut c_char, c_uint) -> c_int;
type GetPciInfo = unsafe extern "C" fn(Device, *mut PciInfoBuffer) -> c_int;
type GetUtilization = unsafe extern "C" fn(Device, *mut NvmlUtilization) -> c_int;
type GetMemoryInfo = unsafe extern "C" fn(Device, *mut NvmlMemory) -> c_int;
type GetTemperature = unsafe extern "C" fn(Device, c_uint, *mut c_uint) -> c_int;
type GetDeviceValue = unsafe extern "C" fn(Device, *mut c_uint) -> c_int;
type GetClockInfo = unsafe extern "C" fn(Device, c_uint, *mut c_uint) -> c_int;
type GetEngineUtilization = unsafe extern "C" fn(Device, *mut c_uint, *mut c_uint) -> c_int;

#[repr(C)]
#[derive(Default)]
struct NvmlUtilization {
    gpu: c_uint,
    memory: c_uint,
}

#[repr(C)]
#[derive(Default)]
struct NvmlMemory {
    total: u64,
    free: u64,
    used: u64,
}

#[repr(C, align(8))]
struct PciInfoBuffer([u8; 256]);

#[derive(Debug, Clone, Default)]
pub struct DeviceMetrics {
    pub source: Option<&'static str>,
    pub name: Option<String>,
    pub uuid: Option<String>,
    pub pci_address: Option<String>,
    pub gpu_busy_percent: Option<u8>,
    pub memory_busy_percent: Option<u8>,
    pub vram_used_bytes: Option<u64>,
    pub vram_total_bytes: Option<u64>,
    pub temperature_celsius: Option<u32>,
    pub power_milliwatts: Option<u32>,
    pub power_limit_milliwatts: Option<u32>,
    pub graphics_clock_mhz: Option<u32>,
    pub memory_clock_mhz: Option<u32>,
    pub fan_percent: Option<u8>,
    pub encoder_busy_percent: Option<u8>,
    pub decoder_busy_percent: Option<u8>,
}

pub struct NvmlApi {
    library: NonNull<c_void>,
    shutdown: Shutdown,
    get_count: GetCount,
    get_handle_by_index: GetHandleByIndex,
    get_name: Option<GetName>,
    get_uuid: Option<GetName>,
    get_pci_info: Option<GetPciInfo>,
    get_utilization: Option<GetUtilization>,
    get_memory_info: Option<GetMemoryInfo>,
    get_temperature: Option<GetTemperature>,
    get_power_usage: Option<GetDeviceValue>,
    get_power_limit: Option<GetDeviceValue>,
    get_clock_info: Option<GetClockInfo>,
    get_fan_speed: Option<GetDeviceValue>,
    get_encoder_utilization: Option<GetEngineUtilization>,
    get_decoder_utilization: Option<GetEngineUtilization>,
}

impl NvmlApi {
    pub fn load() -> Option<Self> {
        let library_name = c"libnvidia-ml.so.1";
        let library = NonNull::new(unsafe { dlopen(library_name.as_ptr(), RTLD_NOW | RTLD_LOCAL) })?;
        let init = unsafe { load_symbol_any::<Init>(library.as_ptr(), &[c"nvmlInit_v2", c"nvmlInit"]) };
        let shutdown = unsafe { load_symbol::<Shutdown>(library.as_ptr(), c"nvmlShutdown") };
        let get_count = unsafe {
            load_symbol_any::<GetCount>(library.as_ptr(), &[c"nvmlDeviceGetCount_v2", c"nvmlDeviceGetCount"])
        };
        let get_handle_by_index = unsafe {
            load_symbol_any::<GetHandleByIndex>(
                library.as_ptr(),
                &[c"nvmlDeviceGetHandleByIndex_v2", c"nvmlDeviceGetHandleByIndex"],
            )
        };
        let (Some(init), Some(shutdown), Some(get_count), Some(get_handle_by_index)) =
            (init, shutdown, get_count, get_handle_by_index)
        else {
            unsafe { dlclose(library.as_ptr()) };
            return None;
        };
        if unsafe { init() } != NVML_SUCCESS {
            unsafe { dlclose(library.as_ptr()) };
            return None;
        }

        Some(Self {
            library,
            shutdown,
            get_count,
            get_handle_by_index,
            get_name: unsafe { load_symbol(library.as_ptr(), c"nvmlDeviceGetName") },
            get_uuid: unsafe { load_symbol(library.as_ptr(), c"nvmlDeviceGetUUID") },
            get_pci_info: unsafe {
                load_symbol_any::<GetPciInfo>(
                    library.as_ptr(),
                    &[c"nvmlDeviceGetPciInfo_v3", c"nvmlDeviceGetPciInfo_v2", c"nvmlDeviceGetPciInfo"],
                )
            },
            get_utilization: unsafe {
                load_symbol(library.as_ptr(), c"nvmlDeviceGetUtilizationRates")
            },
            get_memory_info: unsafe {
                load_symbol(library.as_ptr(), c"nvmlDeviceGetMemoryInfo")
            },
            get_temperature: unsafe {
                load_symbol(library.as_ptr(), c"nvmlDeviceGetTemperature")
            },
            get_power_usage: unsafe {
                load_symbol(library.as_ptr(), c"nvmlDeviceGetPowerUsage")
            },
            get_power_limit: unsafe {
                load_symbol(library.as_ptr(), c"nvmlDeviceGetPowerManagementLimit")
            },
            get_clock_info: unsafe { load_symbol(library.as_ptr(), c"nvmlDeviceGetClockInfo") },
            get_fan_speed: unsafe { load_symbol(library.as_ptr(), c"nvmlDeviceGetFanSpeed") },
            get_encoder_utilization: unsafe {
                load_symbol(library.as_ptr(), c"nvmlDeviceGetEncoderUtilization")
            },
            get_decoder_utilization: unsafe {
                load_symbol(library.as_ptr(), c"nvmlDeviceGetDecoderUtilization")
            },
        })
    }

    pub fn sample(&self) -> Vec<DeviceMetrics> {
        let mut count = 0;
        if unsafe { (self.get_count)(&mut count) } != NVML_SUCCESS {
            return Vec::new();
        }
        (0..count.min(MAX_DEVICES))
            .filter_map(|index| {
                let mut device = std::ptr::null_mut();
                (unsafe { (self.get_handle_by_index)(index, &mut device) } == NVML_SUCCESS)
                    .then(|| self.sample_device(device))
            })
            .collect()
    }

    fn sample_device(&self, device: Device) -> DeviceMetrics {
        let mut metrics = DeviceMetrics {
            source: Some("NVML"),
            ..DeviceMetrics::default()
        };
        metrics.name = self.query_string(self.get_name, device, 256);
        metrics.uuid = self.query_string(self.get_uuid, device, 80);
        metrics.pci_address = self.query_pci_address(device);

        if let Some(function) = self.get_utilization {
            let mut utilization = NvmlUtilization::default();
            if unsafe { function(device, &mut utilization) } == NVML_SUCCESS {
                metrics.gpu_busy_percent = valid_percent(utilization.gpu);
                metrics.memory_busy_percent = valid_percent(utilization.memory);
            }
        }
        if let Some(function) = self.get_memory_info {
            let mut memory = NvmlMemory::default();
            if unsafe { function(device, &mut memory) } == NVML_SUCCESS {
                if memory.total > 0 {
                    metrics.vram_total_bytes = Some(memory.total);
                    metrics.vram_used_bytes = (memory.used <= memory.total).then_some(memory.used);
                }
            }
        }
        metrics.temperature_celsius = self.query_temperature(device);
        metrics.power_milliwatts = self.query_device_value(self.get_power_usage, device);
        metrics.power_limit_milliwatts = self.query_device_value(self.get_power_limit, device);
        metrics.graphics_clock_mhz = self.query_clock(device, CLOCK_GRAPHICS);
        metrics.memory_clock_mhz = self.query_clock(device, CLOCK_MEMORY);
        metrics.fan_percent = self
            .query_device_value(self.get_fan_speed, device)
            .and_then(valid_percent);
        metrics.encoder_busy_percent = self.query_engine_utilization(self.get_encoder_utilization, device);
        metrics.decoder_busy_percent = self.query_engine_utilization(self.get_decoder_utilization, device);
        metrics
    }

    fn query_string(&self, function: Option<GetName>, device: Device, buffer_length: usize) -> Option<String> {
        let function = function?;
        let mut buffer = vec![0_i8; buffer_length];
        if unsafe { function(device, buffer.as_mut_ptr(), buffer.len() as c_uint) } != NVML_SUCCESS {
            return None;
        }
        let value = unsafe { CStr::from_ptr(buffer.as_ptr()) }.to_str().ok()?.trim();
        (!value.is_empty() && !value.chars().any(char::is_control)).then(|| value.to_owned())
    }

    fn query_pci_address(&self, device: Device) -> Option<String> {
        let function = self.get_pci_info?;
        let mut buffer = PciInfoBuffer([0; 256]);
        if unsafe { function(device, &mut buffer) } != NVML_SUCCESS {
            return None;
        }
        let address = CStr::from_bytes_until_nul(&buffer.0[..PCI_BUS_ID_LEGACY_LENGTH])
            .ok()?
            .to_str()
            .ok()?;
        Some(address.to_owned())
    }

    fn query_temperature(&self, device: Device) -> Option<u32> {
        let mut value = 0;
        (unsafe {
            self.get_temperature?(device, GPU_TEMPERATURE_SENSOR, &mut value)
        } == NVML_SUCCESS)
            .then_some(value)
    }

    fn query_device_value(&self, function: Option<GetDeviceValue>, device: Device) -> Option<u32> {
        let mut value = 0;
        (unsafe { function?(device, &mut value) } == NVML_SUCCESS).then_some(value)
    }

    fn query_clock(&self, device: Device, clock_type: c_uint) -> Option<u32> {
        let mut value = 0;
        (unsafe { self.get_clock_info?(device, clock_type, &mut value) } == NVML_SUCCESS)
            .then_some(value)
    }

    fn query_engine_utilization(&self, function: Option<GetEngineUtilization>, device: Device) -> Option<u8> {
        let mut utilization = 0;
        let mut sampling_period_us = 0;
        if unsafe { function?(device, &mut utilization, &mut sampling_period_us) } != NVML_SUCCESS {
            return None;
        }
        valid_percent(utilization)
    }
}

impl Drop for NvmlApi {
    fn drop(&mut self) {
        unsafe {
            (self.shutdown)();
            dlclose(self.library.as_ptr());
        }
    }
}

fn valid_percent(value: c_uint) -> Option<u8> {
    u8::try_from(value).ok().filter(|value| *value <= 100)
}

unsafe fn load_symbol<T: Copy>(library: *mut c_void, name: &CStr) -> Option<T> {
    let symbol = unsafe { dlsym(library, name.as_ptr()) };
    if symbol.is_null() {
        None
    } else {
        Some(unsafe { mem::transmute_copy(&symbol) })
    }
}

unsafe fn load_symbol_any<T: Copy>(library: *mut c_void, names: &[&CStr]) -> Option<T> {
    names
        .iter()
        .find_map(|name| unsafe { load_symbol(library, name) })
}

const RTLD_LOCAL: c_int = 0;
const RTLD_NOW: c_int = 2;

#[link(name = "dl")]
unsafe extern "C" {
    fn dlopen(filename: *const c_char, flags: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
    fn dlclose(handle: *mut c_void) -> c_int;
}

#[cfg(test)]
mod tests {
    use super::valid_percent;

    #[test]
    fn rejects_out_of_range_nvml_percentages() {
        assert_eq!(valid_percent(0), Some(0));
        assert_eq!(valid_percent(100), Some(100));
        assert_eq!(valid_percent(101), None);
        assert_eq!(valid_percent(u32::MAX), None);
    }
}
