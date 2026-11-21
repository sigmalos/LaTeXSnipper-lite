//! MathCraft OCR: the recognition engine and model downloader shared by the
//! LaTeXSnipper desktop app (`backend/`) and the Office add-in host
//! (`office_plugin/host/`). It has no UI or Tauri dependency, so each program
//! links it and builds on its own.

pub mod modelhub;
pub mod ocr;
