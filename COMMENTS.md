# Opis kodu

## slave.rs

Ten plik uruchamia się na każdym slavie

### Importowanie bibliotek
```Rust
use std::sync::{Arc, Mutex, Barrier};
use std::sync::mpsc::{self, TryRecvError, Receiver};
use std::io::Write;
use std::thread;

use rppal::gpio::{Gpio, Trigger, InputPin};
use alsa::{Direction, ValueOr};
use alsa::pcm::{PCM, HwParams, Format, Access, State};
use alsa::direct::pcm::SyncPtrStatus;
use num::pow;
use ta::indicators::SimpleMovingAverage;
use ta::Next;
use hound;

use super::Args;
```

Jedyne co tu może być niejasne, to może `use super::Args`, w ten sposób importuję z pliku wyżej,
czyli w tym przypadku z `main.rs`, w którym jest definicja `Args`.

### Funkcja do zmiany pcma na deskryptor pliku
```Rust
fn pcm_to_fd(p: &PCM) -> alsa::Result<std::os::unix::io::RawFd> {
    let mut fds: [libc::pollfd; 1] = unsafe { std::mem::zeroed() };
    let c = (p as &alsa::PollDescriptors).fill(&mut fds)?;
    if c != 1 {
        return Err(alsa::Error::unsupported("snd_pcm_poll_descriptors returned wrong number of fds"))
    }
    Ok(fds[0].fd)
}
```
Ta funkcja jest tylko po to by zmienić instancję typu `PCM` na unixowy deskryptor pliku ("file descriptor"),
bo tylko coś takiego przyjmuje `SyncPtrStatus::sync_ptr` (Dlaczego? Nie mam pojęcia, wymysł autora biblioteki)

### Funkcja do synchronizacji
```Rust
fn synch_status(pin: &mut InputPin, pcm_fd: &std::os::unix::io::RawFd, sma_val: &Arc<Mutex<f64>>,
                int_time: u32, rx: &Receiver<()>, sma_num: u32, barrier: &Arc<Barrier>)
{
    // to jest po prostu średnia ruchoma
    let mut sma = SimpleMovingAverage::new(sma_num).unwrap();
    let mut first_time = true;
    // wypełniam ją zerami żeby pierwsze wartości nie były odjechane w kosmos
    for _ in 0..sma_num {
        sma.next(0f64);
    }
    // wektor w którym będe zapisywał odchyłki zegara
    let mut deviation: Vec<i32> = Vec::with_capacity((300f64/int_time as f64) as usize);
    let mut prev_time: u64 = 0;
    // właczam interrupta na zboczu wznoszącym (tak lepiej działał)
    pin.set_interrupt(Trigger::RisingEdge).unwrap();
    loop {
        // Czekam na interrupt przez 2*czas który powinien minąć pomiędzy interruptami
        match pin.poll_interrupt(true, Some(std::time::Duration::from_nanos(2*int_time as u64))) {
            Ok(None) => { // To się dzieje jeżeli upłynął czas i nie było interrupta
                prev_time = 0;
            }
            Ok(_) => { // To się dzieje jeżeli był interrupt
                // Pobieram status karty dźwiękowej
                match unsafe { SyncPtrStatus::sync_ptr(*pcm_fd, true, None, None) } {
                    Ok(status) => { // Jeżeli udało mi się zsychronizować
                        // Jeżeli to pierwszy raz to ustawia barierę
                        // Bariera zatrzymuje egzekucję wątku do czasu aż określona ilość wątków wywoła na niej metodę wait()
                        // To po to by pierwszy interrupt uruchamiał odtwarzanie
                        if first_time { first_time = false; barrier.wait(); }
                        //Dodaje sekundy i nanosekundy do jednej wartości w nano
                        let cur_time = status.htstamp().tv_sec as u64 * pow::pow(10u64,9) + status.htstamp().tv_nsec as u64;
                        if prev_time != 0 {
                            // Jeżeli to nie jest pierwszy interrupt i jeżeli ostatnio udało się go otrzymać
                            // Odchyłka to różnica pomiędzy czasem który powinien minąć pomiędzy interruptami,
                            // a czasem który rzeczywiście minął
                            let dev = int_time as i32 - (cur_time - prev_time) as i32;
                            deviation.push(dev);
                            // Wyciągam z odchyłki średnią ruchomą
                            let next_val = sma.next(dev as f64);
                            // I zapiuje tę wartość do argumentu "val" żeby można byłą ją odczytać w głównym wątku
                            if let Ok(mut val) = sma_val.try_lock() {
                                // ale tylko jeżeli ktoś nie używa tej zmiennej jednocześnie (nie wywołał na niej lock())
                                *val = next_val;
                            }
                        }
                        prev_time = cur_time;
                    }
                    Err(e) => println!("Error syncing pointer: {:?}", e),
                }
            }
            Err(_) => panic!("Error polling interrupt!")
        }
        // Próbuje odebrać wiadomość z końca kanału z argumentu i jeżeli się udało to kończę tego loopa
        match rx.try_recv() {
            Ok(_) | Err(TryRecvError::Disconnected) => break,
            Err(TryRecvError::Empty) => {}
        }
    }
    // Zapisuje odchyłki do pliku
    npy::to_file("deviation.npy", deviation).unwrap();
}
```
Ta funkcja nie jest zbyt ładna. Chciałem przenieść do niej część kodu ale
okazało się że spowodowało to więcej walki niż było zysku (z tym milionem argumentów)

### Główna funkcja

```Rust
pub fn main(args: Args) { // w args przekazane są argumenty z main.rs
    let gpio: rppal::gpio::Gpio = Gpio::new().unwrap();
    // numeracja pinów BCM
    let mut pin: InputPin = gpio.get(16).unwrap().into_input_pullup();

    // w flag_device jest nazwa urządzenia ALSA
    let pcm = PCM::new(&args.flag_device, Direction::Playback, false).unwrap();

    let (tx, rx) = mpsc::channel();

    // otwieram plik do odtwarzania
    let mut reader = hound::WavReader::open(args.flag_testfile).unwrap();
    let reader_spec = reader.spec();

    let fs = reader_spec.sample_rate;
    let num_channels = reader_spec.channels as u32;
    let int_time: u32 = 2 * 5 * pow(10, 6);

    // Arci pozwalają kopiować referencje
    let sma_val = Arc::new(Mutex::new(0f64));
    let barrier = Arc::new(Barrier::new(2));
    // Nowy wątek, który odpowiada za synchronizację
    let sync_thr = {
        let pcm_fd = pcm_to_fd(&pcm).unwrap();
        let sma_val = sma_val.clone();
        let barrier = barrier.clone();
        thread::spawn(move || {
            synch_status(&mut pin, &pcm_fd, &sma_val, int_time, &rx, 1000, &barrier)
        })
    };

    //ustawianie parametrów karty dźwiękowej
    let hwp = HwParams::any(&pcm).unwrap();
    hwp.set_channels(num_channels).unwrap();
    hwp.set_rate(fs, ValueOr::Nearest).unwrap();
    hwp.set_format(Format::s16()).unwrap();
    // interleaved oznacza że próbki kanałów są naprzemiennie
    hwp.set_access(Access::RWInterleaved).unwrap();
    pcm.hw_params(&hwp).unwrap();
    let io = pcm.io_i16().unwrap();

    let hwp = pcm.hw_params_current().unwrap();
    let swp = pcm.sw_params_current().unwrap();
    // period to część całego bufora która jest odtwarzana na przemian z resztą periodów w buforze
    // w ten sposób wpisuje się dane do perioda który nie jest odtwarzany
    let period_size = hwp.get_period_size().unwrap();
    // cału bufor, zazwyczaj 2*period
    let buffer_size = hwp.get_buffer_size().unwrap();
    swp.set_start_threshold(period_size - buffer_size).unwrap();
    swp.set_tstamp_mode(true).unwrap();
    pcm.sw_params(&swp).unwrap();

    let sam_num = period_size as usize * num_channels as usize;
    let mut first_time = true;
    loop {
        let samples = reader.samples::<i16>();

        if samples.len() == 0 { break; }
        let mut buf: Vec<i16> = Vec::with_capacity(sam_num);

        // pobieram z pliku tyle sampli ile się mieści w periodzie
        for sample in samples {
            buf.push(sample.unwrap());
            if buf.len() >= sam_num {
                break;
            }
        }

        if first_time {
            first_time = false;
            // za pierwszym razem czekam na pierwszy interrupt
            barrier.wait();
            // sprawdzam czy udało się wpisać wszystkie próbki do bufora
            assert_eq!(io.writei(&buf[..]).unwrap(), buf.len()/num_channels as usize);
            if pcm.state() != State::Running { pcm.start().unwrap() };
        } else {
            assert_eq!(io.writei(&buf[..]).unwrap(), buf.len()/num_channels as usize);
            let dev = *sma_val.lock().unwrap() as i32;
            // docelowow tu będzie jakaś synchronizacja
            if args.flag_verbose { // jeżeli da się switch --verbose, to wyświetlanie jest odchylenie
                print!("Deviation: {} ns \r", dev);
                std::io::stdout().flush().unwrap();
            }
        }
    }

    // czekam aż skończy grać
    pcm.drain().unwrap();
    // wysyłam pustą wiadomośc do wątku synchronizującego żeby się skończył
    tx.send(()).unwrap();
    // czekam aż się skończy
    sync_thr.join().unwrap();
}
```

Tu się niewiele dzieje, tak napradę tylko ustawiana jest karta dźwiękowa i odtwarzany jest plik w momencie dostania interruptu

## master.rs

Pominę importowanie

### Główna funkcja
```Rust
pub fn main(args : Args) {
    let (tx, rx) = mpsc::channel();

    // czas przez który zegar działa ustawia się switchem --time
    let wait_time: u64 = args.flag_time;
    let sma_num = 1000;
    let int_time: f32 = 0.01;

    // wątek który wysyła zegar
    let child = thread::spawn(move || {
        let gpio = Gpio::new().unwrap();
        let mut pin = gpio.get(16).unwrap().into_output();
        // średnia podobnie jak w slavie
        let mut sma = SimpleMovingAverage::new(sma_num).unwrap();
        let mut prev_time = 0f32;
        for _ in 0..sma_num {
            sma.next(0f64);
        }

        pin.set_low();
        let mut high = false;
        let mut deviation: Vec<i32> = Vec::with_capacity((wait_time as f32/int_time) as usize);

        // ustawiam czas wyzwalania zegara
        let mut timer = adi_clock::Timer::new(int_time/2.0);
        loop {
            // tak jak w slave jeśli wiadomość to koniec pętli
            match rx.try_recv() {
                Ok(_) | Err(TryRecvError::Disconnected) => break,
                Err(TryRecvError::Empty) => {}
            }
            // czekam na wyzwolenie, zwracany jest czas w którym nastąpiło
            let cur_time = timer.wait();
            // przłączam stan pinu na przeciwny
            pin.toggle();
            high = !high;
            if high { // tu wszystko jak w slavie, wyliczana odchyłka
                if prev_time != 0f32 {
                    let dev = pow(10.0, 9)*(int_time - (cur_time - prev_time));
                    deviation.push(dev as i32);
                    if args.flag_verbose {
                        print!("Deviation: {} ns \r", sma.next(dev as f64) as i32);
                        std::io::stdout().flush().unwrap();
                    }
                }
                prev_time = cur_time;
            }
        }
        npy::to_file("deviation.npy", deviation).unwrap();
    });

    // wątek sieciowy, który nie działa jak na razie
    let net_th = thread::spawn(move || {
        let addr: SocketAddr = "0.0.0.0:10000".parse().unwrap();
        let server = TcpListener::bind(&addr).unwrap();
        let poll = Poll::new().unwrap();
        let mut events = Events::with_capacity(1024);
        let stream = TcpStream::connect(&server.local_addr().unwrap()).unwrap();

        poll.register(&stream, Token(0), Ready::readable() | Ready::writable(), PollOpt::edge()).unwrap();

        loop {
            if true { break }
            poll.poll(&mut events, None).unwrap();

            for event in &events {
                if event.token() == Token(0) && event.readiness().is_writable() {

                    
                }
            }
        }
    });


    // czekam przez okreśłony czas
    thread::sleep(std::time::Duration::new(wait_time, 0));

    // tak jak w slavie wyłączam wątek do zegara i czekam
    tx.send(()).unwrap();
    net_th.join().unwrap();
    child.join().unwrap();
}
```
