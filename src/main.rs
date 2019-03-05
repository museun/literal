#![allow(dead_code)]
use std::io::Write;

use chrono::prelude::*;
use multimap::MultiMap;
use rand::prelude::*;
use serde::Deserialize;
use termcolor::{Buffer, BufferWriter, Color, ColorChoice, ColorSpec, WriteColor};

// from https://github.com/JohannesNE/literature-clock
// line 474, in the source, should be on a single line
const ANNOTATED_CSV: &[u8] = include_bytes!("../etc/litclock_annotated.csv");

#[derive(Debug, Clone, Deserialize, PartialEq)]
struct Quote {
    time: String,
    context: String,
    quote: String,
    source: String,
    author: String,
}

impl Quote {
    fn format(
        &self,
        stream: &mut Buffer,
        colors: &ColorSet,
        width: usize,
    ) -> Result<(), std::io::Error> {
        let quote = textwrap::Wrapper::new(width)
            .initial_indent("  ")
            .subsequent_indent("    ")
            .wrap(&self.quote.replace('’', "\'"))
            .join("\n");

        writeln!(stream)?;

        let ctx = self.context.replace('’', "\'").to_ascii_lowercase();

        let mut head = false;
        let mut highlights = vec![];

        for (i, ch) in quote.chars().enumerate() {
            if ch == '\n' {
                head = true;
                continue;
            }

            let z = ch.to_ascii_lowercase();
            if Some(z) == ctx.chars().nth(highlights.len()) {
                highlights.push(i);
                if highlights.len() == ctx.len() {
                    break;
                }
                continue;
            }

            if ch == ' ' && head {
                continue;
            }
            highlights.clear();
            head = false;
        }

        for (i, ch) in quote.replace('\'', "’").chars().enumerate() {
            if highlights.contains(&i) {
                stream.set_color(&colors.highlight)?;
            } else {
                stream.set_color(&colors.inactive)?;
            }
            write!(stream, "{}", ch)?;
            stream.reset()?;
        }

        writeln!(stream)?;
        writeln!(stream)?;

        let attrib = textwrap::Wrapper::new(width)
            .initial_indent("        ")
            .subsequent_indent("        ")
            .wrap(&format!("{} – {}", self.author.trim(), self.source))
            .join("\n");

        stream.set_color(&colors.active)?;
        writeln!(stream, "{}", attrib)?;
        stream.reset()
    }

    fn format_no_wrap(&self, stream: &mut Buffer, colors: &ColorSet) -> Result<(), std::io::Error> {
        let ctx = self.context.to_lowercase();

        let start = self.quote.to_lowercase().find(&ctx).unwrap();
        let end = start + ctx.len();

        writeln!(stream)?;

        stream.set_color(&colors.inactive)?;
        write!(stream, "{}", &self.quote[..start])?;

        stream.set_color(&colors.highlight)?;
        write!(stream, "{}", &self.quote[start..end])?;

        stream.set_color(&colors.inactive)?;
        writeln!(stream, "{}", &self.quote[end..])?;

        writeln!(stream)?;

        stream.set_color(&colors.active)?;
        writeln!(stream, "{:>20} – {}", self.author.trim(), self.source)?;

        stream.reset()
    }
}

struct Database<'a> {
    map: MultiMap<(u8, u8), &'a Quote>,
}

impl<'a> Database<'a> {
    pub fn new(quotes: &'a [Quote]) -> Self {
        Self {
            map: quotes
                .iter()
                .map(|q| (q, &q.time))
                .map(|(q, t)| {
                    let mut t = t.splitn(2, ':').map(|d| d.parse::<u8>().unwrap());
                    ((t.next().unwrap(), t.next().unwrap()), q)
                })
                .collect(),
        }
    }

    pub fn around_time(&self, hh: u8, mm: u8, dir: Direction) -> &Quote {
        let (mut hh, mut mm) = (hh, mm);

        loop {
            match self.at_time(hh, mm) {
                Some(quote) => return quote,
                None => {
                    let (h, m) = Self::next_time(hh, mm, dir);
                    hh = h;
                    mm = m;
                }
            }
        }
    }

    pub fn at_time(&self, hh: u8, mm: u8) -> Option<&Quote> {
        self.map
            .get_vec(&(hh, mm))
            .map(|q| *q.choose(&mut thread_rng()).unwrap())
    }

    fn next_time(hh: u8, mm: u8, dir: Direction) -> (u8, u8) {
        use self::Direction::*;
        match (dir, hh, mm) {
            (Backward, 0, 0) => (23, 59),
            (Backward, .., 0) => (hh - 1, 59),
            (Backward, ..) => (hh, mm - 1),

            (Forward, 23, 59) => (0, 0),
            (Forward, .., 59) => (hh + 1, 0),
            (Forward, ..) => (hh, mm + 1),
        }
    }
}

#[derive(Debug, Copy, Clone)]
enum Direction {
    Forward,
    Backward,
}

#[derive(Debug, Clone)]
struct ColorSet {
    active: ColorSpec,
    inactive: ColorSpec,
    highlight: ColorSpec,
}

fn main() {
    let clock = match std::env::args().nth(1) {
        Some(ref s) if s == "clock" => true,
        _ => false,
    };

    // TODO make this customizable
    let wait = 60;
    let width = 60;

    let mut highlight = ColorSpec::new();
    highlight.set_fg(Some(Color::Red)).set_intense(true);

    let mut inactive = ColorSpec::new();
    inactive.set_fg(Some(Color::White)).set_intense(false);

    let mut active = ColorSpec::new();
    active.set_fg(Some(Color::White)).set_intense(true);

    let color = ColorSet {
        highlight,
        inactive,
        active,
    };

    fn load_quotes() -> Vec<Quote> {
        csv::ReaderBuilder::new()
            .delimiter(b'|')
            .has_headers(false)
            .from_reader(ANNOTATED_CSV)
            .deserialize()
            .filter_map(Result::ok)
            .collect()
    }

    let quotes = load_quotes();
    let db = Database::new(&quotes);

    let stream = BufferWriter::stdout(ColorChoice::Auto);

    let mut last = None;
    loop {
        let now: DateTime<Local> = Local::now();
        let (hh, mm) = (now.hour() as u8, now.minute() as u8);

        // TODO add flag for approx time, and if so, which direction to search
        let quote = db.around_time(hh, mm, Direction::Backward);
        let mut buffer = stream.buffer();
        quote.format(&mut buffer, &color, width).unwrap();

        match last.replace(quote) {
            Some(prev) if prev != quote => stream.print(&buffer).unwrap(),
            None => stream.print(&buffer).unwrap(),
            _ => (),
        }

        if !clock {
            return;
        }

        let diff = (wait - now.second()).into();
        let delta = std::time::Duration::from_secs(diff);
        std::thread::sleep(delta);
    }
}

fn is_timestamp(val: String) -> Result<(), String> {
    let err = String::from("The value must be a valid 24-hour timestamp, HH:MM");

    let mut s = val
        .split(':')
        .map(|d| d.parse::<u8>().map_err(|_| err.clone()));
    match (
        s.next().ok_or_else(|| err.clone())??,
        s.next().ok_or_else(|| err.clone())??,
    ) {
        ((0...23), (0...59)) => Ok(()),
        _ => Err(err),
    }
}

fn is_color(val: String) -> Result<(), String> {
    const COLORS: [&str; 9] = [
        "black", "blue", "green", "red", "cyan", "magenta", "yellow", "white", "grey",
    ];

    if COLORS.contains(&val.to_ascii_lowercase().as_str()) {
        return Ok(());
    }
    Err(format!("Unknown color, available: {}", COLORS.join(", ")))
}
