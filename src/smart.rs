use crate::item::{Action, Icon, Item, Kind};

/// Raycast-style instant answers: unit conversion and hex colors.
pub fn instant_items(query: &str) -> Vec<Item> {
    let mut items = Vec::new();
    if let Some(item) = unit_conversion(query) {
        items.push(item);
    }
    if let Some(item) = hex_color(query) {
        items.push(item);
    }
    if let Some(item) = rgb_color(query) {
        items.push(item);
    }
    items
}

pub fn picker_item(which: impl Fn(&str) -> bool) -> Option<Item> {
    let (program, args) = if which("hyprpicker") {
        ("hyprpicker", vec!["-a".into()])
    } else if which("wl-color-picker") {
        ("wl-color-picker", Vec::new())
    } else {
        return None;
    };
    Some(Item {
        id: "cmd:pick-color".into(),
        title: "Pick color".into(),
        subtitle: format!("Eyedropper via {program}"),
        keywords: "color picker hex rgb hsl hyprpicker".into(),
        kind: Kind::Command,
        icon: Icon::Name("applications-graphics".into()),
        action: Action::Spawn {
            program: program.into(),
            args,
        },
    })
}

pub fn path_command(query: &str) -> Option<Item> {
    let cmd = query.trim();
    if cmd.is_empty()
        || cmd.contains(char::is_whitespace)
        || cmd.starts_with(['>', '$', '=', '/', '~'])
    {
        return None;
    }
    if cmd.chars().count() < 2 || cmd.contains('/') {
        return None;
    }
    if !command_exists(cmd) {
        return None;
    }
    Some(Item {
        id: format!("path:{cmd}"),
        title: format!("Run {cmd}"),
        subtitle: "Executable on PATH".into(),
        keywords: "bin command shell path".into(),
        kind: Kind::Command,
        icon: Icon::Name("utilities-terminal".into()),
        action: Action::Shell {
            command: cmd.to_string(),
            terminal: false,
        },
    })
}

fn unit_conversion(query: &str) -> Option<Item> {
    let (value, from, to) = parse_conversion(query)?;
    let from_u = lookup_unit(&from)?;
    let to_u = lookup_unit(&to)?;
    if from_u.kind != to_u.kind {
        return None;
    }
    let converted = if from_u.kind == UnitKind::Temperature {
        convert_temp(value, from_u.symbol, to_u.symbol)?
    } else {
        value * from_u.to_base / to_u.to_base
    };
    let rendered = format_number(converted);
    let expr = format!("{value} {} → {rendered} {}", from_u.symbol, to_u.symbol);
    Some(Item {
        id: format!("unit:{expr}"),
        title: format!("{rendered} {}", to_u.symbol),
        subtitle: format!("{expr}  ·  copy result"),
        keywords: "convert unit calculator".into(),
        kind: Kind::Calc,
        icon: Icon::Name("accessories-calculator".into()),
        action: Action::Copy(rendered),
    })
}

fn hex_color(query: &str) -> Option<Item> {
    let raw = query.trim();
    let hex = raw.strip_prefix('#').unwrap_or(raw);
    if raw.contains(' ') || !(hex.len() == 3 || hex.len() == 6 || hex.len() == 8) {
        return None;
    }
    if !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    if !raw.starts_with('#') && hex.len() != 6 {
        return None;
    }
    let normalized = normalize_hex(hex);
    let (r, g, b) = rgb(&normalized)?;
    Some(color_item(r, g, b))
}

fn rgb_color(query: &str) -> Option<Item> {
    let raw = query.trim();
    let lower = raw.to_ascii_lowercase();
    let rest = lower
        .strip_prefix("rgba")
        .or_else(|| lower.strip_prefix("rgb"))?;
    if rest.is_empty() {
        return None;
    }
    let prefix_len = raw.len() - rest.len();
    let body = raw[prefix_len..]
        .trim()
        .trim_start_matches('(')
        .trim_end_matches(')')
        .trim();
    let parts: Vec<&str> = body
        .split(|c: char| c == ',' || c.is_whitespace())
        .filter(|part| !part.is_empty())
        .collect();
    if parts.len() < 3 {
        return None;
    }
    let r: u8 = parts[0].parse().ok()?;
    let g: u8 = parts[1].parse().ok()?;
    let b: u8 = parts[2].parse().ok()?;
    Some(color_item(r, g, b))
}

fn color_item(r: u8, g: u8, b: u8) -> Item {
    let hex = format!("#{r:02x}{g:02x}{b:02x}");
    let (h, s, l) = rgb_to_hsl(r, g, b);
    Item {
        id: format!("color:{hex}"),
        title: hex.clone(),
        subtitle: format!("RGB {r}, {g}, {b}  ·  HSL {h}, {s}%, {l}%  ·  copy hex"),
        keywords: "color hex rgb hsl css".into(),
        kind: Kind::Calc,
        icon: Icon::Name("applications-graphics".into()),
        action: Action::Copy(hex),
    }
}

fn rgb_to_hsl(r: u8, g: u8, b: u8) -> (i32, i32, i32) {
    let r = r as f64 / 255.0;
    let g = g as f64 / 255.0;
    let b = b as f64 / 255.0;
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let lightness = (max + min) / 2.0;
    let delta = max - min;
    if delta < 1e-9 {
        return (0, 0, (lightness * 100.0).round() as i32);
    }
    let saturation = delta / (1.0 - (2.0 * lightness - 1.0).abs());
    let hue = if (max - r).abs() < 1e-9 {
        ((g - b) / delta).rem_euclid(6.0)
    } else if (max - g).abs() < 1e-9 {
        (b - r) / delta + 2.0
    } else {
        (r - g) / delta + 4.0
    };
    (
        (hue * 60.0).round() as i32,
        (saturation * 100.0).round() as i32,
        (lightness * 100.0).round() as i32,
    )
}

fn parse_conversion(query: &str) -> Option<(f64, String, String)> {
    let q = query.trim();
    let lower = q.to_ascii_lowercase();
    let split = if let Some(idx) = lower.find(" to ") {
        (" to ", idx)
    } else if let Some(idx) = lower.find(" in ") {
        (" in ", idx)
    } else if let Some(idx) = lower.find(" as ") {
        (" as ", idx)
    } else {
        return parse_compact_temp(q);
    };
    let left = q[..split.1].trim();
    let right = q[split.1 + split.0.len()..].trim();
    if right.is_empty() {
        return None;
    }
    let (value, from) = split_value_unit(left)?;
    Some((value, from, right.to_string()))
}

fn parse_compact_temp(query: &str) -> Option<(f64, String, String)> {
    let q = query.trim();
    let bytes = q.as_bytes();
    if bytes.len() < 2 {
        return None;
    }
    let last = bytes[bytes.len() - 1].to_ascii_lowercase();
    let (from, to) = match last {
        b'f' => ("f", "c"),
        b'c' => ("c", "f"),
        _ => return None,
    };
    let value: f64 = q[..q.len() - 1].trim().parse().ok()?;
    Some((value, from.into(), to.into()))
}

fn split_value_unit(input: &str) -> Option<(f64, String)> {
    let input = input.trim();
    if let Some(idx) = input.find(char::is_alphabetic) {
        let value: f64 = input[..idx].trim().parse().ok()?;
        let unit = input[idx..].trim().to_string();
        if unit.is_empty() {
            return None;
        }
        return Some((value, unit));
    }
    None
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum UnitKind {
    Length,
    Mass,
    Data,
    Temperature,
}

#[derive(Clone, Copy)]
struct Unit {
    symbol: &'static str,
    kind: UnitKind,
    to_base: f64,
}

fn lookup_unit(raw: &str) -> Option<Unit> {
    let key = raw.trim().trim_end_matches('.').to_ascii_lowercase();
    let key = key.trim_end_matches('s');
    let key = match key {
        "kilometer" | "kilometre" => "km",
        "meter" | "metre" => "m",
        "centimeter" | "centimetre" => "cm",
        "millimeter" | "millimetre" => "mm",
        "mile" => "mi",
        "foot" | "feet" | "ft" => "ft",
        "inch" | "inche" => "in",
        "kilogram" => "kg",
        "gram" => "g",
        "pound" => "lb",
        "ounce" => "oz",
        "kilobyte" => "kb",
        "megabyte" => "mb",
        "gigabyte" => "gb",
        "terabyte" => "tb",
        "celsiu" | "celsius" | "centigrade" => "c",
        "fahrenheit" => "f",
        "kelvin" => "k",
        other => other,
    };
    UNITS.iter().copied().find(|unit| unit.symbol == key)
}

const UNITS: &[Unit] = &[
    Unit {
        symbol: "km",
        kind: UnitKind::Length,
        to_base: 1000.0,
    },
    Unit {
        symbol: "m",
        kind: UnitKind::Length,
        to_base: 1.0,
    },
    Unit {
        symbol: "cm",
        kind: UnitKind::Length,
        to_base: 0.01,
    },
    Unit {
        symbol: "mm",
        kind: UnitKind::Length,
        to_base: 0.001,
    },
    Unit {
        symbol: "mi",
        kind: UnitKind::Length,
        to_base: 1609.344,
    },
    Unit {
        symbol: "ft",
        kind: UnitKind::Length,
        to_base: 0.3048,
    },
    Unit {
        symbol: "in",
        kind: UnitKind::Length,
        to_base: 0.0254,
    },
    Unit {
        symbol: "kg",
        kind: UnitKind::Mass,
        to_base: 1.0,
    },
    Unit {
        symbol: "g",
        kind: UnitKind::Mass,
        to_base: 0.001,
    },
    Unit {
        symbol: "lb",
        kind: UnitKind::Mass,
        to_base: 0.45359237,
    },
    Unit {
        symbol: "oz",
        kind: UnitKind::Mass,
        to_base: 0.028349523125,
    },
    Unit {
        symbol: "tb",
        kind: UnitKind::Data,
        to_base: 1024.0 * 1024.0 * 1024.0 * 1024.0,
    },
    Unit {
        symbol: "gb",
        kind: UnitKind::Data,
        to_base: 1024.0 * 1024.0 * 1024.0,
    },
    Unit {
        symbol: "mb",
        kind: UnitKind::Data,
        to_base: 1024.0 * 1024.0,
    },
    Unit {
        symbol: "kb",
        kind: UnitKind::Data,
        to_base: 1024.0,
    },
    Unit {
        symbol: "c",
        kind: UnitKind::Temperature,
        to_base: 1.0,
    },
    Unit {
        symbol: "f",
        kind: UnitKind::Temperature,
        to_base: 1.0,
    },
    Unit {
        symbol: "k",
        kind: UnitKind::Temperature,
        to_base: 1.0,
    },
];

fn convert_temp(value: f64, from: &str, to: &str) -> Option<f64> {
    let c = match from {
        "c" => value,
        "f" => (value - 32.0) * 5.0 / 9.0,
        "k" => value - 273.15,
        _ => return None,
    };
    Some(match to {
        "c" => c,
        "f" => c * 9.0 / 5.0 + 32.0,
        "k" => c + 273.15,
        _ => return None,
    })
}

fn format_number(value: f64) -> String {
    if value.abs() >= 100.0 || (value.fract() - 0.0).abs() < 1e-9 {
        format!("{value:.4}")
            .trim_end_matches('0')
            .trim_end_matches('.')
            .to_string()
    } else {
        let text = format!("{value:.4}");
        text.trim_end_matches('0').trim_end_matches('.').to_string()
    }
}

fn normalize_hex(hex: &str) -> String {
    if hex.len() == 3 {
        hex.chars()
            .flat_map(|c| [c, c])
            .collect::<String>()
            .to_ascii_lowercase()
    } else {
        hex.to_ascii_lowercase()
    }
}

fn rgb(hex: &str) -> Option<(u8, u8, u8)> {
    if hex.len() < 6 {
        return None;
    }
    Some((
        u8::from_str_radix(&hex[0..2], 16).ok()?,
        u8::from_str_radix(&hex[2..4], 16).ok()?,
        u8::from_str_radix(&hex[4..6], 16).ok()?,
    ))
}

fn command_exists(bin: &str) -> bool {
    std::env::var_os("PATH")
        .map(|paths| std::env::split_paths(&paths).any(|dir| dir.join(bin).is_file()))
        .unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::{hex_color, parse_conversion, picker_item, rgb_color, unit_conversion};

    #[test]
    fn converts_length_and_temp() {
        let miles = unit_conversion("10 km to mi").expect("km to mi");
        assert!(miles.title.contains("mi"));
        assert!(miles.title.starts_with('6'));
        let f = unit_conversion("32f").expect("32f");
        assert_eq!(f.title, "0 c");
        let c = unit_conversion("100 celsius in f").expect("c to f");
        assert!(c.title.starts_with("212"));
    }

    #[test]
    fn parse_accepts_words_and_compact() {
        assert_eq!(
            parse_conversion("2.5 GB to mb").map(|(v, f, t)| (
                v,
                f.to_ascii_lowercase(),
                t.to_ascii_lowercase()
            )),
            Some((2.5, "gb".into(), "mb".into()))
        );
        assert_eq!(
            parse_conversion("32F"),
            Some((32.0, "f".into(), "c".into()))
        );
    }

    #[test]
    fn hex_colors_normalize() {
        let color = hex_color("#Ff5A1F").expect("hex");
        assert_eq!(color.title, "#ff5a1f");
        assert!(color.subtitle.contains("HSL"));
        assert!(hex_color("fff").is_none());
        assert!(hex_color("#fff").is_some());
        assert!(hex_color("not-a-color").is_none());
    }

    #[test]
    fn rgb_converts_to_hex_and_hsl() {
        let color = rgb_color("rgb(255, 90, 31)").expect("rgb");
        assert_eq!(color.title, "#ff5a1f");
        assert!(color.subtitle.contains("HSL"));
        assert!(rgb_color("rgb 255 90 31").is_some());
        assert!(rgb_color("rgb").is_none());
        assert!(picker_item(|_| false).is_none());
        assert_eq!(
            picker_item(|bin| bin == "hyprpicker")
                .and_then(|item| match item.action {
                    crate::item::Action::Spawn { program, .. } => Some(program),
                    _ => None,
                })
                .as_deref(),
            Some("hyprpicker")
        );
    }
}
