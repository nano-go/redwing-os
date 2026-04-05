#![no_std]

use core::fmt;

pub struct HumanSize(pub u64);

impl fmt::Display for HumanSize {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        const UNITS: [&str; 7] = ["", "K", "M", "G", "T", "P", "E"];

        let mut bytes = self.0;
        let mut unit = 0;
        while bytes >= 1024 && unit < UNITS.len() - 1 {
            bytes /= 1024;
            unit += 1;
        }

        if unit == 0 {
            write!(f, "{}", bytes)
        } else {
            // compute fraction part without float.
            let div = 1024_u64.pow(unit as u32);
            let mut whole = self.0 / div;
            let frac = (self.0 % div) * 10 / div;

            if whole >= 10 || frac == 0 {
                if frac >= 5 {
                    whole += 1;
                }
                write!(f, "{}{}", whole, UNITS[unit])
            } else {
                write!(f, "{}.{}{}", whole, frac, UNITS[unit])
            }
        }
    }
}

pub fn human_size(bytes: u64) -> HumanSize {
    HumanSize(bytes)
}
