use std::thread;
use std::sync::mpsc::{self, TryRecvError};
use mio::{Events, Poll, Ready, PollOpt, Token};
use mio::net::TcpStream;
use std::net::{TcpListener, SocketAddr};

use rppal::gpio::Gpio;

use super::Args;

pub fn main(args : Args) {
    let (tx, rx) = mpsc::channel();

    let child = thread::spawn(move || {
        let gpio = Gpio::new().unwrap();
        let mut pin = gpio.get(5).unwrap().into_output();
        let mut timer = adi_clock::Timer::new(0.005);
	pin.set_reset_on_drop(false);

        loop {
            match rx.try_recv() {
                Ok(_) | Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }
            timer.wait();
            pin.toggle();
        }
	pin.set_high();
    });

    let net_th = thread::spawn(move || {
        let addr: SocketAddr = "0.0.0.0:10000".parse().unwrap();
        let server = TcpListener::bind(&addr).unwrap();
        let poll = Poll::new().unwrap();
        let mut events = Events::with_capacity(1024);
        let stream = TcpStream::connect(&server.local_addr().unwrap()).unwrap();

        poll.register(&stream, Token(0), Ready::readable() | Ready::writable(), PollOpt::edge()).unwrap();

        loop {
            poll.poll(&mut events, None).unwrap();

            for event in &events {
                if event.token() == Token(0) && event.readiness().is_writable() {

                    
                    break;
                }
            }
        }
    });


    thread::sleep(std::time::Duration::new(1, 0));
    tx.send(()).unwrap();
    net_th.join().unwrap();
    child.join().unwrap();
}
