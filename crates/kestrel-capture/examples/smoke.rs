//! Manual smoke test: `cargo run -p kestrel-capture --example smoke`
//! Verifies the platform backend and, on macOS, that screen recording
//! permission has actually been granted to the running binary.

use kestrel_capture::{backend, CaptureBackend};

fn main() {
    let b = backend();
    println!("capabilities: {:?}", b.capabilities());

    match b.displays() {
        Ok(d) => {
            println!("displays: {}", d.len());
            for x in &d {
                println!(
                    "  {} {:?} scale={} primary={}",
                    x.name, x.region, x.scale_factor, x.is_primary
                );
            }
        }
        Err(e) => println!("displays FAILED: {e}"),
    }

    match b.windows() {
        Ok(w) => {
            println!("windows: {}", w.len());
            for x in w.iter().take(8) {
                println!(
                    "  id={} app={:?} title={:?} {:?}",
                    x.id, x.app_name, x.title, x.region
                );
            }
        }
        Err(e) => println!("windows FAILED: {e}"),
    }

    match b.capture_all_displays() {
        Ok(c) => println!("capture_all: {}x{}", c.width(), c.height()),
        Err(e) => println!("capture_all FAILED: {e}"),
    }
}
