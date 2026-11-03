//! Usage: `zoraya-mockups --fonts build/fonts --out docs/research/desktop/mockups
//! [--direction NAME] [--scale N]`. Reads `<out>/<direction>/tokens.txt` for
//! each direction and writes `<screen>.png` (scale 1) and `<screen>@2x.png`.

use std::fs;
use std::path::{Path, PathBuf};

use zoraya_mockups::canvas::{Canvas, Face, FontSet};
use zoraya_mockups::png;
use zoraya_mockups::scene::{LOGICAL_HEIGHT, LOGICAL_WIDTH, Painter};
use zoraya_mockups::tokens::{Appearance, Tokens};
use zoraya_mockups::ttf::Font;

type Screen = fn(&mut Painter);

fn usage() -> ! {
    eprintln!("usage: zoraya-mockups --fonts DIR --out DIR [--direction NAME] [--scale N]");
    std::process::exit(2)
}

fn load_fonts(dir: &Path, tokens: &Tokens) -> Result<FontSet, String> {
    let mut set = FontSet::new();
    for (face, key) in [
        (Face::Ui, "font.ui"),
        (Face::UiMedium, "font.ui.medium"),
        (Face::Mono, "font.mono"),
        (Face::MonoBold, "font.mono.bold"),
    ] {
        let name = tokens.str(key);
        let path = dir.join(name);
        let data = fs::read(&path)
            .map_err(|e| format!("{}: {e} (run tools/mockups/fetch-fonts.sh)", path.display()))?;
        let font = Font::parse(data).map_err(|e| format!("{}: {e}", path.display()))?;
        set.insert(face, font);
    }
    Ok(set)
}

fn render(out: &Path, direction: &str, fonts_dir: &Path, scales: &[usize]) -> Result<(), String> {
    let dir = out.join(direction);
    let text = fs::read_to_string(dir.join("tokens.txt"))
        .map_err(|e| format!("{direction}: tokens.txt: {e}"))?;
    let tokens = Tokens::parse(&text).map_err(|e| format!("{direction}: {e}"))?;
    let appearances: Vec<(Appearance, &str)> = match tokens.str("appearance") {
        "dark" => vec![(Appearance::Dark, "")],
        "both" => vec![(Appearance::Light, ""), (Appearance::Dark, "-dark")],
        _ => vec![(Appearance::Light, "")],
    };
    let mut fonts = load_fonts(fonts_dir, &tokens)?;
    let classic: [(&str, Screen); 5] = [
        ("desktop", |p| p.screen_desktop(None)),
        ("terminal", |p| p.screen_terminal()),
        ("chooser", |p| p.screen_chooser()),
        ("settings", |p| p.screen_settings()),
        ("switcher", |p| p.screen_switcher()),
    ];
    let clear: [(&str, Screen); 12] = [
        ("desktop", |p| p.clear_desktop()),
        ("grants", |p| p.clear_grants()),
        ("drop", |p| p.clear_drop()),
        ("chooser", |p| p.clear_chooser()),
        ("rooms", |p| p.clear_rooms()),
        ("switcher", |p| p.clear_switcher()),
        ("settings", |p| p.clear_settings()),
        ("terminal", |p| p.clear_terminal()),
        ("commands", |p| p.clear_commands()),
        ("dock", |p| p.clear_dock_screen()),
        ("dock-compare", |p| p.clear_dock_compare()),
        ("trust", |p| p.clear_trust()),
    ];
    let table: &[(&str, Screen)] = match tokens.str("concept") {
        "classic" => &classic,
        "clear" => &clear,
        other => {
            return Err(format!(
                "{direction}: concept = {other}: not classic or clear"
            ));
        }
    };
    let mut screens: Vec<(&str, Screen)> = Vec::new();
    for name in tokens.str("screens").split_whitespace() {
        let Some(entry) = table.iter().find(|(n, _)| *n == name) else {
            return Err(format!("{direction}: screens: {name}: no such screen"));
        };
        screens.push(*entry);
    }
    // `screens.2x` names the screens rendered at scale 2; without it, all
    let at_2x: Option<Vec<&str>> = tokens
        .has("screens.2x")
        .then(|| tokens.str("screens.2x").split_whitespace().collect());
    for &scale in scales {
        for &(appearance, suffix) in &appearances {
            for &(name, draw) in &screens {
                if scale != 1 && at_2x.as_ref().is_some_and(|l| !l.contains(&name)) {
                    continue;
                }
                let mut canvas = Canvas::new(LOGICAL_WIDTH, LOGICAL_HEIGHT, scale);
                canvas.smooth = tokens.str("radius.shape") == "smooth";
                let mut p = Painter {
                    canvas,
                    wallpaper: None,
                    blurred: None,
                    fonts: &mut fonts,
                    tokens: &tokens,
                    appearance,
                };
                draw(&mut p);
                let file = if scale == 1 {
                    format!("{name}{suffix}.png")
                } else {
                    format!("{name}{suffix}@{scale}x.png")
                };
                let bytes = png::encode(p.canvas.width, p.canvas.height, &p.canvas.px);
                fs::write(dir.join(&file), &bytes).map_err(|e| format!("{file}: {e}"))?;
                println!(
                    "{direction}/{file}: {} x {}, {} bytes",
                    p.canvas.width,
                    p.canvas.height,
                    bytes.len()
                );
            }
        }
    }
    Ok(())
}

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let mut fonts: Option<PathBuf> = None;
    let mut out: Option<PathBuf> = None;
    let mut direction: Option<String> = None;
    let mut scales = vec![1usize, 2];
    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--fonts" => fonts = args.get(i + 1).map(PathBuf::from),
            "--out" => out = args.get(i + 1).map(PathBuf::from),
            "--direction" => direction = args.get(i + 1).cloned(),
            "--scale" => {
                scales = vec![
                    args.get(i + 1)
                        .and_then(|s| s.parse().ok())
                        .unwrap_or_else(|| usage()),
                ]
            }
            _ => usage(),
        }
        i += 2;
    }
    let (Some(fonts), Some(out)) = (fonts, out) else {
        usage()
    };
    let directions: Vec<String> = match direction {
        Some(d) => vec![d],
        None => {
            let mut v: Vec<String> = fs::read_dir(&out)
                .map(|rd| {
                    rd.filter_map(|e| e.ok())
                        .filter(|e| e.path().join("tokens.txt").is_file())
                        .filter_map(|e| e.file_name().into_string().ok())
                        .collect()
                })
                .unwrap_or_default();
            v.sort();
            v
        }
    };
    if directions.is_empty() {
        eprintln!("no directions with a tokens.txt under {}", out.display());
        std::process::exit(1);
    }
    for d in &directions {
        if let Err(e) = render(&out, d, &fonts, &scales) {
            eprintln!("zoraya-mockups: {e}");
            std::process::exit(1);
        }
    }
}
