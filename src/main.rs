#![no_std]
#![no_main]
#![feature(type_alias_impl_trait)]

mod lights;
mod seesaw;

use core::sync::atomic::{AtomicUsize, Ordering};

use defmt_rtt as _;
use panic_probe as _;

use embassy::executor::Spawner;
use embassy_nrf::{interrupt, twim, Peripherals};
use embassy_traits::i2c::I2c;
use seesaw::{
    neopixel::{ColorOrder, Speed},
    SeeSaw,
};

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

async fn set_pixel_rgb<I2C>(
    seesaw: &mut SeeSaw<I2C>,
    index: u8,
    red: u8,
    green: u8,
    blue: u8,
) -> Result<(), seesaw::Error>
where
    I2C: I2c,
{
    seesaw
        .neopixel_write_buf_raw(3 * (index as u16), &[green, red, blue])
        .await?;

    Ok(())
}

#[embassy::main]
async fn main(_spawner: Spawner, peripherals: Peripherals) {
    let mut config = twim::Config::default();
    config.frequency = twim::Frequency::K400;

    let irq = interrupt::take!(SPIM0_SPIS0_TWIM0_TWIS0_SPI0_TWI0);

    let twim = twim::Twim::new(
        peripherals.TWISPI0,
        irq,
        peripherals.P1_05,
        peripherals.P1_06,
        config,
    );

    let mut seesaw = SeeSaw {
        i2c: twim,
        address: 0x2E,
    };

    seesaw.neopixel_set_speed(Speed::Khz400).await.unwrap();
    seesaw.neopixel_set_pin(3).await.unwrap();
    seesaw
        .neopixel_set_buf_length_pixels(16, ColorOrder::GRB)
        .await
        .unwrap();

    for i in 0..16 {
        set_pixel_rgb(&mut seesaw, i, 255, 255, 255).await.unwrap();
    }
    seesaw.neopixel_show().await.unwrap();

    defmt::info!("App started!");

    loop {}
}
