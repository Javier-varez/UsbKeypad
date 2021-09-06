#![no_std]
#![no_main]

mod display;

use core::convert::TryFrom;
use core::sync::atomic::{AtomicUsize, Ordering};

use defmt_rtt as _;
use panic_probe as _;

use embedded_graphics::{
    draw_target::DrawTarget,
    mono_font::{ascii::FONT_5X8, MonoTextStyle},
    pixelcolor::Rgb888,
    prelude::*,
    text::{Baseline, Text, TextStyleBuilder},
};
use nrf52840_hal as _;
use tinybmp::Bmp;

use display::NeoTrellisDisplay;

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

#[rtic::app(device = nrf52840_hal::pac, peripherals = true, dispatchers = [USBD, QSPI, NFCT])]
mod app {
    use crate::scroll_text;
    use adafruit_neotrellis::NeoTrellis;
    use embedded_graphics::pixelcolor::Rgb888;
    use nrf52840_hal::{self as _, gpio, pac, timer, twim};
    use shared_bus::BusManagerSimple;
    use tinybmp::Bmp;

    use crate::apply_breathing_effect;
    use crate::display::NeoTrellisDisplay;

    use nrf52840_hal::clocks;
    use nrf52840_hal::usbd;

    use usb_device::{
        bus::UsbBusAllocator,
        device::{UsbDevice, UsbDeviceBuilder},
        prelude::*,
    };
    use usbd_hid::descriptor::{MouseReport, SerializedDescriptor};
    use usbd_hid::hid_class::HIDClass;

    use dwt_systick_monotonic::DwtSystick;
    use rtic::time::duration::Milliseconds;

    const MONO_HZ: u32 = 64_000_000; // 64 MHz

    #[monotonic(binds = SysTick, default = true, priority = 8)]
    type MyMono = DwtSystick<MONO_HZ>;

    #[local]
    struct Local {
        timer: timer::Timer<pac::TIMER0>,
        i2c: BusManagerSimple<twim::Twim<pac::TWIM0>>,
        heart_bmp: Bmp<'static, Rgb888>,
        usb_device: UsbDevice<'static, usbd::Usbd<usbd::UsbPeripheral<'static>>>,
    }

    #[shared]
    struct Shared {
        hid_class: HIDClass<'static, usbd::Usbd<usbd::UsbPeripheral<'static>>>,
    }

    #[init(
        local = [
            clocks: Option<clocks::Clocks<clocks::ExternalOscillator, clocks::Internal, clocks::LfOscStopped>> = None,
            usb_buf_alloc: Option<UsbBusAllocator<usbd::Usbd<usbd::UsbPeripheral<'static>>>> = None,
        ]
    )]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
        static HEART_DATA: &[u8; 246] = include_bytes!("../heart.bmp");

        let peripherals = cx.device;

        let mut dcb = cx.core.DCB;
        let dwt = cx.core.DWT;
        let systick = cx.core.SYST;

        let mono = DwtSystick::new(&mut dcb, dwt, systick, 64_000_000);

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

        let clocks = clocks::Clocks::new(peripherals.CLOCK);
        *cx.local.clocks = Some(clocks.enable_ext_hfosc());

        // Usb class
        let usb_periph =
            usbd::UsbPeripheral::new(peripherals.USBD, cx.local.clocks.as_ref().unwrap());
        let usb_bus = usbd::Usbd::new(usb_periph);
        *cx.local.usb_buf_alloc = Some(usb_bus.into());
        let usb_bus_allocator = cx.local.usb_buf_alloc.as_ref().unwrap();

        let mut hid_class = HIDClass::new(usb_bus_allocator, MouseReport::desc(), 60);
        let mut usb_device = UsbDeviceBuilder::new(usb_bus_allocator, UsbVidPid(0x5824, 0x27dd))
            .manufacturer("AllThingsEmbedded")
            .product("USB mouse")
            .serial_number("00000000")
            .device_class(0xef)
            .build();
        usb_device.poll(&mut [&mut hid_class]);

        usb_task::spawn().unwrap();
        run_display::spawn().unwrap();

        (
            Shared { hid_class },
            Local {
                timer,
                i2c,
                heart_bmp,
                usb_device,
            },
            init::Monotonics(mono),
        )
    }

    #[task(local = [usb_device], shared = [hid_class], priority = 3)]
    fn usb_task(mut cx: usb_task::Context) {
        let usb_dev = cx.local.usb_device;
        cx.shared.hid_class.lock(|hid| {
            usb_dev.poll(&mut [hid]);
        });
        usb_task::spawn_after(Milliseconds(2u32)).unwrap();
    }

    #[task(shared = [hid_class], priority = 2)]
    fn hid_task(mut cx: hid_task::Context) {
        defmt::info!("moving mouse");
        cx.shared.hid_class.lock(|hid| {
            let report = MouseReport {
                x: 0,
                y: 10,
                buttons: 0,
                wheel: 0,
            };
            hid.push_input(&report).unwrap();
        });
    }

    #[task(local = [i2c, timer, heart_bmp], priority = 1)]
    fn run_display(cx: run_display::Context) {
        let timer = cx.local.timer;
        let i2c = cx.local.i2c;
        let heart_bmp = cx.local.heart_bmp;

        let neotrellis_devs = [
            NeoTrellis::new(i2c.acquire_i2c(), timer, Some(0x2E)).unwrap(),
            NeoTrellis::new(i2c.acquire_i2c(), timer, Some(0x2F)).unwrap(),
            NeoTrellis::new(i2c.acquire_i2c(), timer, Some(0x30)).unwrap(),
            NeoTrellis::new(i2c.acquire_i2c(), timer, Some(0x31)).unwrap(),
        ];
        let mut display = NeoTrellisDisplay::new(neotrellis_devs);
        display.init().unwrap();

        // TODO(javier): Chunk these operations so that they keypad can be used concurrently
        apply_breathing_effect(&mut display, timer, heart_bmp, 1000);
        scroll_text(&mut display, timer, "Hi There!!");

        defmt::info!("run_display finished");

        hid_task::spawn().unwrap();
        run_display::spawn_after(Milliseconds(10u32)).ok();
    }
}
