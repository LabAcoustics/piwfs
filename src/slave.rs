use std::thread;
use std::time::Duration;

use rppal::gpio::{Gpio, Trigger};


pub fn main() {
    let gpio = Gpio::new().unwrap();
    let mut pin = gpio.get(4).unwrap().into_input_pulldown();

    pin.set_async_interrupt(Trigger::RisingEdge, |level| {
        println!("Interrupted! Level: {}", level);
    }).unwrap();

    thread::sleep(Duration::from_secs(10));
}
