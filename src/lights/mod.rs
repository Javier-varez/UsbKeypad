use adafruit_neotrellis::{self as neotrellis, neopixel, NeoPixels};
use embedded_hal::blocking::i2c::{Read, Write};
use heapless::Vec;

#[derive(Clone, Copy)]
pub struct Pixel {
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

pub struct BreathingLights<'a, I2C: Read + Write, const STEP: u8> {
    pixels: Vec<NeoPixels<'a, I2C>, 4>,
    direction: BreathingDirection,
    value: u8,
}

impl<'a, I2C: Read + Write, const STEP: u8> BreathingLights<'a, I2C, STEP> {
    pub fn new(pixels: Vec<NeoPixels<'a, I2C>, 4>) -> Self {
        Self {
            pixels,
            direction: BreathingDirection::Increasing,
            value: 0,
        }
    }

    pub fn init(&mut self) -> Result<(), neotrellis::Error> {
        let mut_pixels: &mut Vec<NeoPixels<'a, I2C>, 4> = self.pixels.as_mut();
        mut_pixels
            .into_iter()
            .for_each(|pixel| init_pixels(pixel).unwrap());
        Ok(())
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

    pub fn show_next(&mut self) -> Result<(), neotrellis::Error> {
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
