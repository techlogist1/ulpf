use std::fs;
use std::io::{BufWriter, Write};
use std::path::Path;
use std::time::Instant;

/// Deterministic xorshift128 RNG with fixed seed.
struct Rng {
    x: u32,
    y: u32,
    z: u32,
    w: u32,
}

impl Rng {
    fn new() -> Self {
        // Fixed seed for reproducibility
        Rng {
            x: 123456789,
            y: 362436069,
            z: 521288629,
            w: 88675123,
        }
    }

    fn next(&mut self) -> u32 {
        let t = self.x ^ (self.x << 11);
        self.x = self.y;
        self.y = self.z;
        self.z = self.w;
        self.w = self.w ^ (self.w >> 19) ^ (t ^ (t >> 8));
        self.w
    }

    fn range(&mut self, max: usize) -> usize {
        (self.next() as usize) % max
    }
}

/// Read all sample files and collect framed events.
/// A frame is a line plus any following lines that start with space/tab.
/// Handles non-UTF-8 data by replacing invalid bytes with U+FFFD.
fn read_samples(samples_dir: &Path) -> std::io::Result<Vec<String>> {
    let mut events = Vec::new();

    if !samples_dir.exists() {
        eprintln!("Warning: samples directory not found at {:?}", samples_dir);
        return Ok(events);
    }

    for entry in fs::read_dir(samples_dir)? {
        let entry = entry?;
        let path = entry.path();

        if path.is_file() && path.extension().map(|e| e == "log").unwrap_or(false) {
            let content = fs::read(&path)?;
            let text = String::from_utf8_lossy(&content);
            let mut lines = text.lines().peekable();

            while let Some(line) = lines.next() {
                let mut event = line.to_string();

                // Collect continuation lines (starting with space or tab)
                while let Some(&next_line) = lines.peek() {
                    if next_line.starts_with(' ') || next_line.starts_with('\t') {
                        let next = lines.next().unwrap();
                        event.push('\n');
                        event.push_str(next);
                    } else {
                        break;
                    }
                }

                if !event.is_empty() {
                    events.push(event);
                }
            }
        }
    }

    Ok(events)
}

/// Mutate an event: replace IPs and specific numeric values.
/// Keep mutations minimal and fast.
fn mutate_event(event: &str, rng: &mut Rng, _time_offset_secs: u64) -> String {
    let mut result = event.to_string();

    // Replace IPv4 addresses (simple regex-like scan)
    result = replace_ipv4_simple(&result, rng);

    // Replace specific keyword patterns
    result = simple_replace_pattern(&result, "srcport=", rng);
    result = simple_replace_pattern(&result, "dstport=", rng);
    result = simple_replace_pattern(&result, "port=", rng);
    result = simple_replace_pattern(&result, "sessionid=", rng);
    result = simple_replace_pattern(&result, "connid=", rng);

    result
}

/// Fast IPv4 replacement using char iteration
fn replace_ipv4_simple(event: &str, rng: &mut Rng) -> String {
    let mut result = String::new();
    let mut chars = event.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch.is_ascii_digit() {
            let mut potential_ip = String::from(ch);

            // Peek ahead for possible IP pattern
            while let Some(&next_ch) = chars.peek() {
                if next_ch.is_ascii_digit() || next_ch == '.' {
                    potential_ip.push(chars.next().unwrap());
                } else {
                    break;
                }
            }

            // Check if it's a valid IP
            if is_ipv4(&potential_ip) {
                let new_ip = generate_random_ip(rng);
                result.push_str(&new_ip);
            } else {
                result.push_str(&potential_ip);
            }
        } else {
            result.push(ch);
        }
    }

    result
}

fn is_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|p| {
        !p.is_empty() && p.parse::<u8>().is_ok()
    })
}

/// Simple pattern replacement: find "keyword" followed by digits, replace the digits
fn simple_replace_pattern(event: &str, keyword: &str, rng: &mut Rng) -> String {
    let mut result = String::new();
    let mut remaining = event;

    while let Some(pos) = remaining.find(keyword) {
        result.push_str(&remaining[..pos + keyword.len()]);
        remaining = &remaining[pos + keyword.len()..];

        // Extract digits by character iteration
        let mut num_str = String::new();
        let mut chars_to_skip = 0;

        for ch in remaining.chars() {
            if ch.is_ascii_digit() {
                num_str.push(ch);
                chars_to_skip += ch.len_utf8();
            } else {
                break;
            }
        }

        if !num_str.is_empty() {
            if num_str.parse::<u64>().is_ok() {
                // For ports, constrain to reasonable range
                let new_val = if keyword.contains("port") {
                    ((rng.next() % 65535) as u16 + 1).to_string()
                } else {
                    rng.next().to_string()
                };
                result.push_str(&new_val);
            } else {
                result.push_str(&num_str);
            }
            remaining = &remaining[chars_to_skip..];
        }
    }

    result.push_str(remaining);
    result
}

fn generate_random_ip(rng: &mut Rng) -> String {
    let octets = [
        (rng.next() % 256) as u8,
        (rng.next() % 256) as u8,
        (rng.next() % 256) as u8,
        (rng.next() % 256) as u8,
    ];
    format!("{}.{}.{}.{}", octets[0], octets[1], octets[2], octets[3])
}

fn main() -> std::io::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let lines_target = if args.len() > 1 {
        args[1].parse::<usize>().unwrap_or(5_000_000)
    } else {
        5_000_000
    };

    let out_dir = if args.len() > 2 {
        &args[2]
    } else {
        "bench"
    };

    fs::create_dir_all(out_dir)?;

    let start = Instant::now();

    // Find samples directory relative to this executable
    let samples_dir = Path::new("samples");
    let events = read_samples(samples_dir)?;

    if events.is_empty() {
        eprintln!("No sample events found. Checked: {:?}", samples_dir);
        return Ok(());
    }

    eprintln!("Loaded {} sample events", events.len());

    let mut rng = Rng::new();
    let output_path = format!("{}/mixed-{}.log", out_dir, lines_target);
    let output = fs::File::create(&output_path)?;
    let mut writer = BufWriter::with_capacity(1024 * 1024, output);

    let mut line_count = 0;
    let mut byte_count = 0u64;

    for i in 0..lines_target {
        // Select a random event
        let event_idx = rng.range(events.len());
        let event = &events[event_idx];

        // Mutate it
        let time_offset = (i as u64) / 1000; // Spread timestamps across time
        let mutated = mutate_event(event, &mut rng, time_offset);

        // Inject mess at ~0.1% rate
        let mess_chance = rng.next() % 1000;
        if mess_chance < 1 {
            match mess_chance % 4 {
                0 => {
                    // Truncate line - use character-based truncation
                    let char_len = mutated.chars().count();
                    let truncate_at = rng.range(char_len.max(1)) + 1;
                    let truncated = mutated.chars().take(truncate_at.min(char_len)).collect::<String>();
                    writer.write_all(truncated.as_bytes())?;
                    byte_count += truncated.len() as u64;
                }
                1 => {
                    // Insert non-UTF-8 byte
                    writer.write_all(mutated.as_bytes())?;
                    writer.write_all(&[0xFF])?;
                    byte_count += mutated.len() as u64 + 1;
                }
                2 => {
                    // Double a space
                    let doubled = mutated.replace(' ', "  ");
                    writer.write_all(doubled.as_bytes())?;
                    byte_count += doubled.len() as u64;
                }
                3 => {
                    // Empty line
                    writer.write_all(b"\n")?;
                    byte_count += 1;
                    continue;
                }
                _ => {
                    writer.write_all(mutated.as_bytes())?;
                    byte_count += mutated.len() as u64;
                }
            }
        } else {
            writer.write_all(mutated.as_bytes())?;
            byte_count += mutated.len() as u64;
        }

        writer.write_all(b"\n")?;
        byte_count += 1;
        line_count += 1;

        if (i + 1) % 100_000 == 0 {
            eprintln!("Progress: {}/{} lines", i + 1, lines_target);
        }
    }

    writer.flush()?;
    let elapsed = start.elapsed();

    eprintln!("Generated {} lines, {} bytes in {:?}", line_count, byte_count, elapsed);
    eprintln!("Output: {}", output_path);
    println!("{} {} {:?}", line_count, byte_count, elapsed);

    Ok(())
}
