//! LaTeXSnipper Office for Mac: the local host behind the Word / PowerPoint
//! add-in. It serves the add-in pages over loopback HTTPS and provides the
//! native services they call through `/api`. It runs on its own, without the
//! LaTeXSnipper desktop app; screenshot recognition uses the MathCraft engine
//! linked into this binary.

#[cfg(not(target_os = "macos"))]
compile_error!(
    "LaTeXSnipper Office is the add-in host for Office for Mac and builds on macOS only."
);

pub mod api;
pub mod cert;
pub mod http;
pub mod install;
pub mod omml;
pub mod outlines;
pub mod paths;
pub mod recognition;
pub mod system;
pub mod typography;
pub mod xml;
