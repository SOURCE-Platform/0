#![cfg(target_os = "linux")]

use evdev::Key;

#[cfg(target_os = "linux")]
pub fn key_to_char(key: Key) -> Option<char> {
    use evdev::Key::*;

    match key {
        KEY_SPACE => Some(' '),
        KEY_A => Some('a'),
        KEY_B => Some('b'),
        KEY_C => Some('c'),
        KEY_D => Some('d'),
        KEY_E => Some('e'),
        KEY_F => Some('f'),
        KEY_G => Some('g'),
        KEY_H => Some('h'),
        KEY_I => Some('i'),
        KEY_J => Some('j'),
        KEY_K => Some('k'),
        KEY_L => Some('l'),
        KEY_M => Some('m'),
        KEY_N => Some('n'),
        KEY_O => Some('o'),
        KEY_P => Some('p'),
        KEY_Q => Some('q'),
        KEY_R => Some('r'),
        KEY_S => Some('s'),
        KEY_T => Some('t'),
        KEY_U => Some('u'),
        KEY_V => Some('v'),
        KEY_W => Some('w'),
        KEY_X => Some('x'),
        KEY_Y => Some('y'),
        KEY_Z => Some('z'),
        KEY_0 => Some('0'),
        KEY_1 => Some('1'),
        KEY_2 => Some('2'),
        KEY_3 => Some('3'),
        KEY_4 => Some('4'),
        KEY_5 => Some('5'),
        KEY_6 => Some('6'),
        KEY_7 => Some('7'),
        KEY_8 => Some('8'),
        KEY_9 => Some('9'),
        _ => None,
    }
}
