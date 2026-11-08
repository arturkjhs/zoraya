//! `zoraya-tour TOUR SERIAL GUI CONVENTIONS`: writes the two sessions the
//! tour gives and INV-0155-1's, from the descriptions under the current
//! directory (see the library).

use std::process::ExitCode;

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let [tour, serial, gui, conventions] = args.as_slice() else {
        eprintln!("usage: zoraya-tour TOUR SERIAL GUI CONVENTIONS");
        return ExitCode::from(2);
    };
    let text = match std::fs::read_to_string(tour) {
        Ok(t) => t,
        Err(e) => {
            eprintln!("zoraya-tour: {tour}: {e}");
            return ExitCode::FAILURE;
        }
    };
    let c = match zoraya_tour::conventions(&zoraya_tour::described(std::path::Path::new("."))) {
        Ok(c) => c,
        Err(e) => {
            eprintln!("zoraya-tour: {e}");
            return ExitCode::FAILURE;
        }
    };
    for (path, body) in [
        (serial, zoraya_tour::serial(&text)),
        (gui, zoraya_tour::gui(&text)),
        (conventions, c),
    ] {
        if let Err(e) = std::fs::write(path, body) {
            eprintln!("zoraya-tour: {path}: {e}");
            return ExitCode::FAILURE;
        }
    }
    ExitCode::SUCCESS
}
