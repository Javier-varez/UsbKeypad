#![no_std]
#![no_main]

mod display;

use core::convert::TryFrom;
use core::sync::atomic::{AtomicUsize, Ordering};

use defmt_rtt as _;
use panic_probe as _;

use adafruit_neotrellis::NeoTrellis;
use embedded_graphics::{
    draw_target::DrawTarget,
    mono_font::{ascii::FONT_5X8, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    text::{Baseline, Text, TextStyleBuilder},
};
use nrf52840_hal::{self as _, gpio, pac, timer, twim};
use shared_bus::BusManagerSimple;
use tinybmp::Bmp;

use display::NeoTrellisDisplay;

use rtic::cyccnt::U32Ext as _;

#[defmt::panic_handler]
fn panic() -> ! {
    cortex_m::asm::udf()
}

defmt::timestamp!("{=usize}", {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    COUNT.fetch_add(1, Ordering::Relaxed)
});

fn apply_breathing_effect<I2C, TIMER>(
    display: &mut NeoTrellisDisplay<I2C>,
    timer: &mut TIMER,
    bmp: &Bmp<'_, Rgb888>,
    time_ms: u32,
) where
    I2C: embedded_hal::blocking::i2c::Write + embedded_hal::blocking::i2c::Read,
    TIMER: embedded_hal::blocking::delay::DelayMs<u32>,
{
    const NUM_FRAMES: u32 = 100;

    let time_per_frame = time_ms / NUM_FRAMES;

    let apply_alpha = |value, alpha| {
        let value = value as u32;
        (if alpha < 50 {
            value * alpha / 50
        } else {
            value * (100 - alpha) / 50
        }) as u8
    };

    let convert_color = |color: Rgb888, alpha| {
        Rgb888::new(
            apply_alpha(color.r(), alpha),
            apply_alpha(color.g(), alpha),
            apply_alpha(color.b(), alpha),
        )
    };

    for i in 0..NUM_FRAMES {
        display.clear(Rgb888::BLACK).unwrap();
        display
            .draw_iter(
                bmp.pixels()
                    .map(|pixel| Pixel(pixel.0, convert_color(pixel.1, i))),
            )
            .unwrap();
        display.flush().unwrap();

        timer.delay_ms(time_per_frame);
    }
}

fn scroll_text<T, TIMER>(display: &mut NeoTrellisDisplay<T>, timer: &mut TIMER, text: &str)
where
    T: embedded_hal::blocking::i2c::Write + embedded_hal::blocking::i2c::Read,
    TIMER: embedded_hal::blocking::delay::DelayMs<u32>,
{
    const TEXT_WIDTH: usize = 5;

    let character_style = MonoTextStyle::new(&FONT_5X8, Rgb888::WHITE);
    let text_style = TextStyleBuilder::new().baseline(Baseline::Bottom).build();

    let max_disp = text.len() * TEXT_WIDTH;
    for i in 0..max_disp {
        display.clear(Rgb888::BLACK).unwrap();
        Text::with_text_style(
            text,
            Point::new(-i32::try_from(i).unwrap(), 7),
            character_style,
            text_style,
        )
        .draw(display)
        .unwrap();
        display.flush().unwrap();
        timer.delay_ms(200u32);
    }
}

static HEART_DATA: &[u8; 246] = include_bytes!("../heart.bmp");

#[rtic::app(device = nrf52840_hal::pac, peripherals = true, monotonic = rtic::cyccnt::CYCCNT)]
const APP: () = {
    struct Resources {
        timer: timer::Timer<pac::TIMER0>,
        i2c: BusManagerSimple<twim::Twim<pac::TWIM0>>,
        heart_bmp: Bmp<'static, Rgb888>,
    }

    #[init(schedule = [run_display])]
    fn init(mut cx: init::Context) -> init::LateResources {
        // Initialize (enable) the monotonic timer (CYCCNT)
        cx.core.DCB.enable_trace();
        // required on Cortex-M7 devices that software lock the DWT (e.g. STM32F7)
        cortex_m::peripheral::DWT::unlock();
        cx.core.DWT.enable_cycle_counter();

        let now = cx.start; // the start time of the system

        let peripherals = cx.device;

        let p1 = gpio::p1::Parts::new(peripherals.P1);
        let pins = twim::Pins {
            sda: p1.p1_05.degrade().into_floating_input(),
            scl: p1.p1_06.degrade().into_floating_input(),
        };

        let twim: twim::Twim<pac::TWIM0> =
            twim::Twim::new(peripherals.TWIM0, pins, twim::Frequency::K400);
        let timer = timer::Timer::new(peripherals.TIMER0);
        let i2c = BusManagerSimple::new(twim);

        let heart_bmp = Bmp::<Rgb888>::from_slice(HEART_DATA).unwrap();

        defmt::info!("App started!");

        cx.schedule
            .run_display(now + 8_000_000u32.cycles())
            .unwrap();

        init::LateResources {
            timer,
            i2c,
            heart_bmp,
        }
    }

    #[task(resources = [i2c, timer, heart_bmp])]
    fn run_display(cx: run_display::Context) {
        let timer = cx.resources.timer;
        let i2c = cx.resources.i2c;
        let heart_bmp = cx.resources.heart_bmp;

        let neotrellis_devs = [
            NeoTrellis::new(i2c.acquire_i2c(), timer, Some(0x2E)).unwrap(),
            NeoTrellis::new(i2c.acquire_i2c(), timer, Some(0x2F)).unwrap(),
            NeoTrellis::new(i2c.acquire_i2c(), timer, Some(0x30)).unwrap(),
            NeoTrellis::new(i2c.acquire_i2c(), timer, Some(0x31)).unwrap(),
        ];
        let mut display = NeoTrellisDisplay::new(neotrellis_devs);
        display.init().unwrap();

        loop {
            apply_breathing_effect(&mut display, timer, heart_bmp, 1000);
            scroll_text(&mut display, timer, "Hi There!!");

            defmt::info!("run_display finished");
        }
    }

    extern "C" {
        fn QSPI();
    }
};
