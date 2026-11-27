//! `score-screen`: what the viewer actually sees. A client scales the decoded picture to the TV
//! itself, after decoding, so a dump can't show it; a screenshot of a still clip can.
//!
//! ```text
//! encode_quality score-screen --reference frame.png --screen screencap.png --out DIR \
//!   --label firetv-snes-1x [--rect x,y,w,h] [--csv screen.csv]
//! ```
//!
//! The video rectangle is found as the bounding box of the screen's pixels that differ from its
//! corners' colour (or given with `--rect`). The reference is scaled to it by nearest neighbour
//! (each console pixel a sharp block, the ideal display) and every pixel of the rectangle is
//! scored with ΔE2000. The widths of colour edges along the middle row tell how the client
//! scales: a nearest-neighbour scale moves from one colour to the next within a pixel, a
//! bilinear one over about as many pixels as the scale factor.

use std::fs::{self, OpenOptions};
use std::io::Write as _;

use crate::score::{Reference, VISIBLE_DE, delta_e};
use crate::{Options, Result, read_png};

type Rect = (usize, usize, usize, usize);

fn find_rect(screen: &Reference) -> Rect {
    let (w, h) = (screen.width, screen.height);
    let corners = [(0, 0), (w - 1, 0), (0, h - 1), (w - 1, h - 1)].map(|(x, y)| screen.pixel(x, y));
    // Background: anything within a few levels of a corner's colour, or of the gradient behind
    // the player, which changes slowly: compare with the corner on the same side.
    let background = |x: usize, y: usize| {
        let p = screen.pixel(x, y);
        let side = if x < w / 2 { screen.pixel(0, y) } else { screen.pixel(w - 1, y) };
        let near = |c: [u8; 3]| (0..3).all(|i| p[i].abs_diff(c[i]) <= 6);
        near(side) || corners.iter().any(|&c| near(c))
    };
    // The rectangle is the widest run of non-background columns on the middle row, and the
    // tallest run of non-background rows in the middle column.
    let run = |len: usize, at: &dyn Fn(usize) -> bool| {
        let (mut best, mut start) = ((0, 0), None);
        for i in 0..=len {
            match (i < len && at(i), start) {
                (true, None) => start = Some(i),
                (false, Some(s)) => {
                    if i - s > best.1 - best.0 {
                        best = (s, i);
                    }
                    start = None;
                }
                _ => {}
            }
        }
        best
    };
    let (x0, x1) = run(w, &|x| !background(x, h / 2) || !background(x, h / 3));
    let (y0, y1) = run(h, &|y| !background((x0 + x1) / 2, y) || !background(x0 + (x1 - x0) / 3, y));
    (x0, y0, x1 - x0, y1 - y0)
}

pub(crate) fn score_screen(o: &Options) -> Result<()> {
    let reference = read_png(&o.path("reference")?)?;
    let screen = read_png(&o.path("screen")?)?;
    let out = o.path("out")?;
    fs::create_dir_all(&out)?;
    let label = o.get("label").ok_or("--label is required")?;
    let rect: Rect = match o.get("rect") {
        Some(r) => {
            let v: Vec<usize> =
                r.split(',').map(str::parse).collect::<std::result::Result<_, _>>()?;
            (v[0], v[1], v[2], v[3])
        }
        None => find_rect(&screen),
    };
    let (rx, ry, rw, rh) = rect;
    let ideal = |x: usize, y: usize| {
        reference.pixel(
            (x * reference.width / rw).min(reference.width - 1),
            (y * reference.height / rh).min(reference.height - 1),
        )
    };

    let mut des = Vec::with_capacity(rw * rh);
    for y in 0..rh {
        for x in 0..rw {
            des.push(delta_e(ideal(x, y), screen.pixel(rx + x, ry + y)));
        }
    }
    let n = des.len() as f64;
    let mean = des.iter().sum::<f64>() / n;
    let over = 100.0 * des.iter().filter(|&&d| d > VISIBLE_DE).count() as f64 / n;
    let mut sorted = des.clone();
    sorted.sort_by(f64::total_cmp);
    let p99 = sorted[((n * 0.99) as usize).min(sorted.len() - 1)];

    // Edge widths: along rows a quarter, half and three quarters down, every place the ideal
    // picture changes colour, count the screen pixels around it that sit between the two
    // colours (neither within ΔE 2.3 of the left colour nor of the right one).
    let scale = rw as f64 / reference.width as f64;
    let reach = scale.ceil() as usize + 2;
    let mut widths = Vec::new();
    for row in [rh / 4, rh / 2, 3 * rh / 4] {
        for x in reach..rw - reach {
            let (a, b) = (ideal(x - 1, row), ideal(x, row));
            if delta_e(a, b) < 10.0 || ideal(x - reach, row) != a || ideal(x + reach - 1, row) != b
            {
                continue; // only clean edges between two flat colours
            }
            let between = (x - reach..x + reach)
                .filter(|&sx| {
                    let p = screen.pixel(rx + sx, ry + row);
                    delta_e(p, a) > VISIBLE_DE && delta_e(p, b) > VISIBLE_DE
                })
                .count();
            widths.push(between);
        }
    }
    let edges = widths.len();
    let mean_width =
        if edges == 0 { 0.0 } else { widths.iter().sum::<usize>() as f64 / edges as f64 };

    let path = out.join(o.get("csv").unwrap_or("screen.csv"));
    let new = !path.exists();
    let mut csv = OpenOptions::new().create(true).append(true).open(&path)?;
    if new {
        writeln!(csv, "label,rect,scale,de_mean,de_over_2_3_pct,de_p99,edges,edge_width_px")?;
    }
    writeln!(
        csv,
        "{label},{rx}x{ry}+{rw}x{rh},{scale:.2},{mean:.3},{over:.2},{p99:.2},{edges},{mean_width:.2}"
    )?;
    eprintln!(
        "{label}: video {rw}x{rh} at ({rx},{ry}), {scale:.2}x; dE mean {mean:.3}, {over:.1}% > 2.3, \
         p99 {p99:.1}; {edges} clean edges, {mean_width:.2} px between colours on average"
    );
    Ok(())
}
