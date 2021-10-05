use core::convert::TryInto;

use embedded_graphics::{
    draw_target::DrawTarget,
    geometry::{OriginDimensions, Point, Size},
    pixelcolor::{self, RgbColor},
    Pixel,
};

use adafruit_neotrellis::{self as neotrellis, neopixel, NeoPixels, NeoTrellis};
use embedded_hal::blocking::i2c::{Read, Write};

pub enum EventType {
    KeyDown,
    KeyUp,
}

pub struct KeyEvent {
    pub usb_scan_code: u8,
    pub event_type: EventType,
}

impl From<neotrellis::Edge> for EventType {
    fn from(edge: neotrellis::Edge) -> Self {
        match edge {
            neotrellis::Edge::Falling => Self::KeyUp,
            neotrellis::Edge::Rising => Self::KeyDown,
            _ => unimplemented!(),
        }
    }
}

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

fn index_for_device_and_pixel(device_idx: u32, pix_idx: u32) -> usize {
    return (device_idx * 16 + pix_idx) as usize;
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
    pub fn new(devices: [NeoTrellis<I2C>; 4]) -> Self {
        Self {
            devices,
            framebuffer: [pixelcolor::Rgb888::default(); 64],
        }
    }

    pub fn init(&mut self) -> Result<(), Error> {
        for dev in &mut self.devices {
            init_pixels(&mut dev.neopixels())?;
            let mut keypad = dev.keypad();
            for i in 0..16 {
                keypad.enable_key_event(i, neotrellis::Edge::Falling)?;
                keypad.enable_key_event(i, neotrellis::Edge::Rising)?;
            }
        }
        self.flush()?;
        Ok(())
    }

    pub fn process_events<
        Delay: embedded_hal::blocking::delay::DelayUs<u32>,
        Handler: FnMut(KeyEvent),
    >(
        &mut self,
        delay: &mut Delay,
        mut event_handler: Handler,
    ) -> Result<(), Error> {
        let mut any_updates = false;
        for (dev_idx, dev) in self.devices.iter_mut().enumerate() {
            let mut keypad = dev.keypad();
            let pending_events = keypad.pending_events(delay)?;

            for _ in 0..pending_events {
                match keypad.get_event(delay)? {
                    Some(event) => {
                        let index = index_for_device_and_pixel(
                            dev_idx.try_into().unwrap(),
                            event.key.into(),
                        );

                        match event.event {
                            neotrellis::Edge::Falling => {
                                self.framebuffer[index] = pixelcolor::Rgb888::BLACK;
                                any_updates = true;
                            }
                            neotrellis::Edge::Rising => {
                                self.framebuffer[index] = pixelcolor::Rgb888::WHITE;
                                any_updates = true;
                            }
                            _ => {}
                        }
                        let event = KeyEvent {
                            // TODO(javier): Use proper scan code table and remap pixels
                            usb_scan_code: index as u8 + 4,
                            event_type: event.event.into(),
                        };
                        event_handler(event);
                    }
                    None => {
                        defmt::error!("Incomplete read of events for keypad device!");
                        break;
                    }
                }
            }
        }

        if any_updates {
            self.flush()?;
        }
        Ok(())
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
            let dev_index = 1 - point.x / 4 + (1 - point.y / 4) * 2;
            let pix_index = point.x % 4 + (point.y % 4) * 4;

            return Ok(index_for_device_and_pixel(
                dev_index as u32,
                pix_index as u32,
            ));
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
