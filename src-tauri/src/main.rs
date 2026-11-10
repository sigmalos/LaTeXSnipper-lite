fn main() {
    // `LaTeXSnipper --selftest <image>`: headless OCR check used by the DMG smoke test.
    let args: Vec<String> = std::env::args().collect();
    if args.get(1).map(String::as_str) == Some("--selftest") {
        std::process::exit(latexsnipper_lib::selftest(args.get(2).map(String::as_str)));
    }
    latexsnipper_lib::run();
}
