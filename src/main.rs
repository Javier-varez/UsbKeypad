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
use tinybmp::Bmp;

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

    let character_style = MonoTextStyle::new(&FONT_5X8, Rgb888::WHITE);
    let text_style = TextStyleBuilder::new().baseline(Baseline::Bottom).build();
    let text = "COOL!";

    let bmp_data = include_bytes!("../heart.bmp");
    let bmp = Bmp::<Rgb888>::from_slice(bmp_data).unwrap();

    const TEXT_WIDTH: usize = 5;
    let max_disp = text.len() * TEXT_WIDTH;
    loop {
        for _ in 0..4 {
            for i in 0..100 {
                display.clear(Rgb888::BLACK).unwrap();
                display
                    .draw_iter(bmp.pixels().map(|pixel| {
                        if i < 50 {
                            Pixel(
                                pixel.0,
                                Rgb888::new(
                                    (pixel.1.r() as u32 * i / 50) as u8,
                                    (pixel.1.g() as u32 * i / 50) as u8,
                                    (pixel.1.b() as u32 * i / 50) as u8,
                                ),
                            )
                        } else {
                            Pixel(
                                pixel.0,
                                Rgb888::new(
                                    (pixel.1.r() as u32 * (100 - i) / 50) as u8,
                                    (pixel.1.g() as u32 * (100 - i) / 50) as u8,
                                    (pixel.1.b() as u32 * (100 - i) / 50) as u8,
                                ),
                            )
                        }
                    }))
                    .unwrap();
                display.flush().unwrap();

                timer.delay_ms(20u32);
            }
        }

        for i in 0..max_disp {
            display.clear(Rgb888::BLACK).unwrap();
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
