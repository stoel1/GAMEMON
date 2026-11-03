use std::{collections::HashMap, fs, time::Instant};

#[derive(Debug, Clone)]
pub struct ProcessSnapshot {
    pub pid: u32,
    pub name: String,
    pub cpu_percent: Option<f64>,
    pub memory_bytes: Option<u64>,
    pub state: Option<char>,
}

#[derive(Debug, Clone, Default)]
pub struct ProcessListSnapshot {
    pub processes: Vec<ProcessSnapshot>,
}

#[derive(Debug, Clone, Copy)]
struct ProcessCounters {
    cpu_ticks: u64,
    sampled_at: Instant,
}

#[derive(Debug, Default)]
pub struct ProcessCollector {
    previous: HashMap<u32, ProcessCounters>,
    clock_ticks: f64,
    logical_cpus: f64,
}

impl ProcessCollector {
    pub fn sample(&mut self) -> ProcessListSnapshot {
        if self.clock_ticks == 0.0 {
            self.clock_ticks = unsafe { libc::sysconf(libc::_SC_CLK_TCK) as f64 };
        }
        if self.logical_cpus == 0.0 {
            self.logical_cpus = fs::read_to_string("/sys/devices/system/cpu/online")
                .ok().and_then(|value| parse_cpu_count(&value)).unwrap_or(1) as f64;
        }
        let now = Instant::now();
        let Ok(entries) = fs::read_dir("/proc") else { return ProcessListSnapshot::default() };
        let mut next = HashMap::new();
        let mut processes = Vec::new();
        for entry in entries.flatten() {
            let Some(pid) = entry.file_name().to_str().and_then(|value| value.parse::<u32>().ok()) else { continue };
            let Ok(stat) = fs::read_to_string(entry.path().join("stat")) else { continue };
            let Some(parsed) = parse_process_stat(&stat) else { continue };
            let cpu_percent = self.previous.get(&pid).and_then(|old| {
                let delta = parsed.cpu_ticks.checked_sub(old.cpu_ticks)?;
                let elapsed = now.duration_since(old.sampled_at).as_secs_f64();
                (elapsed > 0.0 && self.clock_ticks > 0.0).then_some(
                    (delta as f64 / self.clock_ticks / elapsed * 100.0).min(100.0 * self.logical_cpus)
                )
            });
            next.insert(pid, ProcessCounters { cpu_ticks: parsed.cpu_ticks, sampled_at: now });
            processes.push(ProcessSnapshot {
                pid, name: parsed.name, cpu_percent,
                memory_bytes: parsed.rss_pages.checked_mul(page_size()),
                state: Some(parsed.state),
            });
        }
        self.previous = next;
        processes.sort_by(|a,b| b.cpu_percent.unwrap_or(0.0).total_cmp(&a.cpu_percent.unwrap_or(0.0))
            .then_with(|| b.memory_bytes.cmp(&a.memory_bytes)));
        processes.truncate(30);
        ProcessListSnapshot { processes }
    }
}

struct ParsedProcess {
    name: String,
    state: char,
    cpu_ticks: u64,
    rss_pages: u64,
}

fn parse_process_stat(contents: &str) -> Option<ParsedProcess> {
    let open = contents.find('(')?;
    let close = contents.rfind(')')?;
    if close <= open { return None; }
    let name = contents[open + 1..close].to_owned();
    let fields = contents[close + 1..].split_whitespace().collect::<Vec<_>>();
    if fields.len() < 22 { return None; }
    let state = fields[0].chars().next()?;
    let user = fields[10].parse::<u64>().ok()?;
    let system = fields[11].parse::<u64>().ok()?;
    let rss_pages = fields[21].parse::<i64>().ok()?.max(0) as u64;
    Some(ParsedProcess { name, state, cpu_ticks: user.checked_add(system)?, rss_pages })
}

fn page_size() -> u64 {
    let size = unsafe { libc::sysconf(libc::_SC_PAGESIZE) };
    if size > 0 { size as u64 } else { 4096 }
}

fn parse_cpu_count(contents: &str) -> Option<u32> {
    let mut count = 0u32;
    for part in contents.trim().split(',') {
        let (start,end) = match part.split_once('-') {
            Some((a,b)) => (a.parse::<u32>().ok()?,b.parse::<u32>().ok()?),
            None => { let id=part.parse::<u32>().ok()?; (id,id) }
        };
        if start > end { return None; }
        count = count.checked_add(end - start + 1)?;
    }
    (count > 0).then_some(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn parses_process_stat_with_spaces_and_parentheses_in_name() {
        let stat = "42 (worker (render)) R 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19 20 21 22 23 24";
        let parsed = parse_process_stat(stat).unwrap();
        assert_eq!(parsed.name, "worker (render)");
        assert_eq!(parsed.cpu_ticks, 25);
    }

    #[test]
    fn parses_cpu_ranges() {
        assert_eq!(parse_cpu_count("0-3,5"), Some(5));
        assert_eq!(parse_cpu_count("3-1"), None);
    }
}
