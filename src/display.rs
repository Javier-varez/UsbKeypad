use core::convert::TryInto;
use core::result::Result;

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    pixelcolor::{self, RgbColor},
    Pixel,
};

use adafruit_neotrellis::{self as neotrellis, neopixel, NeoPixels, NeoTrellis};
use embedded_hal::blocking::i2c::{Read, Write};

#[derive(Debug)]
pub enum Error {
    Device(neotrellis::Error),
    OutOfBoundsCoordinate,
}

impl From<neotrellis::Error> for Error {
    fn from(error: neotrellis::Error) -> Self {
        Error::Device(error)
    }
}

fn init_pixels<I2C: Read + Write>(
    pixels: &'_ mut NeoPixels<'_, I2C>,
) -> Result<(), neotrellis::Error> {
    pixels
        .set_pin(3)?
        .set_speed(neopixel::Speed::Khz400)?
        .set_pixel_type(neopixel::ColorOrder::GRB)?
        .set_pixel_count(16)?;
    Ok(())
}

fn plot_pixel_matrix<'a, I2C: Read + Write>(
    neopixels: &'_ mut NeoPixels<'a, I2C>,
    pixels: &[pixelcolor::Rgb888],
) -> Result<(), neotrellis::Error> {
    for (i, pixel) in pixels.iter().enumerate() {
        neopixels.set_pixel_rgb(i as u8, pixel.r(), pixel.g(), pixel.b())?;
    }
    neopixels.show()?;

    Ok(())
}

pub struct NeoTrellisDisplay<I2C: Read + Write> {
    devices: [NeoTrellis<I2C>; 4],
    framebuffer: [pixelcolor::Rgb888; 64],
}

impl<I2C> OriginDimensions for NeoTrellisDisplay<I2C>
where
    I2C: Read + Write,
{
    fn size(&self) -> Size {
        Size::new(8, 8)
    }
}

impl<I2C> NeoTrellisDisplay<I2C>
where
    I2C: Read + Write,
{
    pub fn new(devices: [NeoTrellis<I2C>; 4]) -> Result<Self, Error> {
        let mut instance = Self {
            devices,
            framebuffer: [pixelcolor::Rgb888::default(); 64],
        };

        for dev in &mut instance.devices {
            init_pixels(&mut dev.neopixels())?;
        }
        instance.flush()?;

        Ok(instance)
    }
    pub fn flush(&mut self) -> Result<(), Error> {
        for (i, dev) in self.devices.iter_mut().enumerate() {
            let index = i * 16;
            plot_pixel_matrix(&mut dev.neopixels(), &self.framebuffer[index..index + 16])?;
        }
        Ok(())
    }

    fn index_for_coordinate(&self, point: Point) -> Result<usize, Error> {
        if let Ok((0..=7, 0..=7)) = point.try_into() {
            let dev_index = (1 - point.x / 4) * 1 + (1 - point.y / 4) * 2;
            let pix_index = point.x % 4 + (point.y % 4) * 4;

            return Ok((dev_index * 16 + pix_index) as usize);
        }
        Err(Error::OutOfBoundsCoordinate)
    }
}

impl<I2C> DrawTarget for NeoTrellisDisplay<I2C>
where
    I2C: Read + Write,
{
    type Color = pixelcolor::Rgb888;
    type Error = Error;

    fn draw_iter<I>(&mut self, pixels: I) -> Result<(), Self::Error>
    where
        I: IntoIterator<Item = Pixel<Self::Color>>,
    {
        for Pixel(coord, color) in pixels.into_iter() {
            if let Ok(index) = self.index_for_coordinate(coord) {
                self.framebuffer[index] = color;
            }
        }
        Ok(())
    }
}
