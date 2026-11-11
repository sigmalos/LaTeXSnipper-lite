//! Backend end-to-end: real ONNX models must reproduce the original Python
//! MathCraft output (`fixtures/python_reference.json`) exactly.
//!
//! Models are read from `$LATEXSNIPPER_MODELS` or the app data directory; the
//! test is skipped (with a notice) when they are not installed.

use latexsnipper_lib::commands::{model_status, Core};
use latexsnipper_lib::ocr::Mode;
use std::path::PathBuf;

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// A throwaway data dir whose `MathCraft/models` points at the real models.
fn core_with_models() -> Option<(tempfile::TempDir, Core)> {
    let models = std::env::var("LATEXSNIPPER_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| latexsnipper_lib::data_dir().join("MathCraft/models"));
    if !model_status(&models).values().all(|v| *v) {
        eprintln!(
            "SKIP: MathCraft models not found under {}",
            models.display()
        );
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("MathCraft")).unwrap();
    std::os::unix::fs::symlink(&models, dir.path().join("MathCraft/models")).unwrap();
    let core = Core::new(dir.path());
    Some((dir, core))
}

#[test]
fn matches_python_reference_for_every_mode() {
    let Some((_dir, core)) = core_with_models() else {
        return;
    };
    let reference: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixtures().join("python_reference.json")).unwrap(),
    )
    .unwrap();
    for (name, case) in reference.as_object().unwrap() {
        let mode: Mode = serde_json::from_value(case["mode"].clone()).unwrap();
        let bytes = std::fs::read(fixtures().join(format!("{name}.png"))).unwrap();
        let got = core.recognize_bytes(&bytes, Some(mode)).unwrap();
        assert_eq!(
            got.recognition.text,
            case["text"].as_str().unwrap(),
            "fixture {name}"
        );
        assert!(
            got.recognition.score > 0.5,
            "fixture {name} score {}",
            got.recognition.score
        );
    }
}

#[test]
fn dark_mode_screenshot_is_normalized() {
    let Some((_dir, core)) = core_with_models() else {
        return;
    };
    let mut img = image::open(fixtures().join("frac.png")).unwrap().to_rgb8();
    image::imageops::invert(&mut img);
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    let got = core.recognize_bytes(&png, Some(Mode::Formula)).unwrap();
    assert!(
        got.recognition.text.starts_with("\\frac { a + b }"),
        "{}",
        got.recognition.text
    );
}

#[test]
fn blank_image_reports_empty_or_low_confidence() {
    let Some((_dir, core)) = core_with_models() else {
        return;
    };
    let img = image::RgbImage::from_pixel(200, 60, image::Rgb([255, 255, 255]));
    let mut png = Vec::new();
    img.write_to(&mut std::io::Cursor::new(&mut png), image::ImageFormat::Png)
        .unwrap();
    match core.recognize_bytes(&png, Some(Mode::Text)) {
        Err(e) => assert_eq!(e.to_string(), "EMPTY_RESULT"),
        Ok(r) => panic!("blank image produced text {:?}", r.recognition.text),
    }
}

#[test]
fn recognition_then_history_roundtrip() {
    let Some((dir, core)) = core_with_models() else {
        return;
    };
    let bytes = std::fs::read(fixtures().join("sum.png")).unwrap();
    let got = core.recognize_bytes(&bytes, None).unwrap();
    assert_eq!(got.kind, "mathcraft");
    core.store
        .lock()
        .unwrap()
        .add_history(&got.recognition.text, got.kind)
        .unwrap();
    let reopened = Core::new(dir.path());
    assert_eq!(
        reopened.snapshot().history.last().unwrap().text,
        got.recognition.text
    );
}

/// The engine built at startup (warmup) must not keep "model not found"
/// after the models are downloaded while the app is running.
#[test]
fn models_downloaded_after_startup_are_used_without_restart() {
    let source = std::env::var("LATEXSNIPPER_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| latexsnipper_lib::data_dir().join("MathCraft/models"));
    if !model_status(&source).values().all(|v| *v) {
        eprintln!(
            "SKIP: MathCraft models not found under {}",
            source.display()
        );
        return;
    }
    let dir = tempfile::tempdir().unwrap();
    let core = Core::new(dir.path());
    let _warm = core.engine(); // what the app does at launch, before any model exists
    let bytes = std::fs::read(fixtures().join("mixed.png")).unwrap();
    let missing = core.recognize_bytes(&bytes, Some(Mode::Mixed)).unwrap_err();
    assert!(
        missing.to_string().starts_with("MODELS_MISSING:"),
        "{missing}"
    );

    for id in [
        "mathcraft-formula-det",
        "mathcraft-formula-rec",
        "mathcraft-text-det",
        "mathcraft-text-rec",
    ] {
        let state = core
            .download_model_with(
                id,
                |root, id, _| {
                    std::fs::create_dir_all(root)?;
                    std::os::unix::fs::symlink(source.join(id), root.join(id))?;
                    Ok(())
                },
                &mut |_| {},
            )
            .unwrap();
        assert!(state.models.iter().find(|m| m.id == id).unwrap().installed);
    }
    let reference: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(fixtures().join("python_reference.json")).unwrap(),
    )
    .unwrap();
    let got = core.recognize_bytes(&bytes, Some(Mode::Mixed)).unwrap();
    assert_eq!(
        got.recognition.text,
        reference["mixed"]["text"].as_str().unwrap()
    );
}

/// Deleting a model from Settings makes recognition ask for it again right away,
/// and never reaches through a symlinked model directory into its target.
#[test]
fn deleting_a_model_takes_effect_without_restart() {
    let source = std::env::var("LATEXSNIPPER_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| latexsnipper_lib::data_dir().join("MathCraft/models"));
    if !model_status(&source).values().all(|v| *v) {
        eprintln!(
            "SKIP: MathCraft models not found under {}",
            source.display()
        );
        return;
    }
    let Some((_dir, core)) = core_with_each_model_linked(&source) else {
        return;
    };
    let bytes = std::fs::read(fixtures().join("mixed.png")).unwrap();
    assert!(core.recognize_bytes(&bytes, Some(Mode::Mixed)).is_ok());

    let state = core.delete_model("mathcraft-formula-det").unwrap();
    assert!(!state.modes["mixed"] && state.modes["formula"] && state.modes["text"]);
    let err = core.recognize_bytes(&bytes, Some(Mode::Mixed)).unwrap_err();
    assert_eq!(err.to_string(), "MODELS_MISSING:mathcraft-formula-det");
    // Formula mode does not need the deleted model and keeps working.
    let formula = std::fs::read(fixtures().join("frac.png")).unwrap();
    assert!(core.recognize_bytes(&formula, Some(Mode::Formula)).is_ok());
    // The real model files behind the link are intact.
    assert!(source
        .join("mathcraft-formula-det/mathcraft-mfd.onnx")
        .is_file());
}

/// A data dir whose model directories are individual symlinks into `source`.
fn core_with_each_model_linked(source: &std::path::Path) -> Option<(tempfile::TempDir, Core)> {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("MathCraft/models");
    std::fs::create_dir_all(&root).unwrap();
    for id in [
        "mathcraft-formula-det",
        "mathcraft-formula-rec",
        "mathcraft-text-det",
        "mathcraft-text-rec",
    ] {
        std::os::unix::fs::symlink(source.join(id), root.join(id)).unwrap();
    }
    let core = Core::new(dir.path());
    Some((dir, core))
}
