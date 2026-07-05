/// Arguments carried by a move-like command (G0/G1/G92).
#[derive(Debug, Clone, Copy, Default, PartialEq)]
pub struct MoveArgs {
    pub x: Option<f32>,
    pub y: Option<f32>,
    pub z: Option<f32>,
    pub e: Option<f32>,
    /// Feedrate in mm/min.
    pub f: Option<f32>,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct HomeAxes {
    pub x: bool,
    pub y: bool,
    pub z: bool,
}

impl HomeAxes {
    pub const ALL: HomeAxes = HomeAxes {
        x: true,
        y: true,
        z: true,
    };
    pub const NONE: HomeAxes = HomeAxes {
        x: false,
        y: false,
        z: false,
    };
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Command {
    RapidMove(MoveArgs),
    LinearMove(MoveArgs),
    Home(HomeAxes),
    AbsolutePositioning,
    RelativePositioning,
    SetPosition(MoveArgs),
    ExtruderAbsolute,
    ExtruderRelative,
    SetHotendTemp { celsius: f32 },
    WaitHotendTemp { celsius: f32 },
    SetBedTemp { celsius: f32 },
    WaitBedTemp { celsius: f32 },
}

/// Strips trailing `;` line comments and `(...)` inline comments.
fn strip_comment(line: &str) -> &str {
    let line = match line.find(';') {
        Some(idx) => &line[..idx],
        None => line,
    };
    match line.find('(') {
        Some(idx) => &line[..idx],
        None => line,
    }
}

/// Splits a line into (letter, number) words, e.g. "G1 X10.5 F3000" ->
/// [('G', 1.0), ('X', 10.5), ('F', 3000.0)].
fn tokenize(line: &str) -> Vec<(char, f32)> {
    let mut words = Vec::new();
    for token in line.split_whitespace() {
        let mut chars = token.chars();
        let Some(letter) = chars.next() else {
            continue;
        };
        if !letter.is_ascii_alphabetic() {
            continue;
        }
        let rest = chars.as_str();
        // Bare axis letters (e.g. the `X` in `G28 X Y`) carry no number but
        // still need to register, so treat an empty remainder as 0.
        let value = if rest.is_empty() {
            Some(0.0)
        } else {
            rest.parse::<f32>().ok()
        };
        if let Some(value) = value {
            words.push((letter.to_ascii_uppercase(), value));
        }
    }
    words
}

fn move_args_from_words(words: &[(char, f32)]) -> MoveArgs {
    let mut args = MoveArgs::default();
    for &(letter, value) in words {
        match letter {
            'X' => args.x = Some(value),
            'Y' => args.y = Some(value),
            'Z' => args.z = Some(value),
            'E' => args.e = Some(value),
            'F' => args.f = Some(value),
            _ => {}
        }
    }
    args
}

fn temp_from_words(words: &[(char, f32)]) -> f32 {
    words
        .iter()
        .find(|&&(letter, _)| letter == 'S' || letter == 'R')
        .map(|&(_, value)| value)
        .unwrap_or(0.0)
}

/// Parses a single line of gcode into a `Command`, or `None` for blank
/// lines, comment-only lines, or commands we don't act on.
pub fn parse_line(line: &str) -> Option<Command> {
    let words = tokenize(strip_comment(line));
    let (&(code_letter, code_number), rest) = words.split_first()?;

    match (code_letter, code_number as i32) {
        ('G', 0) => Some(Command::RapidMove(move_args_from_words(rest))),
        ('G', 1) => Some(Command::LinearMove(move_args_from_words(rest))),
        ('G', 28) => {
            let axes = rest.iter().fold(HomeAxes::NONE, |mut acc, &(letter, _)| {
                match letter {
                    'X' => acc.x = true,
                    'Y' => acc.y = true,
                    'Z' => acc.z = true,
                    _ => {}
                }
                acc
            });
            let axes = if axes == HomeAxes::NONE {
                HomeAxes::ALL
            } else {
                axes
            };
            Some(Command::Home(axes))
        }
        ('G', 90) => Some(Command::AbsolutePositioning),
        ('G', 91) => Some(Command::RelativePositioning),
        ('G', 92) => Some(Command::SetPosition(move_args_from_words(rest))),
        ('M', 82) => Some(Command::ExtruderAbsolute),
        ('M', 83) => Some(Command::ExtruderRelative),
        ('M', 104) => Some(Command::SetHotendTemp {
            celsius: temp_from_words(rest),
        }),
        ('M', 109) => Some(Command::WaitHotendTemp {
            celsius: temp_from_words(rest),
        }),
        ('M', 140) => Some(Command::SetBedTemp {
            celsius: temp_from_words(rest),
        }),
        ('M', 190) => Some(Command::WaitBedTemp {
            celsius: temp_from_words(rest),
        }),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_linear_move() {
        let cmd = parse_line("G1 X10.5 Y-2 F1500 E0.5").unwrap();
        assert_eq!(
            cmd,
            Command::LinearMove(MoveArgs {
                x: Some(10.5),
                y: Some(-2.0),
                z: None,
                e: Some(0.5),
                f: Some(1500.0),
            })
        );
    }

    #[test]
    fn strips_comments() {
        assert_eq!(parse_line("; a full comment line"), None);
        let cmd = parse_line("G1 X1 ; move right").unwrap();
        assert_eq!(
            cmd,
            Command::LinearMove(MoveArgs {
                x: Some(1.0),
                ..Default::default()
            })
        );
        let cmd = parse_line("G1 (inline) X1").unwrap();
        assert_eq!(
            cmd,
            Command::LinearMove(MoveArgs {
                ..Default::default()
            })
        );
    }

    #[test]
    fn home_all_when_no_axes_given() {
        assert_eq!(parse_line("G28").unwrap(), Command::Home(HomeAxes::ALL));
    }

    #[test]
    fn home_specific_axes() {
        assert_eq!(
            parse_line("G28 X Y").unwrap(),
            Command::Home(HomeAxes {
                x: true,
                y: true,
                z: false
            })
        );
    }

    #[test]
    fn parses_temperature_commands() {
        assert_eq!(
            parse_line("M104 S200").unwrap(),
            Command::SetHotendTemp { celsius: 200.0 }
        );
        assert_eq!(
            parse_line("M190 R60").unwrap(),
            Command::WaitBedTemp { celsius: 60.0 }
        );
    }

    #[test]
    fn blank_line_is_none() {
        assert_eq!(parse_line("   "), None);
        assert_eq!(parse_line(""), None);
    }
}
