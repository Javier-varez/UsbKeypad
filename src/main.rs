#![no_std]
#![no_main]

mod display;

use core::sync::atomic::{AtomicUsize, Ordering};

use defmt_rtt as _;
use panic_probe as _;

use nrf52840_hal as _;

pub struct HidKeys {
    keys: [u8; 6],
}

impl HidKeys {
    fn new() -> Self {
        Self { keys: [0; 6] }
    }

    fn press_key(&mut self, scan_code: u8) {
        for i in 0..6 {
            if self.keys[i] == 0 {
                self.keys[i] = scan_code;
                break;
            }
        }
    }

    fn release_key(&mut self, scan_code: u8) {
        for i in 0..6 {
            if self.keys[i] == scan_code {
                self.keys[i] = 0;
                break;
            }
        }
    }

    fn clone_to_array(&self) -> [u8; 6] {
        let mut keycodes = [0u8; 6];
        keycodes.clone_from_slice(&self.keys);
        keycodes
    }
}

#[defmt::panic_handler]
fn panic() -> ! {
    cortex_m::asm::udf()
}

defmt::timestamp!("{=usize}", {
    static COUNT: AtomicUsize = AtomicUsize::new(0);
    COUNT.fetch_add(1, Ordering::Relaxed)
});

#[rtic::app(device = nrf52840_hal::pac, peripherals = true, dispatchers = [USBD, QSPI, NFCT])]
mod app {
    use adafruit_neotrellis::NeoTrellis;
    use nrf52840_hal::{self as _, gpio, pac, timer, twim};
    use shared_bus::BusManagerAtomicCheck as BusManager;

    use crate::display::NeoTrellisDisplay;

    use nrf52840_hal::clocks;
    use nrf52840_hal::usbd;

    use usb_device::{
        bus::UsbBusAllocator,
        device::{UsbDevice, UsbDeviceBuilder},
        prelude::*,
    };
    use usbd_hid::descriptor::{KeyboardReport, SerializedDescriptor};
    use usbd_hid::hid_class::HIDClass;

    use dwt_systick_monotonic::DwtSystick;
    use rtic::time::duration::Milliseconds;

    const MONO_HZ: u32 = 64_000_000; // 64 MHz

    #[monotonic(binds = SysTick, default = true, priority = 8)]
    type MyMono = DwtSystick<MONO_HZ>;

    #[local]
    struct Local {
        timer: timer::Timer<pac::TIMER0>,
        display: NeoTrellisDisplay<
            shared_bus::I2cProxy<'static, shared_bus::AtomicCheckMutex<twim::Twim<pac::TWIM0>>>,
        >,
        usb_device: UsbDevice<'static, usbd::Usbd<usbd::UsbPeripheral<'static>>>,
    }

    #[shared]
    struct Shared {
        hid_class: HIDClass<'static, usbd::Usbd<usbd::UsbPeripheral<'static>>>,
        keycodes: crate::HidKeys,
    }

    #[init(
        local = [
            clocks: Option<clocks::Clocks<clocks::ExternalOscillator, clocks::Internal, clocks::LfOscStopped>> = None,
            usb_buf_alloc: Option<UsbBusAllocator<usbd::Usbd<usbd::UsbPeripheral<'static>>>> = None,
            i2c_bus: Option<BusManager<twim::Twim<pac::TWIM0>>> = None,
        ]
    )]
    fn init(cx: init::Context) -> (Shared, Local, init::Monotonics) {
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
        let mut timer = timer::Timer::new(peripherals.TIMER0);
        let i2c = BusManager::new(twim);

        let clocks = clocks::Clocks::new(peripherals.CLOCK);
        *cx.local.clocks = Some(clocks.enable_ext_hfosc());

        // Usb class
        let usb_periph =
            usbd::UsbPeripheral::new(peripherals.USBD, cx.local.clocks.as_ref().unwrap());
        let usb_bus = usbd::Usbd::new(usb_periph);
        *cx.local.usb_buf_alloc = Some(usb_bus);
        let usb_bus_allocator = cx.local.usb_buf_alloc.as_ref().unwrap();

        let mut hid_class = HIDClass::new(usb_bus_allocator, KeyboardReport::desc(), 60);
        let mut usb_device = UsbDeviceBuilder::new(usb_bus_allocator, UsbVidPid(0x5824, 0x27dd))
            .manufacturer("AllThingsEmbedded")
            .product("USB mouse")
            .serial_number("00000000")
            .device_class(0xef)
            .build();
        usb_device.poll(&mut [&mut hid_class]);

        *cx.local.i2c_bus = Some(i2c);
        let i2c = cx.local.i2c_bus.as_mut().unwrap();

        let neotrellis_devs = [
            NeoTrellis::new(i2c.acquire_i2c(), &mut timer, Some(0x2E)).unwrap(),
            NeoTrellis::new(i2c.acquire_i2c(), &mut timer, Some(0x2F)).unwrap(),
            NeoTrellis::new(i2c.acquire_i2c(), &mut timer, Some(0x30)).unwrap(),
            NeoTrellis::new(i2c.acquire_i2c(), &mut timer, Some(0x31)).unwrap(),
        ];
        let mut display = NeoTrellisDisplay::new(neotrellis_devs);
        display.init().unwrap();

        usb_task::spawn().unwrap();
        run_display::spawn().unwrap();

        let keycodes = crate::HidKeys::new();

        (
            Shared {
                hid_class,
                keycodes,
            },
            Local {
                timer,
                display,
                usb_device,
            },
            init::Monotonics(mono),
        )
    }

    #[task(local = [usb_device], shared = [hid_class], priority = 3)]
    fn usb_task(mut cx: usb_task::Context) {
        let usb_dev = cx.local.usb_device;
        cx.shared.hid_class.lock(|hid| {
            if usb_dev.poll(&mut [hid]) {
                hid_task::spawn().unwrap();
            }
        });
        usb_task::spawn_after(Milliseconds(2u32)).unwrap();
    }

    #[task(shared = [hid_class, keycodes], priority = 2)]
    fn hid_task(mut cx: hid_task::Context) {
        let keycodes = cx.shared.keycodes.lock(|k| k.clone_to_array());

        cx.shared.hid_class.lock(|hid| {
            let report = KeyboardReport {
                modifier: 0,
                leds: 0,
                keycodes,
            };
            match hid.push_input(&report) {
                Err(UsbError::WouldBlock) => defmt::warn!("hid_task: Would block"),
                Err(err) => panic!("{:?}", err),
                Ok(_) => {}
            };
        });
    }

    #[task(local = [display, timer], shared = [keycodes], priority = 1)]
    fn run_display(cx: run_display::Context) {
        let display = cx.local.display;
        let timer = cx.local.timer;
        let mut keycodes = cx.shared.keycodes;

        display
            .process_events(timer, |key_idx| match key_idx.event_type {
                crate::display::EventType::KeyUp => {
                    keycodes.lock(|k| k.release_key(key_idx.usb_scan_code));
                }
                crate::display::EventType::KeyDown => {
                    keycodes.lock(|k| k.press_key(key_idx.usb_scan_code));
                }
            })
            .unwrap();

        run_display::spawn_after(Milliseconds(10u32)).ok();
    }
}
