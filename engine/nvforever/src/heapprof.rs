//! A sampling heap profiler for the TrimUI build (feature `heapprof`, aarch64 Linux only).
//!
//! Every allocation of at least `SAMPLE` bytes, and one allocation in each `SAMPLE` bytes
//! of smaller ones, is remembered with its call stack (frame pointers: build with
//! `-C force-frame-pointers=yes`). Freed ones are forgotten. Every `NVF_HEAPPROF_SECS`
//! (default 30) the live samples are written to `NVF_HEAPPROF_OUT` (default
//! /tmp/nvf-heap.txt): an estimated byte count per stack, with the return addresses
//! relative to the program's load address, for symbolizing against the unstripped binary.

use std::alloc::{GlobalAlloc, Layout, System};
use std::cell::Cell;
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};

const SAMPLE: usize = 64 * 1024;
const DEPTH: usize = 20;
const SLOTS: usize = 1 << 17;

#[derive(Clone, Copy)]
struct Slot {
    ptr: usize,
    weight: usize,
    depth: u8,
    stack: [usize; DEPTH],
}

const EMPTY: Slot = Slot { ptr: 0, weight: 0, depth: 0, stack: [0; DEPTH] };
const TOMB: usize = 1;

static mut TABLE: [Slot; SLOTS] = [EMPTY; SLOTS];
static mut COPY: [Slot; SLOTS] = [EMPTY; SLOTS];
static LOCK: AtomicBool = AtomicBool::new(false);
static LIVE: AtomicUsize = AtomicUsize::new(0);
static DROPPED: AtomicUsize = AtomicUsize::new(0);

thread_local! {
    static COUNTDOWN: Cell<isize> = const { Cell::new(SAMPLE as isize) };
    static BOUNDS: Cell<(usize, usize)> = const { Cell::new((0, 0)) };
}

fn lock() {
    while LOCK.compare_exchange_weak(false, true, Ordering::Acquire, Ordering::Relaxed).is_err() {
        std::hint::spin_loop();
    }
}
fn unlock() {
    LOCK.store(false, Ordering::Release);
}

fn hash(p: usize) -> usize {
    (p >> 4).wrapping_mul(0x9E37_79B9_7F4A_7C15) >> (64 - 17)
}

/// This thread's stack, from glibc (its own malloc, not this allocator).
fn bounds() -> (usize, usize) {
    let b = BOUNDS.with(|c| c.get());
    if b.1 != 0 {
        return b;
    }
    let mut out = (0usize, usize::MAX);
    unsafe {
        let mut attr: libc::pthread_attr_t = std::mem::zeroed();
        if libc::pthread_getattr_np(libc::pthread_self(), &mut attr) == 0 {
            let mut addr: *mut libc::c_void = std::ptr::null_mut();
            let mut size: libc::size_t = 0;
            if libc::pthread_attr_getstack(&attr, &mut addr, &mut size) == 0 {
                out = (addr as usize, addr as usize + size);
            }
            libc::pthread_attr_destroy(&mut attr);
        }
    }
    if out.1 == usize::MAX {
        out = (1, 2); // unknown: no walk
    }
    BOUNDS.with(|c| c.set(out));
    out
}

#[inline(always)]
fn walk(stack: &mut [usize; DEPTH]) -> u8 {
    let (lo, hi) = bounds();
    let mut fp: usize;
    unsafe { core::arch::asm!("mov {}, x29", out(reg) fp) };
    let mut n = 0;
    while n < DEPTH {
        if fp < lo || fp + 16 > hi || fp & 7 != 0 {
            break;
        }
        let (next, lr) = unsafe { (*(fp as *const usize), *((fp + 8) as *const usize)) };
        if lr < 4096 {
            break;
        }
        stack[n] = lr;
        n += 1;
        if next <= fp {
            break;
        }
        fp = next;
    }
    n as u8
}

fn record(ptr: usize, weight: usize) {
    let mut s = Slot { ptr, weight, depth: 0, stack: [0; DEPTH] };
    s.depth = walk(&mut s.stack);
    lock();
    unsafe {
        let t = &mut *std::ptr::addr_of_mut!(TABLE);
        let mut i = hash(ptr);
        let mut placed = false;
        for _ in 0..SLOTS {
            if t[i].ptr == 0 || t[i].ptr == TOMB {
                t[i] = s;
                placed = true;
                break;
            }
            i = (i + 1) & (SLOTS - 1);
        }
        if placed {
            LIVE.fetch_add(1, Ordering::Relaxed);
        } else {
            DROPPED.fetch_add(1, Ordering::Relaxed);
        }
    }
    unlock();
}

fn forget(ptr: usize) {
    if LIVE.load(Ordering::Relaxed) == 0 {
        return;
    }
    lock();
    unsafe {
        let t = &mut *std::ptr::addr_of_mut!(TABLE);
        let mut i = hash(ptr);
        for _ in 0..SLOTS {
            if t[i].ptr == ptr {
                t[i].ptr = TOMB;
                LIVE.fetch_sub(1, Ordering::Relaxed);
                break;
            }
            if t[i].ptr == 0 {
                break;
            }
            i = (i + 1) & (SLOTS - 1);
        }
    }
    unlock();
}

/// Whether this allocation is sampled, and the bytes it stands for.
#[inline(always)]
fn sampled(size: usize) -> Option<usize> {
    if size >= SAMPLE {
        return Some(size);
    }
    COUNTDOWN.with(|c| {
        let left = c.get() - size as isize;
        if left <= 0 {
            c.set(left + SAMPLE as isize);
            Some(SAMPLE)
        } else {
            c.set(left);
            None
        }
    })
}

pub struct Profiler;

unsafe impl GlobalAlloc for Profiler {
    unsafe fn alloc(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc(l) };
        if !p.is_null() {
            if let Some(w) = sampled(l.size()) {
                record(p as usize, w);
            }
        }
        p
    }
    unsafe fn alloc_zeroed(&self, l: Layout) -> *mut u8 {
        let p = unsafe { System.alloc_zeroed(l) };
        if !p.is_null() {
            if let Some(w) = sampled(l.size()) {
                record(p as usize, w);
            }
        }
        p
    }
    unsafe fn dealloc(&self, p: *mut u8, l: Layout) {
        forget(p as usize);
        unsafe { System.dealloc(p, l) }
    }
    unsafe fn realloc(&self, p: *mut u8, l: Layout, new: usize) -> *mut u8 {
        let q = unsafe { System.realloc(p, l, new) };
        if !q.is_null() {
            forget(p as usize);
            if let Some(w) = sampled(new) {
                record(q as usize, w);
            }
        }
        q
    }
}

/// The program's load address: the first mapping of /proc/self/exe.
fn base() -> (usize, String) {
    let exe = std::fs::read_link("/proc/self/exe").map(|p| p.to_string_lossy().into_owned()).unwrap_or_default();
    let maps = std::fs::read_to_string("/proc/self/maps").unwrap_or_default();
    for line in maps.lines() {
        if line.ends_with(&exe) {
            let start = line.split('-').next().unwrap_or("0");
            return (usize::from_str_radix(start, 16).unwrap_or(0), exe);
        }
    }
    (0, exe)
}

fn dump(path: &str) {
    lock();
    unsafe {
        let t = &*std::ptr::addr_of!(TABLE);
        let c = &mut *std::ptr::addr_of_mut!(COPY);
        c.copy_from_slice(t);
    }
    unlock();
    let (b, exe) = base();
    let mut by: std::collections::HashMap<Vec<usize>, (usize, usize)> = Default::default();
    let mut total = 0usize;
    unsafe {
        for s in (*std::ptr::addr_of!(COPY)).iter() {
            if s.ptr > TOMB {
                let k: Vec<usize> = s.stack[..s.depth as usize].iter().map(|a| a.wrapping_sub(b)).collect();
                let e = by.entry(k).or_default();
                e.0 += s.weight;
                e.1 += 1;
                total += s.weight;
            }
        }
    }
    let mut v: Vec<_> = by.into_iter().collect();
    v.sort_by(|a, b| b.1 .0.cmp(&a.1 .0));
    let mut out = format!(
        "heapprof: {} MB estimated live in {} stacks; base {:#x} {}; {} samples dropped\n",
        total >> 20,
        v.len(),
        b,
        exe,
        DROPPED.load(Ordering::Relaxed)
    );
    for (k, (w, n)) in v.iter().take(400) {
        out.push_str(&format!("{} {}", w, n));
        for a in k {
            out.push_str(&format!(" {:x}", a));
        }
        out.push('\n');
    }
    let _ = std::fs::write(path, out);
    bevy::log::info!("heapprof: {} MB estimated live, written to {path}", total >> 20);
}

/// Starts the thread that writes the profile.
pub fn start() {
    let secs: u64 = std::env::var("NVF_HEAPPROF_SECS").ok().and_then(|s| s.parse().ok()).unwrap_or(30);
    let path = std::env::var("NVF_HEAPPROF_OUT").unwrap_or_else(|_| "/tmp/nvf-heap.txt".into());
    std::thread::Builder::new()
        .name("heapprof".into())
        .spawn(move || loop {
            std::thread::sleep(std::time::Duration::from_secs(secs));
            dump(&path);
        })
        .ok();
}
