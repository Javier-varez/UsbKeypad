#![no_std]
#![no_main]

mod display;
mod lights;

use core::convert::TryFrom;
use core::sync::atomic::{AtomicUsize, Ordering};

use defmt_rtt as _;
use panic_probe as _;

use adafruit_neotrellis::NeoTrellis;
use embedded_graphics::{
    mono_font::{ascii::FONT_5X8, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    text::{Baseline, Text, TextStyleBuilder},
};
use embedded_hal::blocking::delay::DelayMs;
use nrf52840_hal::{self as _, gpio, pac, timer, twim};
use shared_bus::BusManagerSimple;

use display::NeoTrellisDisplay;

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

    let neotrellis_devs = [
        NeoTrellis::new(i2c.acquire_i2c(), &mut timer, Some(0x2E)).unwrap(),
        NeoTrellis::new(i2c.acquire_i2c(), &mut timer, Some(0x2F)).unwrap(),
        NeoTrellis::new(i2c.acquire_i2c(), &mut timer, Some(0x30)).unwrap(),
        NeoTrellis::new(i2c.acquire_i2c(), &mut timer, Some(0x31)).unwrap(),
    ];

    let mut display = NeoTrellisDisplay::new(neotrellis_devs).unwrap();
    defmt::info!("App started!");

    let character_style = MonoTextStyle::new(&FONT_5X8, Rgb888::new(255, 255, 255));
    let text_style = TextStyleBuilder::new().baseline(Baseline::Bottom).build();
    let text = "SCROLLING TEXT!";

    let max_disp = text.len() * 5;
    loop {
        for i in 0..max_disp {
            display.clear(Rgb888::new(0, 0, 0)).unwrap();
            Text::with_text_style(
                text,
                Point::new(-i32::try_from(i).unwrap(), 7),
                character_style,
                text_style,
            )
            .draw(&mut display)
            .unwrap();
            display.flush().unwrap();

            timer.delay_ms(200u32);
        }
    }
}
