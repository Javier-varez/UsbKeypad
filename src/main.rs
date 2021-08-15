#![no_std]
#![no_main]

mod lights;

use core::sync::atomic::{AtomicUsize, Ordering};

use defmt_rtt as _;
use heapless::Vec;
use panic_probe as _;

use adafruit_neotrellis::{Edge, KeyEvent, NeoTrellis};
use lights::{BreathingLights, Pixel};
use nrf52840_hal::{self as _, gpio, pac, timer, twim};
use shared_bus::BusManagerSimple;

#[defmt::panic_handler]
fn panic() -> ! {
    cortex_m::asm::udf()
}

static COUNT: AtomicUsize = AtomicUsize::new(0);
defmt::timestamp!("{=usize}", {
    // NOTE(no-CAS) `timestamps` runs with interrupts disabled
    let n = COUNT.load(Ordering::Relaxed);
    COUNT.store(n + 1, Ordering::Relaxed);
    n
});

fn update_matrix(matrix: &mut [Pixel], index: u8, event: KeyEvent) {
    let matrix_idx = (index * 16 + event.key) as usize;
    matrix[matrix_idx].r = !matrix[matrix_idx].r;
    matrix[matrix_idx].g = !matrix[matrix_idx].g;
    matrix[matrix_idx].b = !matrix[matrix_idx].b;
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let peripherals = pac::Peripherals::take().unwrap();

    let p1 = gpio::p1::Parts::new(peripherals.P1);
    let pins = twim::Pins {
        sda: p1.p1_05.degrade().into_floating_input(),
        scl: p1.p1_06.degrade().into_floating_input(),
    };
    let twim = twim::Twim::new(peripherals.TWIM0, pins, twim::Frequency::K400);

    let i2c = BusManagerSimple::new(twim);

    let mut timer = timer::Timer::new(peripherals.TIMER0);

    let neotrellis_addresses = [0x2e, 0x2f, 0x30, 0x31];
    let mut devices: Vec<_, 4> = neotrellis_addresses
        .iter()
        .filter_map(|addr| NeoTrellis::new(i2c.acquire_i2c(), &mut timer, Some(*addr)).ok())
        .collect();
    let mut pixels: Vec<_, 4> = devices.iter_mut().map(|x| x.neopixels()).collect();

    let mut breathing_lights = BreathingLights::<5>::new();
    breathing_lights.init(&mut pixels).unwrap();
    drop(pixels);

    defmt::info!("App started!");

    devices.iter_mut().for_each(|neotrellis| {
        for i in 0..16 {
            neotrellis
                .keypad()
                .enable_key_event(i, Edge::Rising)
                .unwrap();
        }
        lights::init_pixels(&mut neotrellis.neopixels()).unwrap();
    });

    let mut matrix = [Pixel { r: 0, g: 0, b: 0 }; 64];

    loop {
        devices
            .iter_mut()
            .enumerate()
            .for_each(|(index, neotrellis)| {
                if let Some(event) = neotrellis.keypad().get_event(&mut timer).unwrap() {
                    defmt::info!("Event: {}", event);
                    update_matrix(&mut matrix, index as u8, event);
                    let index = (index * 16) as usize;
                    lights::plot_pixel_matrix(
                        &mut neotrellis.neopixels(),
                        &matrix[index..index + 16],
                    )
                    .unwrap();
                }
            });
    }
}
