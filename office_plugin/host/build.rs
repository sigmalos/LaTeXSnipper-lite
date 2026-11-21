// Embed an Info.plist in the host executable. The host runs as a login agent,
// so macOS shows this identity when it asks to allow controlling Word (document
// variables) and screen recording (screenshot recognition).
fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("macos") {
        return;
    }
    let version = std::env::var("CARGO_PKG_VERSION").unwrap();
    let plist = format!(
        r#"<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleIdentifier</key>
  <string>com.mathcraft.latexsnipper.office.host</string>
  <key>CFBundleName</key>
  <string>LaTeXSnipper Office</string>
  <key>CFBundleShortVersionString</key>
  <string>{version}</string>
  <key>CFBundleVersion</key>
  <string>{version}</string>
  <key>NSAppleEventsUsageDescription</key>
  <string>LaTeXSnipper Office 需要控制 Microsoft Word，以便在文档中保存公式数据。</string>
</dict>
</plist>
"#
    );
    let out = std::path::PathBuf::from(std::env::var("OUT_DIR").unwrap()).join("Info.plist");
    std::fs::write(&out, plist).unwrap();
    println!(
        "cargo:rustc-link-arg-bins=-Wl,-sectcreate,__TEXT,__info_plist,{}",
        out.display()
    );
}
