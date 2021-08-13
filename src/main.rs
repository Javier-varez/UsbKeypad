#![no_std]
#![no_main]

use core::sync::atomic::{AtomicUsize, Ordering};

use adafruit_neotrellis::{self as neotrellis, neopixel, NeoPixels, NeoTrellis};
use defmt_rtt as _;
use embedded_hal::{
    blocking::i2c::{Read, Write},
    timer::CountDown,
};
use nrf52840_hal::{
    self as _,
    usbd::{UsbPeripheral, Usbd},
};
use panic_probe as _;
use usb_device::{
    bus::UsbBusAllocator,
    device::{UsbDeviceBuilder, UsbVidPid},
};
use usbd_hid::descriptor::generator_prelude::*;
use usbd_hid::descriptor::MouseReport;
use usbd_hid::hid_class::HIDClass;

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
        neopixels
            .set_pixel_rgb(i as u8, pixels[i].r, pixels[i].g, pixels[i].b)?
            .show()?;
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

struct BreathingLights<'a, I2C: Read + Write> {
    pixels: NeoPixels<'a, I2C>,
    direction: BreathingDirection,
    value: u8,
}

impl<'a, I2C: Read + Write> BreathingLights<'a, I2C> {
    fn new(pixels: NeoPixels<'a, I2C>) -> Self {
        Self {
            pixels,
            direction: BreathingDirection::Increasing,
            value: 0,
        }
    }

    fn calculate_next_state(&mut self) {
        match self.direction {
            BreathingDirection::Increasing => {
                if self.value < 245 {
                    self.value += 10;
                } else {
                    self.value = 255;
                    self.direction = BreathingDirection::Decreasing;
                }
            }
            BreathingDirection::Decreasing => {
                if self.value > 10 {
                    self.value -= 10;
                } else {
                    self.value = 0;
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
        plot_pixel_matrix(&mut self.pixels, &matrix)?;
        Ok(())
    }
}

#[cortex_m_rt::entry]
fn main() -> ! {
    let peripherals = nrf52840_hal::pac::Peripherals::take().unwrap();
    let clocks = nrf52840_hal::clocks::Clocks::new(peripherals.CLOCK);
    let clocks = clocks.enable_ext_hfosc();

    let p1 = nrf52840_hal::gpio::p1::Parts::new(peripherals.P1);
    let pins = nrf52840_hal::twim::Pins {
        sda: p1.p1_05.degrade().into_floating_input(),
        scl: p1.p1_06.degrade().into_floating_input(),
    };
    let twim =
        nrf52840_hal::twim::Twim::new(peripherals.TWIM0, pins, nrf52840_hal::twim::Frequency::K400);

    let mut timer = nrf52840_hal::timer::Timer::new(peripherals.TIMER0);

    let usb_periph = nrf52840_hal::usbd::UsbPeripheral::new(peripherals.USBD, &clocks);
    let usb_bus = nrf52840_hal::usbd::Usbd::new(usb_periph);
    let usb_bus_allocator: UsbBusAllocator<Usbd<UsbPeripheral>> = usb_bus;

    let mut hid_device = HIDClass::new(&usb_bus_allocator, MouseReport::desc(), 60);
    let mut usb_device = UsbDeviceBuilder::new(&usb_bus_allocator, UsbVidPid(0x5824, 0x27dd))
        .manufacturer("AllThingsEmbedded")
        .product("MyCool product")
        .serial_number("TEST")
        .device_class(0xef)
        .build();

    let mut neotrellis = NeoTrellis::new(twim, &mut timer, Some(0x2f)).unwrap();
    let mut pixels = neotrellis.neopixels();

    init_pixels(&mut pixels).unwrap();

    let mut _breathing_lights = BreathingLights::new(pixels);

    defmt::info!("App started!");

    let mut timer = timer.into_periodic();
    timer.start(100_000u32);
    let mut state = 4i8;
    let mut count = 0u8;
    loop {
        // breathing_lights.show_next().unwrap();
        usb_device.poll(&mut [&mut hid_device]);
        if timer.wait() == Ok(()) {
            if count < 10 {
                count += 1;
                continue;
            }
            let report = MouseReport {
                x: 0,
                y: state,
                buttons: 0,
                wheel: 0,
            };
            defmt::info!("Pushing report {}", state);
            hid_device.push_input(&report).unwrap_or(0);
            defmt::info!("Pushed report");
            state = -state;
        }
        nb::block!(timer.wait()).unwrap();
    }
}
