#![no_std]
#![no_main]

mod lights;

use core::sync::atomic::{AtomicUsize, Ordering};

use defmt_rtt as _;
use heapless::Vec;
use panic_probe as _;

use adafruit_neotrellis::NeoTrellis;
use lights::BreathingLights;
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
    let devices: &mut [_] = devices.as_mut();
    let pixels: Vec<_, 4> = devices.into_iter().map(|x| x.neopixels()).collect();

    let mut breathing_lights = BreathingLights::<'_, _, 5>::new(pixels);
    breathing_lights.init().unwrap();

    defmt::info!("App started!");

    loop {
        if let Err(_) = breathing_lights.show_next() {
            defmt::error!("Error setting the lights");
        }
    }
}
