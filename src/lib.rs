#![no_std]

use core::{
    fmt::Debug,
    iter::{once, repeat},
};

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

impl TryInto<u8> for Code {
    type Error = DecodeError;

    fn try_into(self) -> Result<u8, Self::Error> {
        match self {
            Code::Start | Code::Continue | Code::End => Err(DecodeError::UnexpectedMarker),
            Code::Short => Ok(0),
            Code::Long => Ok(1),
        }
    }
}

impl TryInto<bool> for Code {
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
pub struct Controller {
    pub mode: Mode,
    pub on: bool,
    pub fan: Fan,
    pub swing: bool,
    pub sleep: bool,
    pub temperature: Temperature,
    pub timing: TimerSetting,
    pub strong: bool,
    pub light: bool,
    pub anion: bool, // health?
    pub dry: bool,   // power saving?
    pub ventilate: bool,

    pub v_swing: SwingMode,
    pub h_swing: SwingMode,
    pub temperature_display: TemperatureDisplay,
    pub i_feel: bool,
    pub wifi: bool,
    pub econo: bool,
}

impl Controller {
    pub fn encode(&self) -> impl Iterator<Item = Code> {
        let code35 = (self.mode.encode())
            .chain(once(Code::from(self.on)))
            .chain(self.fan.encode())
            .chain(once(Code::from(self.swing)))
            .chain(once(Code::from(self.sleep)))
            .chain(self.temperature.encode())
            .chain(self.timing.encode())
            .chain(once(Code::from(self.strong)))
            .chain(once(Code::from(self.light)))
            .chain(once(Code::from(self.anion)))
            .chain(once(Code::from(self.dry)))
            .chain(once(Code::from(self.ventilate)))
            .chain(MAGIC_1.into_iter())
            .chain(MAGIC_3.into_iter());
        let code32 = (self.v_swing.encode())
            .chain(self.h_swing.encode())
            .chain(self.temperature_display.encode())
            .chain(once(Code::from(self.i_feel)))
            .chain(MAGIC_4.into_iter())
            .chain(once(Code::from(self.wifi)))
            .chain(repeat(Code::Short).take(11))
            .chain(once(Code::from(self.econo)))
            .chain(once(Code::Short))
            .chain({
                let checksum = self.checksum();
                (0..4).map(move |i| Code::from(checksum >> i & 0x1 != 0))
            });
        once(Code::Start)
            .chain(code35)
            .chain(once(Code::Continue))
            .chain(code32)
            .chain(once(Code::End))
    }

    pub fn decode(codes: &[Code; 70]) -> Result<Self, DecodeError> {
        let (Code::Start, Code::Continue, Code::End) = (codes[0], codes[36], codes[69]) else {
            return Err(DecodeError::InvalidMarker);
        };
        let mut iter = codes.iter().copied();
        let Code::Start = iter.next().ok_or(DecodeError::Eof)? else {
            return Err(DecodeError::InvalidMarker);
        };
        let mode = Mode::decode(&mut iter)?;
        let on: bool = iter.next().ok_or(DecodeError::Eof)?.try_into()?;
        let fan = Fan::decode(&mut iter)?;
        let swing: bool = iter.next().ok_or(DecodeError::Eof)?.try_into()?;
        let sleep: bool = iter.next().ok_or(DecodeError::Eof)?.try_into()?;
        let temperature = Temperature::decode(&mut iter)?;
        let timing = TimerSetting::decode(&mut iter)?;
        let humidification: bool = iter.next().ok_or(DecodeError::Eof)?.try_into()?;
        let light: bool = iter.next().ok_or(DecodeError::Eof)?.try_into()?;
        let anion: bool = iter.next().ok_or(DecodeError::Eof)?.try_into()?;
        let dry: bool = iter.next().ok_or(DecodeError::Eof)?.try_into()?;
        let ventilate: bool = iter.next().ok_or(DecodeError::Eof)?.try_into()?;
        check_magic_code1(&mut iter)?;
        check_magic_code2(&mut iter)?;
        let Code::Continue = iter.next().ok_or(DecodeError::Eof)? else {
            return Err(DecodeError::InvalidMarker);
        };

        let v_swing = SwingMode::decode(&mut iter)?;
        let h_swing = SwingMode::decode(&mut iter)?;
        let temperature_display = TemperatureDisplay::decode(&mut iter)?;
        let i_feel = iter.next().ok_or(DecodeError::Eof)?.try_into()?;
        check_magic_code3(&mut iter)?;
        let wifi = iter.next().ok_or(DecodeError::Eof)?.try_into()?;

        let power_save: bool = iter.nth(11).ok_or(DecodeError::Eof)?.try_into()?;
        _ = iter.next().ok_or(DecodeError::Eof)?;

        let value = Self {
            mode,
            on,
            fan,
            swing,
            sleep,
            temperature,
            timing,
            strong: humidification,
            light,
            anion,
            dry,
            ventilate,
            v_swing,
            h_swing,
            temperature_display,
            i_feel,
            wifi,
            econo: power_save,
        };

        let mut checksum = 0_u8;
        for i in 0..4 {
            let t: u8 = iter.next().ok_or(DecodeError::Eof)?.try_into()?;
            checksum |= t << i;
        }
        let checksum_calc = value.checksum();
        if checksum != checksum_calc {
            return Err(DecodeError::Checksum(checksum << 4 | checksum_calc));
        }
        let Code::End = iter.next().ok_or(DecodeError::Eof)? else {
            return Err(DecodeError::InvalidMarker);
        };

        Ok(value)
    }

    fn checksum(&self) -> u8 {
        let blocks = [
            self.mode as u8 | (self.on as u8) << 3,
            self.temperature.0 - 16,
            0x0, // timing
            self.ventilate as u8 | 0b01010000,
            self.v_swing as u8 | (self.h_swing as u8) << 4,
            self.temperature_display as u8 | (self.i_feel as u8) << 2 | 0b100 << 3,
            0x00,
        ];
        Self::checksum_block(&blocks[..])
    }

    fn checksum_block(data: &[u8]) -> u8 {
        let mut sum = 10;
        // Sum the lower half of the first 4 bytes of this block.
        for v in data.iter().take(4) {
            sum += *v & 0xF;
        }
        // then sum the upper half of the next 3 bytes.
        for v in data.iter().skip(4).take(3) {
            sum += *v >> 4;
        }
        // Trim it down to fit into the 4 bits allowed. i.e. Mod 16.
        sum & 0xF
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
    InvalidMagic(u8),
    Eof,
    Checksum(u8),
}

#[derive(Clone, Copy, Debug)]
pub enum Mode {
    Auto,
    Cold,
    Dry,
    Wind,
    Hot,
}

impl Mode {
    fn encode(&self) -> impl Iterator<Item = Code> {
        let a = *self as u8;
        [a >> 0, a >> 1, a >> 2]
            .into_iter()
            .map(|x| x & 0x1 != 0)
            .map(Code::from)
    }

    fn decode(codes: &mut impl Iterator<Item = Code>) -> Result<Self, DecodeError> {
        let a: u8 = codes.next().ok_or(DecodeError::Eof)?.try_into()?;
        let b: u8 = codes.next().ok_or(DecodeError::Eof)?.try_into()?;
        let c: u8 = codes.next().ok_or(DecodeError::Eof)?.try_into()?;
        match a << 0 | b << 1 | c << 2 {
            0 => Ok(Mode::Auto),
            1 => Ok(Mode::Cold),
            2 => Ok(Mode::Dry),
            3 => Ok(Mode::Wind),
            4 => Ok(Mode::Hot),
            _ => Err(DecodeError::InvalidMode),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum Fan {
    Auto,
    Level1,
    Level2,
    Level3,
}

impl Fan {
    fn encode(&self) -> impl Iterator<Item = Code> {
        let a = *self as u8;
        [a >> 0, a >> 1]
            .into_iter()
            .map(|x| x & 0x1 != 0)
            .map(Code::from)
    }

    fn decode(codes: &mut impl Iterator<Item = Code>) -> Result<Self, DecodeError> {
        let a: u8 = codes.next().ok_or(DecodeError::Eof)?.try_into()?;
        let b: u8 = codes.next().ok_or(DecodeError::Eof)?.try_into()?;
        match a << 0 | b << 1 {
            0 => Ok(Fan::Auto),
            1 => Ok(Fan::Level1),
            2 => Ok(Fan::Level2),
            3 => Ok(Fan::Level3),
            _ => Err(DecodeError::InvalidFan),
        }
    }
}

#[derive(Clone, Copy)]
pub struct Temperature(u8);

impl Temperature {
    pub fn from_centigrade(degrees: u8) -> Option<Self> {
        if degrees < 16 || degrees > 30 {
            None
        } else {
            Some(Self(degrees))
        }
    }

    fn encode(&self) -> impl Iterator<Item = Code> {
        let a = self.0 - 16;
        [a >> 0, a >> 1, a >> 2, a >> 3]
            .into_iter()
            .map(|x| x & 0x1 != 0)
            .map(Code::from)
    }

    fn decode(codes: &mut impl Iterator<Item = Code>) -> Result<Self, DecodeError> {
        let mut a = 0;
        for i in 0..4 {
            let code = codes.next().ok_or(DecodeError::Eof)?;
            let t: u8 = code.try_into()?;
            a |= t << i;
        }
        if a > 30 - 16 {
            Err(DecodeError::InvalidTemperature)
        } else {
            Ok(Self(a + 16))
        }
    }
}

impl Debug for Temperature {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_fmt(format_args!("{} ℃", self.0))
    }
}

#[derive(Clone, Copy, Debug)]
pub struct TimerSetting {
    pub enabled: bool,
    pub half_hours: u8,
}

impl TimerSetting {
    fn encode(&self) -> impl Iterator<Item = Code> {
        let a: u8 = self.into();
        (0..8).map(move |i| Code::from(a >> i & 1 != 0))
    }

    fn decode(codes: &mut impl Iterator<Item = Code>) -> Result<Self, DecodeError> {
        let mut a = 0u8;
        for i in 0..8 {
            let t: u8 = codes.next().ok_or(DecodeError::Eof)?.try_into()?;
            a |= t << i;
        }
        Self::try_from(a)
    }
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

impl SwingMode {
    fn encode(&self) -> impl Iterator<Item = Code> {
        let a: u8 = *self as u8;
        (0..4).map(move |i| Code::from(a >> i & 1 != 0))
    }

    fn decode(codes: &mut impl Iterator<Item = Code>) -> Result<Self, DecodeError> {
        let mut a = 0u8;
        for i in 0..4 {
            let t: u8 = codes.next().ok_or(DecodeError::Eof)?.try_into()?;
            a |= t << i;
        }
        match a {
            1 => Ok(Self::On),
            2 => Ok(Self::Unknown2),
            3 => Ok(Self::Unknown3),
            4 => Ok(Self::Unknown4),
            5 => Ok(Self::Unknown5),
            6 => Ok(Self::Unknown6),
            7 => Ok(Self::Unknown7),
            8 => Ok(Self::Unknown8),
            9 => Ok(Self::Unknown9),
            10 => Ok(Self::Unknown10),
            11 => Ok(Self::Unknown11),
            12 => Ok(Self::Unknown12),
            13 => Ok(Self::Unknown13),
            14 => Ok(Self::Unknown14),
            15 => Ok(Self::Unknown15),
            _ => Ok(Self::Off),
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum TemperatureDisplay {
    Setting,
    Room,
    Indoor,
    Outdoor,
}

impl TemperatureDisplay {
    fn encode(&self) -> impl Iterator<Item = Code> {
        let a: u8 = *self as u8;
        (0..2).map(move |i| Code::from(a >> i & 1 != 0))
    }

    fn decode(codes: &mut impl Iterator<Item = Code>) -> Result<Self, DecodeError> {
        let a: bool = codes.next().ok_or(DecodeError::Eof)?.try_into()?;
        let b: bool = codes.next().ok_or(DecodeError::Eof)?.try_into()?;
        match (a, b) {
            (false, false) => Ok(Self::Setting),
            (true, false) => Ok(Self::Room),
            (false, true) => Ok(Self::Indoor),
            (true, true) => Ok(Self::Outdoor),
        }
    }
}

const MAGIC_1: [Code; 7] = [
    Code::Short,
    Code::Short,
    Code::Short,
    Code::Long,
    Code::Short,
    Code::Long,
    Code::Short,
];
const MAGIC_2: [Code; 7] = [
    Code::Short,
    Code::Short,
    Code::Short,
    Code::Long,
    Code::Long,
    Code::Long,
    Code::Short,
];
const MAGIC_3: [Code; 3] = [Code::Short, Code::Long, Code::Short];
const MAGIC_4: [Code; 3] = [Code::Short, Code::Short, Code::Long];

fn check_magic_code1(iter: &mut impl Iterator<Item = Code>) -> Result<(), DecodeError> {
    let mut codes = [Code::Short; 7];
    for v in codes.iter_mut() {
        *v = iter.next().ok_or(DecodeError::Eof)?;
    }
    match codes {
        MAGIC_1 | MAGIC_2 => Ok(()),
        _ => Err(DecodeError::InvalidMagic(1)),
    }
}

fn check_magic_code2(iter: &mut impl Iterator<Item = Code>) -> Result<(), DecodeError> {
    let mut codes = [Code::Short; 3];
    for v in codes.iter_mut() {
        *v = iter.next().ok_or(DecodeError::Eof)?;
    }
    match codes {
        MAGIC_3 => Ok(()),
        _ => Err(DecodeError::InvalidMagic(2)),
    }
}

fn check_magic_code3(iter: &mut impl Iterator<Item = Code>) -> Result<(), DecodeError> {
    let mut codes = [Code::Short; 3];
    for v in codes.iter_mut() {
        *v = iter.next().ok_or(DecodeError::Eof)?;
    }
    match codes {
        MAGIC_4 => Ok(()),
        _ => Err(DecodeError::InvalidMagic(3)),
    }
}
