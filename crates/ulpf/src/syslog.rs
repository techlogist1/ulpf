//! Live syslog listeners for `serve`: UDP (one datagram is one event) and TCP (RFC 6587
//! octet counting when a connection starts with `digits SP`, else the file line rule).
//! Every event is appended to the raw store byte for byte before anything else, under the
//! same per-batch lock and sequence as file ingest, and flows through the same bounded
//! queue with the same block-on-full policy. Sources are `udp/<peer ip>` and `tcp/<peer
//! ip>`, so per-source stats, drift and inference work per sending device.

use std::collections::HashMap;
use std::io::{ErrorKind, Read};
use std::net::{IpAddr, SocketAddr, TcpListener, TcpStream, UdpSocket};
use std::sync::Arc;
use std::sync::atomic::{AtomicI64, AtomicUsize, Ordering::Relaxed};
use std::sync::mpsc::SyncSender;
use std::time::{Duration, Instant};

use anyhow::{Context as _, Result};
use ulpf_store::{Framer, RawId};

use crate::engine::{Backing, Batch, FileCtx, Live, now_nanos, send_batch};

/// Datagrams or lines are handed over when this many events, this many bytes, or this
/// much time has accumulated for one peer.
const FLUSH_EVENTS: usize = 1024;
const FLUSH_BYTES: usize = 1 << 20;
const FLUSH_AFTER: Duration = Duration::from_millis(50);
const MAX_DATAGRAM: usize = 65_536;
const MAX_TCP_CONNECTIONS: usize = 256;
/// Asked for; the kernel clamps to its own maximum. Bursts past the queue's capacity land
/// here first, so this is the buffer the soak measures against.
const RECV_BUFFER_BYTES: usize = 8 << 20;

struct PeerBuf {
    buf: Vec<u8>,
    ranges: Vec<std::ops::Range<usize>>,
    receipts: Vec<i64>,
    first_at: Instant,
    source_id: Option<u32>,
    name: String,
}

impl PeerBuf {
    fn new(name: String) -> PeerBuf {
        PeerBuf { buf: Vec::with_capacity(FLUSH_BYTES / 4), ranges: Vec::with_capacity(FLUSH_EVENTS), receipts: Vec::with_capacity(FLUSH_EVENTS), first_at: Instant::now(), source_id: None, name }
    }

    fn push(&mut self, bytes: &[u8]) {
        if self.ranges.is_empty() {
            self.first_at = Instant::now();
        }
        let start = self.buf.len();
        self.buf.extend_from_slice(bytes);
        self.ranges.push(start..self.buf.len());
        self.receipts.push(now_nanos());
    }

    fn due(&self) -> bool {
        !self.ranges.is_empty() && (self.ranges.len() >= FLUSH_EVENTS || self.buf.len() >= FLUSH_BYTES || self.first_at.elapsed() >= FLUSH_AFTER)
    }

    /// Appends every buffered event to the store (one lock), then sends one batch. The
    /// buffer moves into the batch; a fresh one takes its place.
    fn flush(&mut self, live: &Live, tx: &SyncSender<Batch>, in_flight: &AtomicI64) -> Result<()> {
        if self.ranges.is_empty() {
            return Ok(());
        }
        let count = self.ranges.len() as u64;
        let bytes = self.buf.len() as u64;
        let started = self.receipts.first().copied().unwrap_or_else(now_nanos);
        let (first, seq) = {
            let mut store = live.store.lock().unwrap_or_else(|e| e.into_inner());
            let source = match self.source_id {
                Some(s) => s,
                None => {
                    let s = store.source_id(&self.name)?;
                    self.source_id = Some(s);
                    s
                }
            };
            let first = store.len();
            for (r, receipt) in self.ranges.iter().zip(&self.receipts) {
                store.append(source, *receipt, &self.buf[r.clone()]).context("raw store append failed; aborting to avoid an incomplete store")?;
            }
            store.flush(false).context("raw store flush failed")?;
            store.record_ingest(source, Some(RawId(first)), count, bytes, started)?;
            (first, live.seq.fetch_add(1, Relaxed))
        };
        live.metrics.framed.fetch_add(count, Relaxed);
        live.metrics.stored.fetch_add(count, Relaxed);
        live.metrics.bytes.fetch_add(bytes, Relaxed);
        let buf = std::mem::replace(&mut self.buf, Vec::with_capacity(FLUSH_BYTES / 4));
        let ranges = std::mem::replace(&mut self.ranges, Vec::with_capacity(FLUSH_EVENTS));
        let receipts = std::mem::replace(&mut self.receipts, Vec::with_capacity(FLUSH_EVENTS));
        let ctx = Arc::new(FileCtx { backing: Backing::Owned(buf), name: self.name.clone(), names: HashMap::new() });
        send_batch(tx, &live.metrics, in_flight, live.queue_cap, seq, &ctx, 0, first, ranges, receipts)
    }
}

fn set_recv_buffer(fd: std::os::fd::RawFd) {
    let size: libc::c_int = RECV_BUFFER_BYTES as libc::c_int;
    // SAFETY: a plain setsockopt on a socket this process owns; the kernel validates the value.
    unsafe {
        libc::setsockopt(fd, libc::SOL_SOCKET, libc::SO_RCVBUF, (&size as *const libc::c_int).cast(), std::mem::size_of::<libc::c_int>() as libc::socklen_t);
    }
}

/// The UDP listener thread: one datagram is one event, batched per peer.
pub(crate) fn udp_listener(live: &Arc<Live>, addr: SocketAddr, tx: SyncSender<Batch>, in_flight: &AtomicI64) -> Result<()> {
    use std::os::fd::AsRawFd;
    let sock = UdpSocket::bind(addr).with_context(|| format!("binding syslog udp {addr}"))?;
    set_recv_buffer(sock.as_raw_fd());
    sock.set_read_timeout(Some(FLUSH_AFTER))?;
    let bound = sock.local_addr()?;
    live.syslog_bound.lock().unwrap_or_else(|e| e.into_inner()).0 = Some(bound);
    live.metrics.files.fetch_add(1, Relaxed);
    let mut peers: HashMap<IpAddr, PeerBuf> = HashMap::new();
    let mut buf = vec![0u8; MAX_DATAGRAM];
    while !live.stopped() {
        match sock.recv_from(&mut buf) {
            Ok((n, peer)) => {
                live.metrics.syslog_udp_datagrams.fetch_add(1, Relaxed);
                live.metrics.syslog_udp_bytes.fetch_add(n as u64, Relaxed);
                let ip = peer.ip();
                let pb = peers.entry(ip).or_insert_with(|| PeerBuf::new(format!("udp/{ip}")));
                pb.push(&buf[..n]);
                if pb.due() {
                    pb.flush(live, &tx, in_flight)?;
                }
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted) => {}
            Err(e) => {
                live.metrics.syslog_errors.fetch_add(1, Relaxed);
                eprintln!("ulpf: syslog udp: {e}");
            }
        }
        for pb in peers.values_mut() {
            if pb.due() {
                pb.flush(live, &tx, in_flight)?;
            }
        }
    }
    for pb in peers.values_mut() {
        pb.flush(live, &tx, in_flight)?;
    }
    Ok(())
}

/// The TCP acceptor thread: one thread per connection, capped; connections beyond the
/// cap are closed at once and counted.
pub(crate) fn tcp_listener(live: &Arc<Live>, addr: SocketAddr, tx: SyncSender<Batch>, in_flight: &Arc<AtomicI64>) -> Result<()> {
    let listener = TcpListener::bind(addr).with_context(|| format!("binding syslog tcp {addr}"))?;
    listener.set_nonblocking(true)?;
    let bound = listener.local_addr()?;
    live.syslog_bound.lock().unwrap_or_else(|e| e.into_inner()).1 = Some(bound);
    live.metrics.files.fetch_add(1, Relaxed);
    let open = Arc::new(AtomicUsize::new(0));
    let mut handles: Vec<std::thread::JoinHandle<()>> = Vec::new();
    while !live.stopped() {
        match listener.accept() {
            Ok((stream, peer)) => {
                if open.load(Relaxed) >= MAX_TCP_CONNECTIONS {
                    live.metrics.syslog_tcp_refused.fetch_add(1, Relaxed);
                    drop(stream);
                    continue;
                }
                open.fetch_add(1, Relaxed);
                live.metrics.syslog_tcp_connections.fetch_add(1, Relaxed);
                let live = Arc::clone(live);
                let tx = tx.clone();
                let in_flight = Arc::clone(in_flight);
                let open = Arc::clone(&open);
                handles.push(std::thread::spawn(move || {
                    if let Err(e) = tcp_connection(&live, stream, peer, &tx, &in_flight) {
                        live.metrics.syslog_errors.fetch_add(1, Relaxed);
                        eprintln!("ulpf: syslog tcp {peer}: {e}");
                    }
                    open.fetch_sub(1, Relaxed);
                }));
                handles.retain(|h| !h.is_finished());
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => std::thread::sleep(FLUSH_AFTER),
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(e) => {
                live.metrics.syslog_errors.fetch_add(1, Relaxed);
                eprintln!("ulpf: syslog tcp accept: {e}");
                std::thread::sleep(FLUSH_AFTER);
            }
        }
    }
    for h in handles {
        let _ = h.join();
    }
    Ok(())
}

/// One connection. Framing is decided by the first bytes: `[1-9][0-9]* ` is RFC 6587
/// octet counting; anything else is the line rule with terminators kept inside events.
fn tcp_connection(live: &Arc<Live>, mut stream: TcpStream, peer: SocketAddr, tx: &SyncSender<Batch>, in_flight: &AtomicI64) -> Result<()> {
    stream.set_read_timeout(Some(FLUSH_AFTER))?;
    let mut pb = PeerBuf::new(format!("tcp/{}", peer.ip()));
    let mut pending: Vec<u8> = Vec::with_capacity(1 << 16);
    let mut chunk = vec![0u8; 1 << 16];
    let mut octet_counting: Option<bool> = None;
    let mut closed = false;
    while !closed && !live.stopped() {
        match stream.read(&mut chunk) {
            Ok(0) => closed = true,
            Ok(n) => {
                live.metrics.syslog_tcp_bytes.fetch_add(n as u64, Relaxed);
                pending.extend_from_slice(&chunk[..n]);
                if octet_counting.is_none() {
                    octet_counting = Some(looks_octet_counted(&pending));
                }
                let consumed = if octet_counting == Some(true) { take_octet_counted(&pending, &mut pb, live) } else { take_lines(&pending, &mut pb, live) };
                pending.drain(..consumed);
            }
            Err(e) if matches!(e.kind(), ErrorKind::WouldBlock | ErrorKind::TimedOut | ErrorKind::Interrupted) => {}
            Err(e) => {
                closed = true;
                if e.kind() != ErrorKind::ConnectionReset {
                    eprintln!("ulpf: syslog tcp {peer}: {e}");
                }
            }
        }
        if pb.due() {
            pb.flush(live, tx, in_flight)?;
        }
    }
    if !pending.is_empty() {
        // the connection is over, so what is left is final input: complete lines the
        // continuation rule was still holding become events; an unterminated tail (or an
        // incomplete octet-counted frame) is one event too, counted as partial
        if octet_counting == Some(true) {
            live.metrics.syslog_tcp_partial.fetch_add(1, Relaxed);
            live.metrics.syslog_tcp_events.fetch_add(1, Relaxed);
            pb.push(&pending);
        } else {
            for r in Framer::new(&pending, true) {
                let ev = &pending[r];
                if !ev.ends_with(b"\n") {
                    live.metrics.syslog_tcp_partial.fetch_add(1, Relaxed);
                }
                live.metrics.syslog_tcp_events.fetch_add(1, Relaxed);
                pb.push(ev);
            }
        }
    }
    pb.flush(live, tx, in_flight)
}

fn looks_octet_counted(b: &[u8]) -> bool {
    let mut i = 0;
    while i < b.len() && b[i].is_ascii_digit() && i < 10 {
        i += 1;
    }
    i > 0 && b[0] != b'0' && b.get(i) == Some(&b' ')
}

/// Consumes complete `LEN SP MSG` frames; returns how many bytes were consumed.
fn take_octet_counted(b: &[u8], pb: &mut PeerBuf, live: &Live) -> usize {
    let mut pos = 0;
    loop {
        let rest = &b[pos..];
        let Some(sp) = rest.iter().take(11).position(|c| *c == b' ') else { return pos };
        let Ok(len) = std::str::from_utf8(&rest[..sp]).ok().and_then(|s| s.parse::<usize>().ok()).ok_or(()) else {
            // not a length: fall back to the line rule for the remainder
            return pos + take_lines(rest, pb, live);
        };
        let start = sp + 1;
        if rest.len() < start + len {
            return pos;
        }
        pb.push(&rest[start..start + len]);
        live.metrics.syslog_tcp_events.fetch_add(1, Relaxed);
        pos += start + len;
    }
}

/// Consumes complete events by the file line rule (a final unterminated line waits).
fn take_lines(b: &[u8], pb: &mut PeerBuf, live: &Live) -> usize {
    let mut consumed = 0;
    for r in Framer::new(b, false) {
        pb.push(&b[r.clone()]);
        live.metrics.syslog_tcp_events.fetch_add(1, Relaxed);
        consumed = r.end;
    }
    consumed
}
