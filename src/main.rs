#![no_std]
#![no_main]

use core::sync::atomic::{AtomicUsize, Ordering};

use adafruit_neotrellis::{self as neotrellis, neopixel, NeoPixels, NeoTrellis};
use defmt_rtt as _;
use embedded_hal::blocking::i2c::{Read, Write};
use heapless::Vec;
use nrf52840_hal as _;
use panic_probe as _;
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

#[derive(Clone, Copy)]
struct Pixel {
    r: u8,
    g: u8,
    b: u8,
}

fn plot_pixel_matrix<'a, I2C: Read + Write>(
    neopixels: &'_ mut NeoPixels<'a, I2C>,
    pixels: &[Pixel],
) -> Result<(), neotrellis::Error> {
    for i in 0..16usize {
        neopixels.set_pixel_rgb(i as u8, pixels[i].r, pixels[i].g, pixels[i].b)?;
    }
    neopixels.show()?;

    Ok(())
}

fn init_pixels<'a, I2C: Read + Write>(
    pixels: &'_ mut NeoPixels<'a, I2C>,
) -> Result<(), neotrellis::Error> {
    pixels
        .set_pin(3)?
        .set_speed(neopixel::Speed::Khz400)?
        .set_pixel_type(neopixel::ColorOrder::GRB)?
        .set_pixel_count(16)?;

    let matrix = [Pixel { r: 0, g: 0, b: 0 }; 16];
    plot_pixel_matrix(pixels, &matrix)?;

    Ok(())
}

enum BreathingDirection {
    Increasing,
    Decreasing,
}

struct BreathingLights<'a, I2C: Read + Write, const STEP: u8> {
    pixels: Vec<NeoPixels<'a, I2C>, 4>,
    direction: BreathingDirection,
    value: u8,
}

impl<'a, I2C: Read + Write, const STEP: u8> BreathingLights<'a, I2C, STEP> {
    fn new(pixels: Vec<NeoPixels<'a, I2C>, 4>) -> Self {
        Self {
            pixels,
            direction: BreathingDirection::Increasing,
            value: 0,
        }
    }

    fn init(&mut self) {
        let mut_pixels: &mut Vec<NeoPixels<'a, I2C>, 4> = self.pixels.as_mut();
        mut_pixels
            .into_iter()
            .for_each(|pixel| init_pixels(pixel).unwrap());
    }

    fn calculate_next_state(&mut self) {
        match self.direction {
            BreathingDirection::Increasing => {
                self.value = self.value.saturating_add(STEP);
                if self.value == 255 {
                    self.direction = BreathingDirection::Decreasing;
                }
            }
            BreathingDirection::Decreasing => {
                self.value = self.value.saturating_sub(STEP);
                if self.value == 0 {
                    self.direction = BreathingDirection::Increasing;
                }
            }
        }
    }

    fn show_next(&mut self) -> Result<(), neotrellis::Error> {
        self.calculate_next_state();
        let matrix = [Pixel {
            r: self.value,
            g: self.value,
            b: self.value,
        }; 16];

        let mut_pixels: &mut Vec<NeoPixels<'a, I2C>, 4> = self.pixels.as_mut();
        mut_pixels
            .into_iter()
            .for_each(|pixel| plot_pixel_matrix(pixel, &matrix).unwrap());

        Ok(())
    }
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let peripherals = nrf52840_hal::pac::Peripherals::take().unwrap();

    let p1 = nrf52840_hal::gpio::p1::Parts::new(peripherals.P1);
    let pins = nrf52840_hal::twim::Pins {
        sda: p1.p1_05.degrade().into_floating_input(),
        scl: p1.p1_06.degrade().into_floating_input(),
    };
    let twim =
        nrf52840_hal::twim::Twim::new(peripherals.TWIM0, pins, nrf52840_hal::twim::Frequency::K400);

    let i2c = BusManagerSimple::new(twim);

    let mut timer = nrf52840_hal::timer::Timer::new(peripherals.TIMER0);

    let mut neotrellis = [
        NeoTrellis::new(i2c.acquire_i2c(), &mut timer, Some(0x2e)).ok(),
        NeoTrellis::new(i2c.acquire_i2c(), &mut timer, Some(0x2f)).ok(),
        NeoTrellis::new(i2c.acquire_i2c(), &mut timer, Some(0x30)).ok(),
        NeoTrellis::new(i2c.acquire_i2c(), &mut timer, Some(0x31)).ok(),
    ];

    let pixels: Vec<NeoPixels<'_, _>, 4> = neotrellis
        .as_mut()
        .into_iter()
        .filter_map(|x| x.as_mut().map(|x| x.neopixels()))
        .collect();

    let mut breathing_lights = BreathingLights::<'_, _, 5>::new(pixels);
    breathing_lights.init();

    defmt::info!("App started!");

    loop {
        breathing_lights.show_next().unwrap();
    }
}
