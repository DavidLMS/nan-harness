use crate::report::Reason;
use num_traits::ToPrimitive as _;
use xa11y::Rect;

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct Window {
    pub(crate) id: u64,
    pub(crate) pid: u32,
    pub(crate) bounds: Rect,
    pub(crate) name: String,
}

pub(crate) struct Snapshot {
    foreground_pid: u32,
    foreground_window: u64,
    displays: Vec<Rect>,
    pub(crate) windows: Vec<Window>,
}

impl Snapshot {
    pub(super) fn parse(text: &str) -> Result<Self, Reason> {
        let mut lines = text.lines();
        let fields = lines
            .next()
            .ok_or(Reason::DesktopUnavailable)?
            .split_whitespace()
            .collect::<Vec<_>>();
        if fields.len() != 3 || fields[0] != "FG" {
            return Err(Reason::DesktopUnavailable);
        }
        let mut snapshot = Self {
            foreground_pid: parse(fields[1])?,
            foreground_window: parse(fields[2])?,
            displays: Vec::new(),
            windows: Vec::new(),
        };
        for line in lines {
            let fields = line.split_whitespace().collect::<Vec<_>>();
            match fields.as_slice() {
                ["DISPLAY", x, y, width, height] => {
                    snapshot.displays.push(rect(x, y, width, height)?);
                }
                ["WIN", id, pid, x, y, width, height, name] => snapshot.windows.push(Window {
                    id: parse(id)?,
                    pid: parse(pid)?,
                    bounds: rect(x, y, width, height)?,
                    name: decode_name(name)?,
                }),
                _ => return Err(Reason::DesktopUnavailable),
            }
            if snapshot.displays.len() > 32 || snapshot.windows.len() > 1024 {
                return Err(Reason::DesktopUnavailable);
            }
        }
        if snapshot.displays.is_empty() {
            return Err(Reason::DesktopUnavailable);
        }
        Ok(snapshot)
    }

    pub(crate) fn require_clear(&self, expected: &Window) -> Result<(), Reason> {
        let index = self
            .windows
            .iter()
            .position(|window| window.id == expected.id && window.pid == expected.pid)
            .ok_or(Reason::WindowChanged)?;
        let current = &self.windows[index];
        if current.bounds != expected.bounds {
            return Err(Reason::WindowChanged);
        }
        if self.foreground_pid != expected.pid
            || (cfg!(windows) && self.foreground_window != expected.id)
            || self.windows[..index]
                .iter()
                .any(|window| window.pid == expected.pid)
        {
            return Err(Reason::FocusChanged);
        }
        if !self
            .displays
            .iter()
            .any(|display| contains(*display, current.bounds))
        {
            return Err(Reason::WindowChanged);
        }
        if self.windows[..index]
            .iter()
            .any(|window| intersects(window.bounds, current.bounds))
        {
            return Err(Reason::WindowOccluded);
        }
        Ok(())
    }
}

fn parse<T: std::str::FromStr>(value: &str) -> Result<T, Reason> {
    value.parse().map_err(|_| Reason::DesktopUnavailable)
}

fn rect(x: &str, y: &str, width: &str, height: &str) -> Result<Rect, Reason> {
    let values = [parse::<f64>(x)?, parse(y)?, parse(width)?, parse(height)?];
    if values
        .iter()
        .any(|value| !value.is_finite() || value.abs() > 65536.0)
        || values[2] <= 0.0
        || values[3] <= 0.0
    {
        return Err(Reason::DesktopUnavailable);
    }
    Ok(Rect {
        x: values[0].to_i32().ok_or(Reason::DesktopUnavailable)?,
        y: values[1].to_i32().ok_or(Reason::DesktopUnavailable)?,
        width: values[2].to_u32().ok_or(Reason::DesktopUnavailable)?,
        height: values[3].to_u32().ok_or(Reason::DesktopUnavailable)?,
    })
}

fn decode_name(value: &str) -> Result<String, Reason> {
    if value == "-" {
        return Ok(String::new());
    }
    if value.len() > 512
        || !value.len().is_multiple_of(2)
        || !value.bytes().all(|byte| byte.is_ascii_hexdigit())
    {
        return Err(Reason::DesktopUnavailable);
    }
    let bytes = value
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let text = std::str::from_utf8(pair).map_err(|_| Reason::DesktopUnavailable)?;
            u8::from_str_radix(text, 16).map_err(|_| Reason::DesktopUnavailable)
        })
        .collect::<Result<Vec<_>, _>>()?;
    String::from_utf8(bytes).map_err(|_| Reason::DesktopUnavailable)
}

pub(crate) fn contains(outer: Rect, inner: Rect) -> bool {
    inner.x >= outer.x
        && inner.y >= outer.y
        && i64::from(inner.x) + i64::from(inner.width)
            <= i64::from(outer.x) + i64::from(outer.width)
        && i64::from(inner.y) + i64::from(inner.height)
            <= i64::from(outer.y) + i64::from(outer.height)
}

fn intersects(left: Rect, right: Rect) -> bool {
    i64::from(left.x) < i64::from(right.x) + i64::from(right.width)
        && i64::from(right.x) < i64::from(left.x) + i64::from(left.width)
        && i64::from(left.y) < i64::from(right.y) + i64::from(right.height)
        && i64::from(right.y) < i64::from(left.y) + i64::from(left.height)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn snapshot() -> Snapshot {
        Snapshot::parse("FG 10 42\nDISPLAY 0 0 1920 1080\nWIN 42 10 100 100 800 600 7a6564\nWIN 43 11 0 0 1920 1080 6f74686572\n").unwrap()
    }

    #[test]
    fn only_the_unchanged_foreground_unoccluded_window_is_safe() {
        let mut state = snapshot();
        let target = state.windows[0].clone();
        assert!(state.require_clear(&target).is_ok());
        state.foreground_pid = 11;
        assert!(state.require_clear(&target).is_err());
        state.foreground_pid = 10;
        let mut other_owned = target.clone();
        other_owned.id = 100;
        other_owned.bounds.x = 1000;
        state.windows.insert(0, other_owned);
        assert_eq!(state.require_clear(&target), Err(Reason::FocusChanged));
        state.windows.remove(0);
        state.windows.swap(0, 1);
        assert!(state.require_clear(&target).is_err());
        state.windows.swap(0, 1);
        state.windows[0].bounds.x += 1;
        assert!(state.require_clear(&target).is_err());
        state.windows[0] = target.clone();
        state.windows[0].id = 99;
        assert!(state.require_clear(&target).is_err());
    }

    #[test]
    fn metadata_is_bounded_and_offscreen_capture_is_rejected() {
        assert!(Snapshot::parse("FG 1 1\nDISPLAY NaN 0 100 100\n").is_err());
        assert!(Snapshot::parse("FG 1 1\nWIN 1 1 0 0 10 10 zz\n").is_err());
        let mut state = snapshot();
        state.windows[0].bounds.x = -1;
        assert!(state.require_clear(&state.windows[0]).is_err());
    }
}
