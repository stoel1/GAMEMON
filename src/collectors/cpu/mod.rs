use std::fs;

pub fn snapshot() -> String {
    let model = fs::read_to_string("/proc/cpuinfo").ok().and_then(|data| data.lines().find(|line| line.starts_with("model name")).and_then(|line| line.split_once(':').map(|(_, value)| value.trim().to_owned()))).unwrap_or_else(|| "Onbekende CPU".to_owned());
    let online = fs::read_to_string("/sys/devices/system/cpu/online").map(|value| value.trim().to_owned()).unwrap_or_else(|_| "onbekend".to_owned());
    format!("CPU\n{model}\nOnline logical CPU's: {online}")
}
