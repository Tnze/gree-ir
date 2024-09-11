#![no_std]

use core::{fmt::Debug, hint::unreachable_unchecked, iter::once};

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Code {
    Start,
    Continue,
    End,

    Short, // 0
    Long,  // 1
}

impl From<bool> for Code {
    fn from(value: bool) -> Self {
        if value {
            Code::Long
        } else {
            Code::Short
        }
    }
}

impl TryInto<u8> for &Code {
    type Error = DecodeError;

    fn try_into(self) -> Result<u8, Self::Error> {
        match self {
            Code::Start | Code::Continue | Code::End => Err(DecodeError::UnexpectedMarker),
            Code::Short => Ok(0),
            Code::Long => Ok(1),
        }
    }
}

impl TryInto<bool> for &Code {
    type Error = DecodeError;

    fn try_into(self) -> Result<bool, Self::Error> {
        match self {
            Code::Start | Code::Continue | Code::End => Err(DecodeError::UnexpectedMarker),
            Code::Short => Ok(false),
            Code::Long => Ok(true),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Message {
    remote_state: [u8; 8],
}

impl Message {
    pub fn new() -> Self {
        Self {
            remote_state: [0, 0, 0, 0b01010000, 0, 0b00100000, 0, 0],
        }
    }

    pub fn encode(&self) -> impl Iterator<Item = Code> + use<'_> {
        let checksum = self.checksum();
        let byte_to_codes = |x| (0..8).map(move |i| Code::from(x >> i & 1u8 != 0u8));
        let code1 = self.remote_state[..4].iter().flat_map(byte_to_codes);
        let code2 = self.remote_state[4..].iter().flat_map(byte_to_codes);
        once(Code::Start)
            .chain(code1)
            .chain(MAGIC_3.into_iter())
            .chain(code2.take(4 * 8 - 4))
            .chain((0..4).map(move |i| Code::from(checksum >> i & 1u8 != 0u8)))
    }

    pub fn decode(codes: &[Code; 70]) -> Result<Self, DecodeError> {
        let mut message = Self {
            remote_state: [0; 8],
        };
        let mut iter = codes.iter();
        // Start
        let Code::Start = iter.next().ok_or(DecodeError::Eof)? else {
            return Err(DecodeError::InvalidMarker);
        };
        // Code 1
        for v in message.remote_state[..4].iter_mut() {
            for i in 0..8 {
                let t: &Code = iter.next().ok_or(DecodeError::Eof)?;
                *v |= TryInto::<u8>::try_into(t)? << i;
            }
        }
        check_magic_code3(&mut iter)?;
        // Continue
        let Code::Continue = iter.next().ok_or(DecodeError::Eof)? else {
            return Err(DecodeError::InvalidMarker);
        };
        // Code 2
        for v in message.remote_state[4..].iter_mut() {
            for i in 0..8 {
                let t: &Code = iter.next().ok_or(DecodeError::Eof)?;
                *v |= TryInto::<u8>::try_into(t)? << i;
            }
        }
        // End
        let Code::End = iter.next().ok_or(DecodeError::Eof)? else {
            return Err(DecodeError::InvalidMarker);
        };
        // Checksum
        if message.checksum() != message.remote_state[7] >> 4 {
            return Err(DecodeError::Checksum);
        }
        Ok(message)
    }

    fn checksum(&self) -> u8 {
        let mut sum = 10;
        // Sum the lower half of the first 4 bytes of this block.
        for v in self.remote_state.iter().take(4) {
            sum += *v & 0xF;
        }
        // then sum the upper half of the next 3 bytes.
        for v in self.remote_state[4..].iter().take(3) {
            sum += *v >> 4;
        }
        // Trim it down to fit into the 4 bits allowed. i.e. Mod 16.
        sum & 0xF
    }

    pub fn mode(&self) -> Result<Mode, DecodeError> {
        match self.remote_state[0] & 0b111 {
            0 => Ok(Mode::Auto),
            1 => Ok(Mode::Cold),
            2 => Ok(Mode::Dry),
            3 => Ok(Mode::Wind),
            4 => Ok(Mode::Hot),
            _ => Err(DecodeError::InvalidMode),
        }
    }

    pub fn set_mode(&mut self, mode: Mode) {
        self.remote_state[0] = self.remote_state[0] & 0b1111_1000 | mode as u8;
    }

    pub fn is_on(&self) -> bool {
        self.remote_state[0] >> 3 & 1 != 0
    }

    pub fn set_on(&mut self, on: bool) {
        self.remote_state[0] = self.remote_state[0] & 0b1111_0111 | (on as u8) << 3;
    }

    pub fn fan(&self) -> Fan {
        match self.remote_state[0] >> 4 & 0b11 {
            0 => Fan::Auto,
            1 => Fan::Level1,
            2 => Fan::Level2,
            3 => Fan::Level3,
            _ => unsafe { unreachable_unchecked() },
        }
    }

    pub fn set_fan(&mut self, fan: Fan) {
        self.remote_state[0] = self.remote_state[0] & 0b1100_1111 | (fan as u8) << 4;
    }

    pub fn swing(&self) -> bool {
        self.remote_state[0] >> 6 & 1 != 0
    }

    pub fn set_swing(&mut self, swing: bool) {
        self.remote_state[0] = self.remote_state[0] & 0b1011_1111 | (swing as u8) << 6;
    }

    pub fn sleep(&self) -> bool {
        self.remote_state[0] >> 7 & 1 != 0
    }

    pub fn set_sleep(&mut self, sleep: bool) {
        self.remote_state[0] = self.remote_state[0] & 0b0111_1111 | (sleep as u8) << 7;
    }

    pub fn temperature(&self) -> Result<Temperature, DecodeError> {
        // TODO: support fahrenheit
        let value = self.remote_state[1] & 0x0F;
        if value <= 30 - 16 {
            Ok(Temperature::Centigrade(value + 16))
        } else {
            Err(DecodeError::InvalidTemperature)
        }
    }

    pub fn set_temperature(&mut self, temp: Temperature) {
        let value = match temp {
            Temperature::Centigrade(degree) if degree >= 16 && degree <= 30 => degree - 16,
            _ => 25 - 16,
        };
        self.remote_state[1] = self.remote_state[1] & 0xF0 | value;
    }

    pub fn timer(&self) -> Result<TimerSetting, DecodeError> {
        TimerSetting::try_from(self.remote_state[1] >> 4 | self.remote_state[2] << 4)
    }

    pub fn set_timer(&mut self, setting: &TimerSetting) {
        let value: u8 = setting.into();
        self.remote_state[1] = self.remote_state[1] & 0x0F | value << 4;
        self.remote_state[2] = self.remote_state[2] & 0xF0 | value & 0x0F;
    }

    pub fn turbo(&self) -> bool {
        self.remote_state[2] >> 4 & 1 != 0
    }

    pub fn set_turbo(&mut self, turbo: bool) {
        self.remote_state[2] = self.remote_state[2] & 0b1110_1111 | (turbo as u8) << 4;
    }

    pub fn light(&self) -> bool {
        self.remote_state[2] >> 5 & 1 != 0
    }

    pub fn set_light(&mut self, light: bool) {
        self.remote_state[2] = self.remote_state[2] & 0b1101_1111 | (light as u8) << 5;
    }

    pub fn health(&self) -> bool {
        self.remote_state[2] >> 6 & 1 != 0
    }

    pub fn set_health(&mut self, health: bool) {
        self.remote_state[2] = self.remote_state[2] & 0b1011_1111 | (health as u8) << 6;
    }

    pub fn dry(&self) -> bool {
        self.remote_state[2] >> 7 & 1 != 0
    }

    pub fn set_dry(&mut self, dry: bool) {
        self.remote_state[2] = self.remote_state[2] & 0b0111_1111 | (dry as u8) << 7;
    }

    pub fn ventilate(&self) -> bool {
        self.remote_state[3] & 1 != 0
    }

    pub fn set_ventilateo(&mut self, ventilate: bool) {
        self.remote_state[3] = self.remote_state[3] & 0b1111_1110 | ventilate as u8;
    }

    pub fn v_swing(&self) -> SwingMode {
        match self.remote_state[4] & 0xF {
            0 => SwingMode::Off,
            1 => SwingMode::On,
            2 => SwingMode::Unknown2,
            3 => SwingMode::Unknown3,
            4 => SwingMode::Unknown4,
            5 => SwingMode::Unknown5,
            6 => SwingMode::Unknown6,
            7 => SwingMode::Unknown7,
            8 => SwingMode::Unknown8,
            9 => SwingMode::Unknown9,
            10 => SwingMode::Unknown10,
            11 => SwingMode::Unknown11,
            12 => SwingMode::Unknown12,
            13 => SwingMode::Unknown13,
            14 => SwingMode::Unknown14,
            15 => SwingMode::Unknown15,
            _ => unsafe { unreachable_unchecked() },
        }
    }

    pub fn set_v_swing(&mut self, mode: SwingMode) {
        self.remote_state[4] = self.remote_state[4] & 0xF0 | mode as u8;
    }

    pub fn h_swing(&self) -> SwingMode {
        match self.remote_state[4] >> 4 {
            0 => SwingMode::Off,
            1 => SwingMode::On,
            2 => SwingMode::Unknown2,
            3 => SwingMode::Unknown3,
            4 => SwingMode::Unknown4,
            5 => SwingMode::Unknown5,
            6 => SwingMode::Unknown6,
            7 => SwingMode::Unknown7,
            8 => SwingMode::Unknown8,
            9 => SwingMode::Unknown9,
            10 => SwingMode::Unknown10,
            11 => SwingMode::Unknown11,
            12 => SwingMode::Unknown12,
            13 => SwingMode::Unknown13,
            14 => SwingMode::Unknown14,
            15 => SwingMode::Unknown15,
            _ => unsafe { unreachable_unchecked() },
        }
    }

    pub fn set_h_swing(&mut self, mode: SwingMode) {
        self.remote_state[4] = self.remote_state[4] & 0x0F | (mode as u8) << 4;
    }

    pub fn temperature_display(&self) -> TemperatureDisplay {
        match self.remote_state[5] & 0b11 {
            0 => TemperatureDisplay::Setting,
            1 => TemperatureDisplay::Room,
            2 => TemperatureDisplay::Indoor,
            3 => TemperatureDisplay::Outdoor,
            _ => unsafe { unreachable_unchecked() },
        }
    }

    pub fn set_temperature_display(&mut self, temp_display: TemperatureDisplay) {
        self.remote_state[5] = self.remote_state[5] & 0b1111_1100 | temp_display as u8;
    }

    pub fn i_feel(&self) -> bool {
        self.remote_state[5] >> 2 & 1 != 0
    }

    pub fn set_i_feel(&mut self, i_feel: bool) {
        self.remote_state[5] = self.remote_state[5] & 0b1111_1011 | (i_feel as u8) << 2;
    }

    pub fn wifi(&self) -> bool {
        self.remote_state[5] >> 6 & 1 != 0
    }

    pub fn set_wifi(&mut self, wifi: bool) {
        self.remote_state[5] = self.remote_state[5] & 0b1011_1111 | (wifi as u8) << 6;
    }

    pub fn econo(&self) -> bool {
        self.remote_state[7] >> 2 & 1 != 0
    }

    pub fn set_econo(&mut self, econo: bool) {
        self.remote_state[7] = self.remote_state[7] & 0b1111_1011 | (econo as u8) << 2;
    }
}

#[derive(Clone, Debug)]
pub enum DecodeError {
    InvalidMarker,
    UnexpectedMarker,
    InvalidMode,
    InvalidTimerSetting,
    InvalidFan,
    InvalidTemperature,
    InvalidSwingMode,
    InvalidMagic,
    Eof,
    Checksum,
}

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    Auto,
    Cold,
    Dry,
    Wind,
    Hot,
}

#[derive(Clone, Copy, Debug)]
pub enum Fan {
    Auto,
    Level1,
    Level2,
    Level3,
}

#[derive(Clone, Copy)]
pub enum Temperature {
    Centigrade(u8),
}

impl Debug for Temperature {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Temperature::Centigrade(degree) => f.write_fmt(format_args!("{} ℃", degree)),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TimerSetting {
    pub enabled: bool,
    pub half_hours: u8,
}

impl TryFrom<u8> for TimerSetting {
    type Error = DecodeError;

    fn try_from(value: u8) -> Result<Self, Self::Error> {
        let half = value & 1;
        let tens = value >> 1 & 0b11;
        let enabled = value >> 3 & 1 != 0;
        let units = value >> 4;
        if tens > 2 || units > 9 {
            Err(DecodeError::InvalidTimerSetting)
        } else {
            Ok(Self {
                enabled,
                half_hours: (tens * 10 + units) * 2 + half,
            })
        }
    }
}

impl Into<u8> for &TimerSetting {
    fn into(self) -> u8 {
        let hours = self.half_hours / 2;
        let half = self.half_hours % 2;
        let tens = hours / 10;
        let units = hours % 10;
        half | tens << 1 | (self.enabled as u8) << 3 | units << 4
    }
}

#[derive(Clone, Copy, Debug)]
pub enum SwingMode {
    Off,
    On,
    Unknown2,
    Unknown3,
    Unknown4,
    Unknown5,
    Unknown6,
    Unknown7,
    Unknown8,
    Unknown9,
    Unknown10,
    Unknown11,
    Unknown12,
    Unknown13,
    Unknown14,
    Unknown15,
}

#[derive(Clone, Copy, Debug)]
pub enum TemperatureDisplay {
    Setting,
    Room,
    Indoor,
    Outdoor,
}

const MAGIC_3: [Code; 3] = [Code::Short, Code::Long, Code::Short];

fn check_magic_code3<'a>(iter: &mut impl Iterator<Item = &'a Code>) -> Result<(), DecodeError> {
    let mut codes = [Code::Short; 3];
    for v in codes.iter_mut() {
        *v = *iter.next().ok_or(DecodeError::Eof)?;
    }
    match codes {
        MAGIC_3 => Ok(()),
        _ => Err(DecodeError::InvalidMagic),
    }
}
