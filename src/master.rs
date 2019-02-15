use std::thread;
use std::sync::{Arc, Mutex};

use rppal::gpio::Gpio;

pub fn main() {
    let timer = timer::Timer::new();
    let gpio = Gpio::new().unwrap();
    let pin = Arc::new(Mutex::new(gpio.get(4).unwrap().into_output()));

    let guard = {
        let pin = pin.clone();
        timer.schedule_repeating(chrono::Duration::milliseconds(5), move || {
            pin.lock().unwrap().toggle();
        })
    };

    thread::sleep(std::time::Duration::new(1, 0));
    drop(guard);
}
