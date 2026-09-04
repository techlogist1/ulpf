use std::fs;
use std::io::{BufWriter, Write, BufRead, BufReader};
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
/// Keep mutations minimal to preserve parser detection.
fn mutate_event(event: &str, rng: &mut Rng, _time_offset_secs: u64) -> String {
    let mut result = event.to_string();

    // Replace IPv4 addresses carefully
    result = replace_ipv4(&result, rng);

    // Replace specific numeric fields: srcport=, dstport=, sessionid=, port=
    result = replace_keyword_numbers(&result, rng);

    result
}

/// Replace IPv4 addresses with random ones
fn replace_ipv4(event: &str, rng: &mut Rng) -> String {
    let mut result = String::new();
    let bytes = event.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if (bytes[i] as char).is_ascii_digit() {
            let start = i;
            let mut num_parts = Vec::new();
            let mut valid_ip = true;

            // Try to parse X.Y.Z.W pattern
            for _ in 0..4 {
                let part_start = i;
                while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                    i += 1;
                }

                if i == part_start {
                    valid_ip = false;
                    break;
                }

                let part_str = std::str::from_utf8(&bytes[part_start..i]).unwrap_or("");
                if let Ok(num) = part_str.parse::<u16>() {
                    if num <= 255 {
                        num_parts.push(num);
                    } else {
                        valid_ip = false;
                        break;
                    }
                } else {
                    valid_ip = false;
                    break;
                }

                // Check for dot separator (except after last octet)
                if num_parts.len() < 4 {
                    if i < bytes.len() && bytes[i] == b'.' {
                        i += 1;
                    } else {
                        valid_ip = false;
                        break;
                    }
                }
            }

            if valid_ip && num_parts.len() == 4 {
                let new_ip = generate_random_ip(rng);
                result.push_str(&new_ip);
            } else {
                result.push_str(&event[start..i]);
            }
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
}

/// Replace numbers after specific keywords: srcport=, dstport=, sessionid=, connid=, etc.
fn replace_keyword_numbers(event: &str, rng: &mut Rng) -> String {
    let keywords = [
        "srcport=", "dstport=", "port=",
        "sessionid=", "connid=", "connection ",
        "srcport:", "dstport:",
    ];

    let mut result = event.to_string();

    for keyword in &keywords {
        let keyword_bytes = keyword.as_bytes();
        let mut new_result = String::new();
        let bytes = result.as_bytes();
        let mut i = 0;

        while i < bytes.len() {
            if i + keyword_bytes.len() <= bytes.len() && &bytes[i..i + keyword_bytes.len()] == keyword_bytes {
                new_result.push_str(keyword);
                i += keyword_bytes.len();

                let num_start = i;
                while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                    i += 1;
                }

                if i > num_start {
                    let num_str = std::str::from_utf8(&bytes[num_start..i]).unwrap_or("");
                    if let Ok(_num) = num_str.parse::<u64>() {
                        let new_id = rng.next();
                        new_result.push_str(&new_id.to_string());
                    } else {
                        new_result.push_str(num_str);
                    }
                }
            } else {
                new_result.push(bytes[i] as char);
                i += 1;
            }
        }

        result = new_result;
    }

    result
}

fn is_valid_ipv4(s: &str) -> bool {
    let parts: Vec<&str> = s.split('.').collect();
    if parts.len() != 4 {
        return false;
    }
    parts.iter().all(|p| {
        p.parse::<u8>().is_ok()
    })
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

fn advance_timestamps(event: &str, secs: u64) -> String {
    let mut result = String::new();
    let bytes = event.as_bytes();
    let mut i = 0;

    while i < bytes.len() {
        if (bytes[i] as char).is_ascii_digit() {
            let start = i;
            while i < bytes.len() && (bytes[i] as char).is_ascii_digit() {
                i += 1;
            }

            let num_str = std::str::from_utf8(&bytes[start..i]).unwrap_or("");
            if num_str.len() >= 10 && num_str.len() <= 19 {
                if let Ok(ts) = num_str.parse::<u64>() {
                    // Epoch timestamp; add secs (treating as seconds, adjust for scale)
                    // For nanoseconds (19 digits), add secs * 1e9
                    // For milliseconds (13 digits), add secs * 1e3
                    // For seconds (10 digits), add secs directly
                    let new_ts = match num_str.len() {
                        19 => ts + secs * 1_000_000_000,
                        13 => ts + secs * 1_000,
                        _ => ts + secs,
                    };
                    result.push_str(&new_ts.to_string());
                    continue;
                }
            }

            result.push_str(num_str);
        } else {
            result.push(bytes[i] as char);
            i += 1;
        }
    }

    result
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
                    // Truncate line
                    let truncate_at = rng.range(mutated.len().max(1)) + 1;
                    let truncated = &mutated[..truncate_at.min(mutated.len())];
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
