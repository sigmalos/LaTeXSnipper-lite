//! Backend end-to-end with real ONNX models. The engine with the FP32 formula
//! models (still used by the Office add-in) must reproduce the original Python
//! MathCraft output (`fixtures/python_reference.json`) exactly; the app, which
//! loads the Int8 builds, is checked on the same fixtures.
//!
//! Models are read from `$LATEXSNIPPER_MODELS` or the app data directory; each
//! test is skipped (with a notice) when its models are not installed.

use latexsnipper_lib::commands::{model_status, Core, BUILDS};
use latexsnipper_lib::ocr::{imageops, Engine, FormulaBuilds, FormulaModel, Mode};
use std::path::{Path, PathBuf};

fn fixtures() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

#[cfg(unix)]
fn stage_models(source: &Path, target: &Path) -> std::io::Result<()> {
    std::os::unix::fs::symlink(source, target)
}

#[cfg(windows)]
fn stage_models(source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let destination = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            stage_models(&entry.path(), &destination)?;
        } else {
            std::fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

fn models_dir() -> PathBuf {
    std::env::var("LATEXSNIPPER_MODELS")
        .map(PathBuf::from)
        .unwrap_or_else(|_| latexsnipper_lib::data_dir().join("MathCraft/models"))
}

fn reference() -> serde_json::Value {
    serde_json::from_str(
        &std::fs::read_to_string(fixtures().join("python_reference.json")).unwrap(),
    )
    .unwrap()
}

/// A throwaway data dir containing the real models the app loads (Int8).
fn core_with_models() -> Option<(tempfile::TempDir, Core)> {
    let models = models_dir();
    if !model_status(&models, BUILDS).values().all(|v| *v) {
        eprintln!(
            "SKIP: MathCraft models not found under {}",
            models.display()
        );
        return None;
    }
    let dir = tempfile::tempdir().unwrap();
    std::fs::create_dir_all(dir.path().join("MathCraft")).unwrap();
    stage_models(&models, &dir.path().join("MathCraft/models")).unwrap();
    let core = Core::new(dir.path());
    Some((dir, core))
}

/// The engine itself, with the FP32 formula models, matches Python exactly.
#[test]
fn engine_matches_python_reference_for_every_mode() {
    let models = models_dir();
    let fp32 = FormulaBuilds::default();
    if !model_status(&models, fp32).values().all(|v| *v) {
        eprintln!(
            "SKIP: FP32 MathCraft models not found under {}",
            models.display()
        );
        return;
    }
    let engine = Engine::with_builds(&models, fp32);
    for (name, case) in reference().as_object().unwrap() {
        let mode: Mode = serde_json::from_value(case["mode"].clone()).unwrap();
        let img =
            imageops::decode_rgb(&std::fs::read(fixtures().join(format!("{name}.png"))).unwrap())
                .unwrap();
        let got = engine.recognize(&img, mode).unwrap();
        assert_eq!(got.text, case["text"].as_str().unwrap(), "fixture {name}");
        assert!(got.score > 0.5, "fixture {name} score {}", got.score);
    }
}

/// The app recognizes every fixture with the Int8 models. Quantization changes
/// a symbol here and there, so formulas are checked up to their last symbol;
/// text, which no formula model touches, must match exactly.
#[test]
fn app_recognizes_every_fixture_with_the_int8_models() {
    let Some((_dir, core)) = core_with_models() else {
        return;
    };
    assert_eq!(core.engine().builds(), BUILDS);
    let prefixes = [
        ("frac", "\\frac { a + b } { c ^ { 2 } } ="),
        (
            "sum",
            "\\sum _ { n = 0 } ^ { \\infty } \\frac { x ^ { n } } { n! } =",
        ),
    ];
    for (name, case) in reference().as_object().unwrap() {
        let mode: Mode = serde_json::from_value(case["mode"].clone()).unwrap();
        let bytes = std::fs::read(fixtures().join(format!("{name}.png"))).unwrap();
        let got = core
            .recognize_bytes(&bytes, Some(mode))
            .unwrap()
            .recognition;
        let want = case["text"].as_str().unwrap();
        match prefixes.iter().find(|(n, _)| n == name) {
            Some((_, prefix)) => {
                assert!(got.text.starts_with(prefix), "fixture {name}: {}", got.text)
            }
            None if mode == Mode::Text => assert_eq!(got.text, want, "fixture {name}"),
            None => assert!(!got.text.trim().is_empty(), "fixture {name}"),
        }
        assert!(got.score > 0.5, "fixture {name} score {}", got.score);
    }
}

/// The Int8 formula detector finds the same formulas as the FP32 one on the
/// mixed fixture, so mixed mode still reproduces the Python reference.
#[test]
fn int8_detector_matches_python_reference_in_mixed_mode() {
    let models = models_dir();
    let builds = FormulaBuilds {
        det: FormulaModel::Int8,
        rec: FormulaModel::Standard,
    };
    if !model_status(&models, builds)["mixed"] {
        eprintln!(
            "SKIP: Int8 formula detector or mixed-mode models not found under {}",
            models.display()
        );
        return;
    }
    let reference = reference();
    let engine = Engine::with_builds(&models, builds);
    let img = imageops::decode_rgb(&std::fs::read(fixtures().join("mixed.png")).unwrap()).unwrap();
    let got = engine.recognize(&img, Mode::Mixed).unwrap();
    assert_eq!(got.text, reference["mixed"]["text"].as_str().unwrap());
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
    let source = models_dir();
    if !model_status(&source, BUILDS).values().all(|v| *v) {
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

    for id in latexsnipper_lib::commands::BUILTIN_MODELS {
        let state = core
            .download_model_with(
                id,
                |root, id, _| {
                    std::fs::create_dir_all(root)?;
                    stage_models(&source.join(id), &root.join(id))?;
                    Ok(())
                },
                &mut |_| {},
            )
            .unwrap();
        assert!(state.models.iter().find(|m| m.id == id).unwrap().installed);
    }
    let got = core.recognize_bytes(&bytes, Some(Mode::Mixed)).unwrap();
    assert!(!got.recognition.text.trim().is_empty());
}

/// Deleting a model from Settings makes recognition ask for it again right away,
/// and never reaches through a symlinked model directory into its target.
#[test]
fn deleting_a_model_takes_effect_without_restart() {
    let source = models_dir();
    if !model_status(&source, BUILDS).values().all(|v| *v) {
        eprintln!(
            "SKIP: MathCraft models not found under {}",
            source.display()
        );
        return;
    }
    let (_dir, core) = core_with_each_model_staged(&source);
    let bytes = std::fs::read(fixtures().join("mixed.png")).unwrap();
    assert!(core.recognize_bytes(&bytes, Some(Mode::Mixed)).is_ok());

    let state = core.delete_model("mathcraft-formula-det-int8").unwrap();
    assert!(!state.modes["mixed"] && state.modes["formula"] && state.modes["text"]);
    let err = core.recognize_bytes(&bytes, Some(Mode::Mixed)).unwrap_err();
    assert_eq!(err.to_string(), "MODELS_MISSING:mathcraft-formula-det-int8");
    // Formula mode does not need the deleted model and keeps working.
    let formula = std::fs::read(fixtures().join("frac.png")).unwrap();
    assert!(core.recognize_bytes(&formula, Some(Mode::Formula)).is_ok());
    // The source model files are intact.
    assert!(source
        .join("mathcraft-formula-det-int8/mathcraft-mfd.onnx")
        .is_file());
}

/// Stage model directories individually so the app can remove them.
fn core_with_each_model_staged(source: &Path) -> (tempfile::TempDir, Core) {
    let dir = tempfile::tempdir().unwrap();
    let root = dir.path().join("MathCraft/models");
    std::fs::create_dir_all(&root).unwrap();
    for id in latexsnipper_lib::commands::BUILTIN_MODELS {
        stage_models(&source.join(id), &root.join(id)).unwrap();
    }
    let core = Core::new(dir.path());
    (dir, core)
}
